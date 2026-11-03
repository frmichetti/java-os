# Proposal

## Why

A change `jvm-posix-compat-strategy` (arquivada) inventariou as primitivas POSIX mínimas exigidas pelo HotSpot e listou `pthread_mutex`/`pthread_cond` como a base de `PlatformEvent`/`Parker` — ou seja, de `park`/`unpark`, `Object.wait`/`notify` e `java.util.concurrent` inteiro. A change `kernel-wait-queue` (arquivada) entregou o pré-requisito estrutural (`kernel::wait_queue::WaitQueue`: bloqueio/despertar voluntário de thread sem busy-wait), mas deliberadamente não expôs nenhuma API nomeada de exclusão mútua/variável de condição — isso ficou para esta sub-change, a segunda do item 8 do roadmap depois da wait-queue (a primeira sub-change, memória virtual, já foi entregue em paralelo por `kernel-posix-virtual-memory`).

Diferente de `kernel::sync::Spinlock` (exclusão mútua por *busy-wait*, adequada só para seções críticas curtas dentro do próprio kernel), `pthread_mutex`/`pthread_cond` precisam deixar a thread esperando sair de rotação (igual à wait-queue) e, no caso da variável de condição, liberar e reatomicamente reacquirir um mutex ao redor da espera — o padrão clássico de monitor usado por `Object.wait`/`notify` em Java.

## What Changes

- Novo módulo `kernel::pthread` com dois tipos nomeados, construídos sobre `kernel::wait_queue::WaitQueue` existente:
  - `Mutex`: exclusão mútua onde `lock()` bloqueia a thread (sem busy-wait) se já estiver tomado, em vez de girar como `kernel::sync::Spinlock`; `unlock()` libera e acorda uma thread em espera, se houver.
  - `CondVar`: variável de condição no estilo POSIX/Java — `wait(mutex)` libera `mutex` e bloqueia atomicamente (sem janela em que um `notify` concorrente possa ser perdido), e reaquire `mutex` antes de retornar quando despertada; `notify_one()`/`notify_all()` acordam uma ou todas as threads em espera.
- Estende `kernel::wait_queue::WaitQueue` com uma primitiva interna de bloqueio condicional (verifica uma condição e registra a intenção de bloqueio como uma única operação atômica em relação a interrupções) — necessária para `Mutex::lock()` nunca perder um `unlock()` concorrente entre a checagem e o bloqueio de fato. Não é uma capacidade nova exposta publicamente, só uma extensão da já especificada em `kernel-scheduler`.
- Um demo em modo kernel (`kernel::pthread_demo`, análogo aos demos anteriores) prova os dois mecanismos: várias threads disputando um `Mutex` para incrementar um contador compartilhado (provando exclusão mútua real, sem atualização perdida) e um par produtor/consumidor usando `Mutex`+`CondVar` (estilo `Object.wait`/`notify`) para sinalizar disponibilidade de um item.
- **Não inclui ainda** (fica para sub-changes futuras do item 8): nenhuma *syscall* nova para expor `Mutex`/`CondVar` a modo usuário (a JVM portada rodaria em modo usuário eventualmente, mas isso é adiado até existir um consumidor real de modo usuário para essas primitivas), suporte a *timeout* em `Mutex::lock`/`CondVar::wait` (mesmo Non-Goal já herdado de `kernel-wait-queue`), e mutexes recursivos/reentrantes (POSIX permite configurar isso via atributos; esta primeira versão é não-reentrante, como o `std::sync::Mutex` do próprio Rust).
- Nenhuma mudança de comportamento observável para o código já existente (`kernel::sync::Spinlock` continua existindo e sendo usado exatamente como hoje para as seções críticas internas do kernel — `Mutex`/`CondVar` são uma primitiva adicional, não uma substituição).

## Capabilities

### New Capabilities
- `kernel-thread-sync`: exclusão mútua (`Mutex`) e variável de condição (`CondVar`) com bloqueio real de thread (sem busy-wait), construídas sobre `kernel::wait_queue` — a base de `pthread_mutex`/`pthread_cond`.

### Modified Capabilities

Nenhuma — a extensão de `kernel::wait_queue` com bloqueio condicional é uma mudança de implementação interna (evitar um despertar perdido), não uma nova capacidade observável do escalonador: os requisitos já existentes em `kernel-scheduler` ("Bloqueio Voluntário de Thread sem Busy-Wait", "Despertar de Thread Bloqueada a Partir de uma Rotina de Interrupção") já cobrem o contrato de que uma thread pode bloquear e ser despertada; como ela decide *quando* bloquear internamente não é uma mudança desse contrato.

## Impact

- Novo módulo `kernel::pthread` (`Mutex`, `CondVar`) e `kernel::pthread_demo` (a demonstração em modo kernel).
- `kernel/src/wait_queue.rs`: `WaitQueue` ganha um método de bloqueio condicional interno (`pub(crate)`, não public API), usado por `Mutex::lock`; nenhuma mudança na API pública já existente (`block_current`/`wake_one`/`wake_all`).
- `kernel/src/main.rs`: `continue_boot` ganha o spawn das threads de demonstração de `kernel::pthread_demo`, no mesmo ponto onde `kernel::wait_demo` já é iniciado.
- Nenhuma mudança em `kernel::sync::Spinlock`, `kernel::syscall` ou `kernel::process`.
- `docs/architecture/overview.md`: seção 3 (Processos e Threads) e o item 8 do roadmap (seção 15) ganham uma atualização registrando esta sub-change concluída.
