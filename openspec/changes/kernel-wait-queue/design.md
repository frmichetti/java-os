# Design

## Context

Ver `proposal.md` — Why/What Changes. Pesquisa feita no código real do escalonador (`kernel/src/scheduler.rs`, `kernel/src/process.rs`) antes deste design:

- `ReadyQueue` (`scheduler.rs`) é só um `VecDeque<ThreadId>` com um único método `advance()`: sempre remove a thread da frente, **sempre** a recoloca no fim, e devolve `(quem estava rodando, quem roda a seguir)`. Não existe hoje nenhum conceito de thread "não pronta" — toda thread registrada está sempre, incondicionalmente, em rotação.
- A única troca de contexto que existe é feita por `on_timer_tick`, chamada exclusivamente a partir da rotina de entrada do timer em assembly (`timer_entry`, instalada no vetor 32 da IDT — `kernel::timer::TIMER_VECTOR`). Essa rotina empilha todos os registradores de uso geral, chama `on_timer_tick(current_rsp)` (que decide a próxima thread via `ReadyQueue::advance()`, atualiza `TSS.rsp0`/`CURRENT_KERNEL_STACK_TOP` e troca `CR3`), recebe de volta o `rsp` salvo da próxima thread, o restaura em `RSP`, desempilha os registradores e executa `iretq`.
- Não existe nenhuma outra forma de trocar de contexto hoje — nenhuma rotina de "cessão voluntária" (`yield`) existe.
- `kernel::sync::with_interrupts_disabled` (usado por `kernel::keyboard::poll_event`, ver `design.md` arquivado de `kernel-keyboard`) já estabelece o padrão para evitar impasse entre uma ISR e uma thread comum disputando o mesmo recurso — precedente direto para o requisito de "despertar a partir de uma ISR" desta change.
- A spec `kernel-scheduler` já tem um requisito ("Comportamento Ocioso sem Threads Prontas") prevendo que o kernel entre em estado ocioso quando nenhuma thread está pronta — hoje esse caminho nunca é de fato exercitado, porque nada jamais remove uma thread da `ReadyQueue` (o `.expect(...)` em `on_timer_tick` nunca dispara na prática). Esta change introduz, pela primeira vez, uma forma real de uma thread sair da rotação — ver "Riscos / Trade-offs" sobre como isso é tratado.
- `kernel::main::continue_boot` registra a thread de boot (`register_boot_thread`) e termina em `idle_loop()` (`sti` + `hlt` em laço) — essa thread nunca chama nenhuma API de bloqueio nesta change.

## Goals / Non-Goals

**Goals:**
- Uma primitiva `WaitQueue` com a qual a thread corrente pode bloquear (sair da rotação do escalonador sem busy-wait) e outra thread — ou uma ISR — pode acordar uma ou todas as threads bloqueadas nela.
- Reaproveitar o mecanismo de salvar/restaurar contexto já existente e comprovado (`timer_entry`/`on_timer_tick`), em vez de duplicar essa lógica numa rotina de troca de contexto paralela.
- Lógica pura e testável em host para a parte de dados (fila de prontos vs. fila de bloqueados), seguindo o padrão já estabelecido nos testes existentes de `ReadyQueue`.
- Um demo em modo kernel que prova o mecanismo de ponta a ponta em boot real.

**Non-Goals (nesta change):**
- `pthread_mutex`/`pthread_cond` como API nomeada, ou qualquer API exposta a modo usuário (nenhuma *syscall* nova) — ficam para sub-changes futuras do item 8 do roadmap, construídas sobre esta.
- Bloqueio com *timeout* (a thread bloqueada só retoma quando explicitamente acordada, nunca sozinha após um tempo) — o `Parker` do HotSpot eventualmente precisa de ambos os modos, mas o modo sem *timeout* já é suficiente para provar o mecanismo.
- Qualquer tratamento genérico do caso em que **todas** as threads do sistema bloqueiam simultaneamente (a fila de prontos ficar de fato vazia) — ver "Riscos / Trade-offs".
- Qualquer noção de prioridade entre threads bloqueadas na mesma fila — ordem de despertar é FIFO simples, mesmo espírito "sem fila sofisticada ainda" já aceito em outras changes deste projeto.
- Encerramento/remoção de thread — Non-Goal já herdado de `kernel-process-scheduler`, inalterado aqui.

## Decisions

### Reaproveitar `int <vetor do timer>` para reentrar no caminho de troca de contexto já existente, em vez de uma rotina de troca de contexto nova
Bloquear uma thread exige trocar de contexto *fora* de um tick de hardware do timer — algo que hoje não existe. Duas abordagens foram consideradas:

1. **Escolhida**: a thread que quer bloquear registra, numa variável de estado protegida por `with_interrupts_disabled` (ex.: "a thread X pretende ser desviada para a fila de espera Y, não recolocada na `ReadyQueue`"), e então executa a instrução `int` no mesmo vetor já usado pelo timer (`kernel::timer::TIMER_VECTOR`). A CPU trata isso exatamente como entraria num tick de hardware do timer: empilha o mesmo formato de frame, cai na mesma `timer_entry`/`on_timer_tick` já existente e comprovada. `on_timer_tick` é estendida para, antes de decidir o destino de `was_running`, checar esse registro: se bater com `was_running`, move a thread para a `WaitQueue` indicada em vez de recolocá-la no fim da `ReadyQueue` (o comportamento padrão, inalterado, para qualquer tick normal do timer ou para uma thread que não pediu bloqueio). Quando a thread for mais tarde escolhida de novo pelo escalonador, a execução retoma exatamente depois da instrução `int` — o mesmo contrato que `on_timer_tick` já garante para qualquer interrupção.
2. **Descartada**: uma rotina de troca de contexto nova e separada (salvando só os registradores *callee-saved*, no estilo clássico de `context_switch` cooperativo de outros kernels). Mais enxuta em teoria, mas duplica — com um formato de frame diferente do já existente — a lógica de salvar/restaurar contexto, dobrando a superfície de código que precisa estar certa (dois formatos de frame a manter coerentes em vez de um só) sem nenhum ganho de desempenho relevante neste estágio do projeto.

A decisão 1 foi escolhida por reusar 100% do mecanismo já testado em boot real (inclusive a troca de `CR3`/`TSS.rsp0` que `on_timer_tick` já faz), reduzindo o risco a uma extensão pontual da lógica de decisão de `on_timer_tick`, não a uma nova peça de assembly.

### `ReadyQueue` ganha um ponto de decisão explícito sobre o destino da thread interrompida, em vez de sempre recolocá-la no fim
Hoje `ReadyQueue::advance()` sempre recoloca `was_running` no fim da fila — essa função é dividida em passos menores (remover a thread da frente; decidir seu destino; obter a próxima da fila) para que `on_timer_tick` possa, com a informação registrada pela decisão acima, escolher entre "recolocar no fim" (comportamento padrão, qualquer tick normal) ou "mover para a `WaitQueue` indicada" (só quando a própria thread pediu bloqueio antes de disparar a interrupção de software). A lógica de fila em si (prontos vs. bloqueados por fila de espera) continua pura e testável em host, sem nenhum acesso a hardware — mesmo padrão dos testes já existentes de `ReadyQueue`.

### `WaitQueue`: FIFO simples, `wake_one`/`wake_all`, sem associação a um valor de memória específico
A primitiva escolhida é uma fila de espera nomeada (`WaitQueue::new()`, uma instância `static` por "canal" de espera, mesmo padrão já usado para outros estados globais do kernel como `kernel::keyboard::QUEUE`) — não um "futex" genérico associado a um endereço/valor de memória arbitrário. `block_current()` bloqueia a thread corrente nessa fila; `wake_one()`/`wake_all()` movem uma ou todas as threads bloqueadas de volta para a `ReadyQueue`, em ordem FIFO.

**Alternativa descartada**: uma primitiva estilo `futex` (bloquear condicionado ao valor atual de uma palavra de memória, só retomando se esse valor ainda não mudou). Mais próxima da semântica final de `pthread_cond`/`Parker`, mas exige uma camada adicional de comparação atômica de valor que não é necessária só para provar o mecanismo de bloqueio/despertar em si — fica para quando a sub-change de `pthread_mutex`/`pthread_cond` precisar dela de fato.

### Demo: duas threads de kernel compartilhando uma `WaitQueue` estática
Seguindo o padrão já estabelecido (`keyboard_demo`, `storage_demo`, `network_demo`), `kernel::wait_demo` cria duas threads via `kernel::scheduler::spawn_sharing_current_address_space` (sem isolamento de endereço próprio, mesmo racional já documentado em `kernel-keyboard` sobre evitar uma troca de `CR3` extra por tick): uma "produtora" que, a cada N ticks do temporizador, chama `wake_one()`; uma "consumidora" que fica em laço chamando `block_current()` e reportando cada despertar pelo console serial. Prova em boot real que uma thread realmente sai de rotação (não aparece mais round-robin enquanto bloqueada) e retoma corretamente quando acordada.

## Riscos / Trade-offs

- **[Risco] Esta change introduz, pela primeira vez, uma forma real de a `ReadyQueue` ficar vazia (todas as threads registradas bloqueadas simultaneamente) — cenário que `on_timer_tick` hoje trataria com um `.expect(...)` (pânico), não com o estado ocioso que a spec `kernel-scheduler` já promete** → Mitigado nesta change mantendo uma invariante deliberada: a thread de boot (`continue_boot`/`idle_loop`) nunca chama `block_current()`, permanecendo sempre pronta. Como ela é registrada antes de qualquer outra thread e nunca é removida da rotação, a `ReadyQueue` nunca fica de fato vazia nesta change — o requisito "Comportamento Ocioso sem Threads Prontas" continua válido por construção (nunca exercitado), não porque esta change o implementou de verdade. Implementar o caso genérico (toda thread real bloqueada, só a ociosidade de hardware restando) fica para quando um consumidor real exigir isso — mesmo espírito de "adiar complexidade sem uso real ainda" já aplicado em changes anteriores (ex.: enumeração PCI só do barramento 0 em `kernel-storage`).
- **[Risco] Acordar uma thread bloqueada a partir de uma ISR pode reintroduzir a mesma classe de impasse já corrigida uma vez em `kernel-keyboard`** (uma ISR tentando adquirir um lock que a própria thread interrompida já segura) → Mitigado seguindo o mesmo precedente: todo acesso a `ReadyQueue`/`WaitQueue`/`CONTEXTS` a partir de `wake_one`/`wake_all`, de dentro de uma ISR ou não, usa `with_interrupts_disabled`, nunca mantém o lock além da seção crítica mínima, e nenhuma ISR deste projeto faz *busy-wait* num lock que uma thread comum poderia já deter durante uma seção crítica mais longa.
- **[Trade-off] `WaitQueue` nomeada e estática, em vez de uma primitiva genérica tipo futex associada a qualquer endereço de memória** → Aceito deliberadamente (ver Decisions): suficiente para provar o mecanismo; a generalização fica para quando `pthread_cond` precisar dela de fato.
- **[Trade-off] Sem suporte a *timeout* nesta primeira versão** → Aceito: o `Parker` do HotSpot usa park com e sem *timeout*, mas provar o caminho sem *timeout* primeiro é a mesma disciplina incremental já seguida em toda change anterior deste projeto.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: `cargo test -p kernel --lib` continua passando sem regressão (os testes já existentes de `ReadyQueue`/`Context` continuam valendo; novos testes puros cobrem a divisão prontos/bloqueados e a ordem FIFO de despertar), `cargo kernel-build`/`cargo xtask test` continuam passando sem regressão de comportamento esperado no console serial, e uma verificação manual em QEMU confirma que a thread consumidora do demo realmente para de aparecer na rotação enquanto bloqueada e retoma corretamente ao ser despertada pela produtora. Rollback é reverter os commits desta change; nenhum dado persistente é afetado.
