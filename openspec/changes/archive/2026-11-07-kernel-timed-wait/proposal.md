# Proposal

## Why

Desde `kernel-wait-queue`, toda espera bloqueante do kernel (`WaitQueue::block_current`,
`Mutex::lock`, `CondVar::wait`) é indefinida — sem nenhum parâmetro de prazo. Isso já
bloqueia dois pontos deixados deliberadamente como stub explícito pela change
`os-javaos-platform-sync`: `PlatformMonitor::wait(millis != 0)` (`mutex_javaos.cpp`)
aborta o processo com `fatal()` em vez de esperar de verdade, e `PlatformEvent::park`/
`park_nanos`/`PlatformParker` (`park_javaos.hpp`) nem têm corpo algum ainda. O
`ROADMAP.md` aponta exatamente essa lacuna como o próximo passo imediato: sem espera
com prazo, nenhum código do HotSpot que dependa de timeout (locks internos da JVM,
`Object.wait(millis)`, `java.util.concurrent`, polling de safepoint) pode funcionar
de verdade.

## What Changes

- Estender `kernel::wait_queue::WaitQueue` com uma variante de bloqueio que aceita um
  prazo (em nanossegundos monotônicos, via `kernel::clock::monotonic_nanos()` já
  existente): a thread chamadora é despertada por notificação **ou** pela expiração
  do prazo, o que vier primeiro — nunca as duas coisas, nunca nenhuma.
- `kernel::scheduler::on_timer_tick` (já roda a 100Hz) passa também a checar, a cada
  tick, se algum prazo pendente expirou, despertando a thread correspondente nesse
  caso.
- `kernel::process::Context` ganha um novo campo (`timed_out: bool`) para a thread que
  acabou de ser despertada saber se foi por notificação ou por expiração do prazo —
  necessário porque `WaitQueue::commit_block` (a instrução `int` que efetiva o
  bloqueio) não tem nenhum canal de retorno de valor.
- `kernel::pthread::CondVar` ganha uma variante de `wait` que aceita esse mesmo prazo,
  mantendo a mesma garantia de liberar/reaquirir o mutex associado atomicamente que a
  variante sem prazo já tem.
- **Sem nova syscall**: `SYSCALL_COND_WAIT` (número 11) já tem um terceiro argumento
  (`arg2`) que hoje é completamente ignorado — todo chamador existente
  (`kernel::user_posix_demo`, `userland_c/glue.c::pthread_cond_wait`) já passa `0`
  nesse slot. Reaproveitado como `timeout_nanos`, com `0` preservando exatamente o
  comportamento atual (espera indefinida) — nenhuma mudança de ABI incompatível, nenhum
  chamador existente precisa mudar.
- `userland_c/glue.c::pthread_cond_timedwait` real (POSIX: recebe um prazo
  **absoluto**, convertido para um prazo **relativo** — `timeout_nanos` — antes de
  chamar a syscall, usando o relógio de parede já exposto por `SYSCALL_CLOCK_WALL`).
- Completar `PlatformMonitor::wait(millis)` em `mutex_javaos.cpp` para o caso
  `millis != 0`, chamando `pthread_cond_timedwait` de verdade em vez de `fatal()`.
- **Deliberadamente fora de escopo**: `PlatformEvent::park`/`park_nanos` e
  `PlatformParker` (`park_javaos.hpp`) — dependem de um protocolo mais complexo e
  específico do HotSpot (`REL_INDEX`/`ABS_INDEX`, dois `pthread_cond_t`, um contador
  `_counter` para evitar corrida entre `park`/`unpark`), um consumidor
  significativamente mais envolvido do que `PlatformMonitor::wait`. Fica para uma
  change futura, depois que esta entregar e provar a primitiva de base.

## Capabilities

### New Capabilities
(nenhuma — esta change estende capacidades já existentes em camadas já estabelecidas,
sem introduzir nenhum conceito novo de topo.)

### Modified Capabilities
- `kernel-scheduler`: adiciona um novo requisito — bloqueio de thread com prazo em
  `WaitQueue` (nenhum requisito existente muda de comportamento).
- `kernel-thread-sync`: adiciona um novo requisito — espera limitada por prazo em
  `CondVar` (nenhum requisito existente muda).
- `kernel-syscalls`: modifica o requisito existente "Chamada de Espera em Variável de
  Condição Nomeada" para incluir o comportamento do novo argumento de prazo
  (cenários existentes preservados integralmente, cenários novos adicionados).
- `libc-pthread-sync`: adiciona um novo requisito — `pthread_cond_timedwait` real.
- `os-javaos-platform-sync`: remove o requisito "Espera Com Prazo Permanece
  Explicitamente Não Suportada" (substituído por comportamento real) e adiciona um
  novo requisito — espera com prazo real via `PlatformMonitor::wait(millis != 0)`.

## Impact

- **Código afetado**: `kernel/src/wait_queue.rs`, `kernel/src/scheduler.rs`,
  `kernel/src/process.rs` (novo campo em `Context`), `kernel/src/pthread.rs`
  (`CondVar`), `kernel/src/syscall.rs` (`sys_cond_wait` ganha um terceiro parâmetro),
  `userland_c/glue.c` (`pthread_cond_timedwait`),
  `vendor/jdk-27-ga/src/hotspot/os/javaos/mutex_javaos.cpp`
  (`PlatformMonitor::wait`), um novo programa de teste em `userland_c/` e sua
  integração em `xtask`/`kernel/src/main.rs` (mesmo padrão das changes anteriores).
- **Sem extensão de ABI de syscall**: `SYSCALL_COND_WAIT` já aceitava um terceiro
  argumento (ignorado); esta change só passa a interpretá-lo. Nenhuma nova syscall,
  nenhuma mudança incompatível com chamadores existentes.
- **Achado real de design**: uma entrada de "prazo pendente" (`ThreadId` → deadline +
  fila) pode ficar presa (*stale*) se a thread correspondente for notificada
  normalmente antes do prazo expirar e, em seguida, iniciar uma nova espera (com ou
  sem prazo) na mesma fila antes do prazo antigo disparar — sem cuidado, o prazo
  antigo acordaria incorretamente a nova espera. Mitigado descartando qualquer prazo
  pendente de uma thread sempre que ela registra qualquer nova intenção de bloqueio
  (ver design.md).
- **Verificação em boot real mais completa que de costume**: ao contrário da espera
  sem prazo (nunca provada em boot real por falta de syscall de criação de thread —
  `os-javaos-platform-sync`), o caminho de **expiração do prazo** desta primitiva
  pode ser provado de ponta a ponta sem nenhuma segunda thread — basta não notificar
  e confirmar que o tempo decorrido bate com o prazo pedido. O caminho "notificado
  antes do prazo expirar" continua fora de alcance pela mesma limitação de sempre.
- **Sem mudança em**: `kernel-posix-virtual-memory`, `os-javaos-memory`, `libc-float-math`,
  qualquer syscall de memória/mutex já existente, qualquer programa de usuário já
  existente além do ponto de extensão documentado acima.
