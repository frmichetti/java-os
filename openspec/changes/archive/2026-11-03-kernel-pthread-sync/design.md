# Design

## Context

Ver `proposal.md` — Why/What Changes. Estado atual relevante, pesquisado no código real antes deste design:

- `kernel::wait_queue::WaitQueue` (`kernel/src/wait_queue.rs`) já fornece `block_current()` (bloqueia a thread corrente, sai de rotação), `wake_one()`/`wake_all()` (devolvem threads bloqueadas à rotação) e o mecanismo de "intenção de bloqueio" (`BLOCK_INTENT`, consumido por `kernel::scheduler::schedule_next` via `take_block_intent`). `block_current()` é hoje uma operação monolítica: registra a intenção e imediatamente dispara `int {BLOCK_VECTOR}` — não há como "registrar a intenção, mas só efetivar o bloqueio se uma condição ainda for verdadeira nesse instante".
- `kernel::sync::Spinlock` é exclusão mútua por *busy-wait* puro — adequada para seções críticas curtas internas do kernel (`READY_QUEUE`, `CONTEXTS`, etc.), mas inadequada como base de `pthread_mutex` (que precisa de uma thread esperando sair de rotação, não girando).
- `kernel::scheduler::with_interrupts_disabled` já é o mecanismo padrão do projeto para tornar uma seção crítica atômica em relação a interrupções (usado por `WaitQueue` e por `kernel::keyboard::poll_event`).
- O bug corrigido recentemente em `kernel::serial::_print`/`MirroredWriter` (ver `KNOW_ISSUES.md`, "Duas threads de kernel imprimindo concorrentemente travam o timer de hardware para sempre", RESOLVIDO) confirma que qualquer código que fique preso num `Spinlock::lock()` com interrupções desabilitadas, esperando um lock detido por uma thread que só libera com interrupções habilitadas, trava o kernel inteiro — uma restrição a respeitar no design de `Mutex`/`CondVar` (nenhum dos dois deve, em sua implementação interna, reintroduzir esse padrão).

## Goals / Non-Goals

**Goals:**
- `Mutex`/`CondVar` nomeados, com a semântica observável de `pthread_mutex`/`pthread_cond` (e, por extensão, de `Object.wait`/`notify` em Java): bloqueio real (sem busy-wait) e nenhum despertar perdido sob disputa.
- Reaproveitar `kernel::wait_queue::WaitQueue` para a mecânica de bloqueio/despertar em si, em vez de duplicá-la.
- Lógica pura testável em host sempre que possível, seguindo o padrão já estabelecido (`kernel::wait_queue`'s `BlockedQueue`/intenção de bloqueio).
- Um demo em modo kernel que prova os dois mecanismos de ponta a ponta em boot real, agora que a impressão concorrente é segura.

**Non-Goals (nesta change):**
- Qualquer *syscall* nova expondo `Mutex`/`CondVar` a modo usuário — fica para quando existir um consumidor real de modo usuário (o porte da JVM em si, ou uma sub-change posterior do item 8).
- Suporte a *timeout* em `Mutex::lock`/`CondVar::wait` — mesmo Non-Goal já herdado de `kernel-wait-queue`; o `Parker` do HotSpot eventualmente precisa de ambos os modos, mas provar o caminho sem *timeout* primeiro é a mesma disciplina incremental já seguida em toda change anterior.
- Mutexes recursivos/reentrantes, `pthread_rwlock` (leitura/escrita), ou qualquer variante além do par mutex/condvar básico.
- Prioridade entre threads em espera — ordem FIFO simples, mesmo espírito já aceito em `kernel-wait-queue`.

## Decisions

### `Mutex::lock()` evita despertar perdido com uma nova primitiva interna de bloqueio condicional em `WaitQueue`
O problema clássico: se `Mutex::lock()` apenas checasse "está livre?" e, caso não, chamasse `WaitQueue::block_current()` incondicionalmente, existe uma janela real entre a checagem e o bloqueio de fato em que `unlock()` (rodando em outra thread, com interrupções habilitadas) poderia liberar o mutex e chamar `wake_one()` **antes** da thread que ia bloquear ter se registrado na fila de bloqueados — um despertar perdido real, já que `wake_one()` só acorda quem já está registrado naquele instante.

**Decisão**: `WaitQueue` ganha um método `pub(crate)` adicional, algo como `block_current_if(predicate: impl FnOnce() -> bool)`, que executa a checagem do predicado e o registro da intenção de bloqueio como uma única operação atômica em relação a interrupções (`with_interrupts_disabled`): se o predicado ainda for verdadeiro nesse instante, registra a intenção e bloqueia; caso contrário, retorna imediatamente sem bloquear. `Mutex::lock()` usa isso assim:
```
loop {
    se compare_exchange(livre -> tomado) tiver sucesso: retorna (adquirido)
    senão: wait_queue.block_current_if(|| ainda_tomado)
    // volta e tenta de novo
}
```
Como o predicado (`ainda_tomado`, uma leitura atômica do mesmo estado que `compare_exchange` usa) e o registro da intenção acontecem sob o mesmo `with_interrupts_disabled` que qualquer `unlock()` concorrente precisaria para ver efeito (ou, na pior hipótese, um tick real de hardware intercepta a intenção já registrada exatamente como o mecanismo de `kernel-wait-queue` já faz, comprovado em boot real naquela change) — não existe instante em que o mutex esteja livre e a thread, ainda assim, decida bloquear.

**Alternativa descartada**: duplicar a lógica de fila/intenção de bloqueio inteira dentro de `Mutex`, sem reaproveitar `WaitQueue` — rejeitada por duplicar uma mecânica já comprovada e testada, só para adicionar uma checagem condicional.

### `CondVar::wait(mutex)` depende da convenção "notificar com o mutex detido", como POSIX/Java
A ordem de operações em `wait`: (1) registra a intenção de bloqueio na `WaitQueue` da condição — **antes** de liberar o mutex — depois (2) libera o mutex, depois (3) efetiva o bloqueio (dispara o `int`). Como o chamador ainda detém o mutex entre os passos (1) e (2), e a convenção (idêntica à de `pthread_cond_wait`/`Object.notify`) exige que `notify_one`/`notify_all` também sejam chamados com o mesmo mutex detido, qualquer notificador só consegue de fato rodar (adquirir o mutex) depois que o passo (2) libera — ponto em que a intenção do passo (1) já está registrada. Isso elimina a janela de despertar perdido sem precisar de nenhuma primitiva nova além da já usada por `Mutex` (reaproveita o mesmo `block_current`/intenção de `WaitQueue`, só que dividido em "registrar" e "efetivar" ao redor da liberação do mutex, em vez de atômico como em `Mutex::lock`).

Depois de despertada (passo 3 retorna), `wait` reaquire o mutex (`mutex.lock()`, mesmo caminho de qualquer outra aquisição) antes de devolver o controle ao chamador — igual ao contrato de `pthread_cond_wait`.

**Risco aceito, documentado**: a garantia de "sem despertar perdido" depende do chamador seguir a convenção (notificar com o mutex detido) — exatamente como em POSIX/Java (não impor isso no nível de tipos custaria uma complexidade desproporcional nesta etapa; a spec já documenta a convenção explicitamente como parte do contrato, não como um detalhe escondido).

### `Mutex`/`CondVar` como tipos novos em `kernel::pthread`, não extensões de `kernel::sync`
`kernel::sync::Spinlock` continua existindo inalterado (uso interno do kernel, busy-wait deliberado). `Mutex`/`CondVar` vivem num módulo novo, `kernel::pthread`, para deixar claro que são a camada "nomeada" voltada a um futuro consumidor hóspede (a JVM), não mais um mecanismo de exclusão mútua genérico do próprio kernel — mesmo espírito de `kernel::wait_queue` ser um módulo à parte do escalonador em si.

### Demo: disputa de `Mutex` (contador) + produtor/consumidor com `Mutex`+`CondVar`
Seguindo o padrão já estabelecido, `kernel::pthread_demo` prova dois cenários em boot real, agora que a impressão concorrente é segura (bug corrigido):
- Várias threads de kernel incrementando um contador compartilhado protegido por um `Mutex`, confirmando ao final (via mensagem no console serial) que o total bate exatamente com a soma esperada (sem incremento perdido).
- Um par produtor/consumidor usando `Mutex`+`CondVar` no estilo `Object.wait`/`notify`: a produtora põe um item numa "caixa" de tamanho 1 protegida pelo mutex e notifica a condição; a consumidora espera na condição até a caixa não estar mais vazia, retira o item e reporta pelo console serial.

## Achados durante a implementação

**Risco real encontrado e corrigido antes de qualquer teste em boot real: `CondVar::wait` original registraria a intenção de bloqueio *antes* de liberar o mutex, sem as duas operações serem atômicas entre si.** Ao detalhar a implementação (antes de escrever qualquer código), percebi que isso reabriria exatamente o tipo de corrida que esta change tenta evitar, só que na direção oposta: um tick real de hardware poderia interceptar a intenção de bloqueio (já registrada) e bloquear a thread **antes** dela executar `mutex.unlock()` — deixando o mutex preso para sempre, travando qualquer outra thread que precisasse dele (inclusive qualquer thread que quisesse notificar a condição, já que a convenção exige deter o mesmo mutex para notificar). Corrigido envolvendo o registro da intenção (`WaitQueue::register_intent_for`) e a liberação do estado do mutex (`Mutex::release_state`) no mesmo `with_interrupts_disabled`, garantindo que nenhum tick real possa ocorrer entre os dois. `WaitQueue` ganhou dois métodos `pub(crate)` adicionais (`register_intent_for`/`commit_block`, este último promovido de privado) para permitir essa composição sem duplicar a lógica de intenção/bloqueio já existente.

**Verificado em boot real (QEMU), 7 execuções independentes:** em todas, o cenário do contador (4 threads × 2000 incrementos cada, disputando `COUNTER_MUTEX`) terminou com o total exato esperado (8000), confirmando exclusão mútua real sob disputa concorrente sem nenhum incremento perdido; o cenário produtor/consumidor (`Mutex`+`CondVar`) sempre entregou o item correto (42) à consumidora, que só retoma depois do `notify_one` da produtora (nunca antes, por construção do laço `while item.is_none() { wait() }`). Nenhum travamento, nenhuma `EXCECAO` — confirmando também que a correção do bug de impressão concorrente (ver `KNOW_ISSUES.md`, RESOLVIDO) suporta bem múltiplas threads adicionais imprimindo (6 threads novas desta change, além das já existentes de `kernel-wait-queue` e dos demos anteriores).

## Riscos / Trade-offs

- **[Risco] A nova primitiva `block_current_if` em `WaitQueue` adiciona uma segunda forma de iniciar um bloqueio, além de `block_current`** → Mitigado reaproveitando toda a mecânica interna existente (`set_block_intent`/`take_block_intent`/`park`), só adicionando a checagem do predicado dentro do mesmo `with_interrupts_disabled` já usado — nenhuma duplicação de lógica de fila ou de troca de contexto.
- **[Risco] A garantia de "sem despertar perdido" de `CondVar` depende de uma convenção de uso (notificar com o mutex detido), não de algo imposto pelo compilador** → Aceito deliberadamente: é o mesmo contrato de POSIX (`pthread_cond_wait`/`pthread_cond_signal`) e de Java (`Object.wait`/`notify`, que impõe isso via `IllegalMonitorStateException` em tempo de execução — algo que esta primeira versão não replica, mas poderia ser adicionado depois como uma checagem de debug).
- **[Trade-off] Sem suporte a *timeout*, mutexes recursivos ou `pthread_rwlock`** → Aceito, mesma disciplina incremental de toda change anterior deste projeto: provar o caminho básico primeiro.
- **[Trade-off] `Mutex` não-reentrante: uma thread que já detém o mutex e tenta adquiri-lo de novo bloqueia para sempre (deadlock consigo mesma)** → Aceito, mesmo comportamento de `std::sync::Mutex` do próprio Rust; documentado explicitamente na API (`# Safety`/doc comment), não silenciosamente diferente do que um usuário de Rust já esperaria.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: `cargo test -p kernel --lib` sem regressão (testes novos cobrindo a lógica pura de `Mutex`/`CondVar` e a extensão de `WaitQueue`), `cargo kernel-build`/`cargo xtask test` sem regressão de comportamento esperado no console serial, e verificação manual em boot real confirmando os dois cenários do demo (contador sem perda, produtor/consumidor via `Object.wait`/`notify`). Rollback é reverter os commits desta change; nenhum dado persistente é afetado.
