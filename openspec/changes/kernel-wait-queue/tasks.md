# Tasks

## 1. `kernel::scheduler`: `ReadyQueue` ganha um ponto de decisão sobre o destino da thread interrompida

- [ ] 1.1 Refatorar `ReadyQueue` (`kernel/src/scheduler.rs`) para separar "remover a thread da frente", "decidir seu destino" e "obter a próxima da fila", em vez do atual `advance()` monolítico que sempre recoloca a thread interrompida no fim — ver design.md, "`ReadyQueue` ganha um ponto de decisão explícito". Verificar com testes unitários em host: os testes já existentes de round-robin continuam passando sem alteração de comportamento quando a decisão é sempre "recolocar no fim" (nenhuma regressão no caminho padrão).
- [ ] 1.2 Adicionar a estrutura de dados pura da fila de bloqueados (lista FIFO de `ThreadId`, sem nenhum acesso a hardware) usada pela tarefa 2.1. Verificar com testes unitários em host: threads entram e saem em ordem FIFO, e uma fila vazia não tem nenhuma thread para despertar.

## 2. `kernel::wait_queue`: primitiva de bloqueio/despertar

- [ ] 2.1 Implementar `kernel::wait_queue::WaitQueue` (`new`, `wake_one`, `wake_all`) usando a estrutura da tarefa 1.2, e o mecanismo de registro de "intenção de bloqueio" (qual thread pretende ser desviada para qual `WaitQueue`, protegido por `with_interrupts_disabled` — ver design.md, "Reaproveitar `int <vetor do timer>`"). Verificar com testes unitários em host a parte pura (registro/consulta da intenção), e que `cargo kernel-build` compila a parte que depende da instrução `int` real.
- [ ] 2.2 Estender `on_timer_tick` para consultar essa intenção antes de decidir o destino de `was_running`: mover para a `WaitQueue` indicada em vez de recolocar no fim da `ReadyQueue` quando houver uma intenção de bloqueio pendente para essa thread; comportamento padrão (recolocar no fim) inalterado em qualquer outro caso. Implementar `WaitQueue::block_current()` (registra a intenção e executa a instrução `int` no vetor do timer). Verificar que `cargo kernel-build` compila e que `cargo test -p kernel --lib` não tem nenhuma regressão nos testes já existentes de `ReadyQueue`/`on_timer_tick`.
- [ ] 2.3 Confirmar, em boot real (QEMU), que `block_current()`/`wake_one()` funcionam ponta a ponta: uma thread de teste simples (temporária, só para esta verificação) bloqueia e é desbloqueada por outra, retomando exatamente de onde bloqueou (confirmado via mensagens no console serial antes/depois do bloqueio). Documentar o resultado real (e qualquer achado) em design.md, como já é costume neste projeto.

## 3. Demonstração, despertar a partir de uma ISR, e integração em `kernel_main`

- [ ] 3.1 Implementar `kernel::wait_demo` (thread produtora que chama `wake_one()` periodicamente via contagem de ticks do timer, thread consumidora que chama `block_current()` em laço e reporta cada despertar pelo console serial), análogo a `keyboard_demo`/`storage_demo`/`network_demo`. Verificar que `cargo kernel-build` compila.
- [ ] 3.2 Confirmar explicitamente o requisito de despertar a partir de uma ISR (ver spec delta, "Despertar de Thread Bloqueada a Partir de uma Rotina de Interrupção"): chamar `wake_one()`/`wake_all()` a partir de um handler de interrupção existente (ex.: do handler de teclado, de forma temporária só para esta verificação, ou diretamente do próprio handler do timer já usado pelo demo) e confirmar em boot real que não há impasse (deadlock) nem travamento, seguindo o mesmo cuidado de `with_interrupts_disabled` já usado por `kernel::keyboard::poll_event`. Documentar o resultado em design.md.
- [ ] 3.3 Em `continue_boot` (`kernel/src/main.rs`), depois da thread de demonstração do teclado já estar registrada: `spawn_sharing_current_address_space` para as duas threads de `kernel::wait_demo`. Verificar com `cargo xtask test` que o boot continua passando sem regressão no console serial, e visualmente em QEMU que a thread consumidora realmente para de aparecer em rotação enquanto bloqueada.
- [ ] 3.4 Confirmar `cargo test -p kernel --lib` (suíte completa de host) sem regressão, incluindo os novos testes das tarefas 1.1, 1.2 e 2.1.
- [ ] 3.5 Atualizar `docs/architecture/overview.md`: seção 3 (Processos e Threads), registrando a primitiva de bloqueio/despertar, e o item 8 do roadmap (seção 15), registrando esta primeira sub-change como concluída (sem marcar o item 8 inteiro como concluído — ainda faltam `pthread_mutex`/`pthread_cond`, o porte do HotSpot em si, etc.). Verificar lendo o arquivo atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para as modificações propostas.
