# Tasks

## 1. `kernel::wait_queue`: bloqueio condicional sem despertar perdido

- [x] 1.1 Implementar `WaitQueue::block_current_if(predicate: impl FnOnce() -> bool)` (`pub(crate)`): sob `with_interrupts_disabled`, reavalia `predicate`; se ainda verdadeiro, registra a intenção de bloqueio (reaproveitando `set_block_intent`) e efetiva o bloqueio (o mesmo `int {BLOCK_VECTOR}` de `block_current`); se falso, retorna sem bloquear — ver design.md, "`Mutex::lock()` evita despertar perdido". Verificar com testes unitários em host a parte pura (predicado falso não registra nenhuma intenção; predicado verdadeiro registra a intenção corretamente antes do ponto em que o `int` real seria disparado), e que `cargo kernel-build` compila a parte que depende da instrução `int`.

## 2. `kernel::pthread::Mutex`

- [x] 2.1 Implementar o estado puro de `Mutex` (um `AtomicBool`/`AtomicU8` de "tomado/livre" com `compare_exchange`, sem nenhum acesso a `WaitQueue`/escalonador) — a tentativa de aquisição (`try_acquire`) e a liberação (`release`) como funções/métodos isolados. Verificar com testes unitários em host: aquisição bem-sucedida quando livre; aquisição falha (sem efeito colateral) quando já tomado; liberação sempre volta ao estado livre.
- [x] 2.2 Implementar `Mutex::new`/`lock`/`unlock` completos, ligando a tentativa de aquisição (tarefa 2.1) a uma `WaitQueue` interna via `block_current_if` (tarefa 1.1) no laço de `lock()`, e `wake_one()` em `unlock()` — ver design.md. Verificar que `cargo kernel-build` compila.

## 3. `kernel::pthread::CondVar`

- [x] 3.1 Implementar `CondVar::new`/`wait(&Mutex)`/`notify_one`/`notify_all`: `wait` registra a intenção de bloqueio na `WaitQueue` interna da condição *antes* de chamar `mutex.unlock()`, dispara o bloqueio de fato depois, e chama `mutex.lock()` ao retornar (antes de devolver o controle ao chamador) — ver design.md, "`CondVar::wait(mutex)` depende da convenção...". `notify_one`/`notify_all` delegam a `wake_one`/`wake_all` da `WaitQueue` interna. Verificar que `cargo kernel-build` compila.

## 4. Demonstração e verificação em boot real

- [x] 4.1 Implementar `kernel::pthread_demo` com dois cenários: (a) N threads de kernel incrementando repetidamente um contador compartilhado protegido por um `Mutex`; (b) um par produtor/consumidor usando o mesmo `Mutex` + um `CondVar` (estilo `Object.wait`/`notify`) para sinalizar um item disponível numa "caixa" de tamanho 1. Verificar que `cargo kernel-build` compila.
- [x] 4.2 Em `continue_boot` (`kernel/src/main.rs`), depois das threads de `kernel::wait_demo` já registradas: `spawn_sharing_current_address_space` para as threads de `kernel::pthread_demo`. Verificar com `cargo xtask test` que o boot continua passando sem regressão no console serial.
- [x] 4.3 Confirmar em boot real (console serial), em múltiplas execuções (corridas são não-determinísticas): (a) o contador final do cenário (a) bate exatamente com a soma esperada de todos os incrementos, mesmo com várias threads disputando o `Mutex` — nenhum incremento perdido; (b) no cenário (b), a consumidora só reporta o item depois dele estar de fato disponível (nunca antes do `notify`). Documentar o resultado em design.md, como já é costume neste projeto.
- [x] 4.4 Confirmar `cargo test -p kernel --lib` (suíte completa de host) sem regressão, incluindo os novos testes das tarefas 1.1 e 2.1.
- [x] 4.5 Atualizar `docs/architecture/overview.md`: seção 3 (Processos e Threads), registrando `kernel::pthread` (`Mutex`/`CondVar`), e o item 8 do roadmap (seção 15), registrando esta sub-change como concluída (sem marcar o item 8 inteiro — ainda faltam *syscalls* para expor isso a modo usuário, sinal de safepoint, relógio de parede, sistema de arquivos, linking dinâmico, e o próprio porte do HotSpot). Verificar lendo o arquivo atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para as modificações propostas.
