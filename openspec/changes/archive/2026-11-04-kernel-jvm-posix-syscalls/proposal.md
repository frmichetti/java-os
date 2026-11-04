# Proposal

## Why

O roadmap do item 8 ("Porte inicial da JVM", ver `docs/architecture/overview.md` seção 15) já entregou, em modo kernel, as quatro primitivas POSIX mínimas identificadas em `jvm-posix-compat-strategy`: memória virtual anônima (`kernel::vmm` — base de `mmap`/`munmap`/`mprotect`), mutex/condvar nomeados (`kernel::pthread` — base de `pthread_mutex`/`pthread_cond`) e relógios (`kernel::clock` — base de `clock_gettime`). Nenhuma delas é alcançável por código em modo usuário hoje — só existe `SYSCALL_WRITE` e `SYSCALL_GET_TICKS` (`kernel-syscalls`). Sem essa ponte, o próximo passo do roadmap (sistema de arquivos, carregamento dinâmico, porte do HotSpot) não tem como ser exercitado por um programa de usuário real, já que o HotSpot roda inteiramente em modo usuário sobre essas primitivas. Esta é a sub-change explicitamente prevista no roadmap ("ainda faltam: *syscalls* expondo essas primitivas a modo usuário").

## What Changes

- Estende a ABI de `kernel-syscalls` (número em `RAX`, até 4 argumentos, retorno em `RAX`, `SYSCALL_ERROR` para falha) com novas chamadas:
  - **Memória virtual anônima** (sobre `kernel::vmm::VMM`, a janela global já existente): reservar, comprometer, descomprometer, proteger e liberar uma faixa, endereçadas por endereço virtual (não por handle), espelhando a convenção de `mmap`/`munmap`/`mprotect` POSIX.
  - **Mutex nomeado** (sobre `kernel::pthread::Mutex`): criar, adquirir (bloqueante) e liberar, identificados por um handle `u64` devolvido na criação — necessário porque `Mutex`/`CondVar` exigem referência `'static` e são criados dinamicamente a pedido do chamador (ao contrário dos dois mutexes estáticos usados pelos demos de modo kernel existentes).
  - **Variável de condição nomeada** (sobre `kernel::pthread::CondVar`): criar, esperar (associada a um mutex já adquirido), notificar um ou todos os esperantes — mesmo esquema de handle do mutex.
  - **Relógios** (sobre `kernel::clock`): consultar o relógio de parede e o monotônico, cada um em nanossegundos.
- Toda falha (limites inválidos, handle desconhecido, região não comprometida, etc.) colapsa para o mesmo sentinela `SYSCALL_ERROR` já usado por "número de chamada não reconhecido" — nenhum código de erro granular (estilo `errno`) é introduzido nesta change.
- Um novo programa de teste em modo usuário (mesmo padrão de `kernel::user_demo`, assembly independente de posição) exercita as novas chamadas em boot real, incluindo duas threads de usuário disputando um mutex/condvar nomeado.

## Capabilities

### New Capabilities
(nenhuma)

### Modified Capabilities
- `kernel-syscalls`: adiciona os requisitos para as novas chamadas de memória virtual anônima, mutex nomeado, variável de condição nomeada e consulta de relógios, todas em modo usuário.

## Impact

- **Código afetado**: `kernel/src/syscall.rs` (novos números de chamada, novos `sys_*` e extensão de `dispatch`); novas tabelas globais de handles para `Mutex`/`CondVar` nomeados (provavelmente um novo módulo, ex. `kernel::pthread_handles`, ou extensão de `kernel::pthread`); nenhuma mudança em `kernel::vmm`/`kernel::pthread`/`kernel::clock` em si (suas APIs internas já cobrem o necessário).
- **Específicações**: delta em `kernel-syscalls` (ver acima). `kernel-virtual-memory`, `kernel-thread-sync` e `kernel-clock` não mudam — seus requisitos já descrevem o comportamento das primitivas em si, independente de como são alcançadas.
- **Dependências**: nenhuma nova dependência externa; reaproveita `kernel::vmm::VMM`, `kernel::pthread::{Mutex, CondVar}`, `kernel::clock::{wall_clock_nanos, monotonic_nanos}` e `kernel::heap` (para alocar os objetos nomeados dinamicamente, via `Box::leak`).
- **Compatibilidade**: puramente aditivo — `SYSCALL_WRITE`/`SYSCALL_GET_TICKS` e o despacho por número continuam exatamente como estão; nenhum comportamento existente muda.
