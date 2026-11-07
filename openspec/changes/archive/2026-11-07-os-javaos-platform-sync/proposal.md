# Proposal

## Why

Depois de `os-javaos-memory-pd` (arquivada em 2026-11-07), o `ROADMAP.md` aponta como
"Próximo passo imediato" o item 2 de médio prazo: `PlatformMutex`/`PlatformMonitor`/
`PlatformEvent`/`PlatformParker` do esqueleto HotSpot (`os/javaos/mutex_javaos.hpp`/
`park_javaos.hpp`) compilam usando os tipos `pthread_mutex_t`/`pthread_cond_t` que o
newlib já declara, mas nenhuma das funções `pthread_mutex_*`/`pthread_cond_*` em si
tem implementação real — e nenhum `.cpp` define os corpos de `PlatformMutex`/
`PlatformMonitor` (hoje só declarados em `mutex_javaos.hpp`, sem definição alguma, nem
stub `fatal()`). Sem isso, qualquer uso de sincronização do HotSpot (locks internos da
JVM, `Object.wait`/`notify`, estruturas de `java.util.concurrent`) permanece
impossível de exercitar.

## What Changes

- Implementar de verdade `pthread_mutex_init`/`destroy`/`lock`/`unlock`/`trylock` e
  `pthread_cond_init`/`destroy`/`wait`/`signal`/`broadcast` em `userland_c/glue.c`,
  sobre as syscalls de mutex/condvar que o kernel já expõe
  (`SYSCALL_MUTEX_CREATE`/`LOCK`/`UNLOCK`, `SYSCALL_COND_CREATE`/`WAIT`/
  `NOTIFY_ONE`/`NOTIFY_ALL`). `pthread_mutex_t`/`pthread_cond_t` do newlib
  vendorizado já são só inteiros de 32 bits opacos — guardam diretamente o `handle`
  devolvido pelas syscalls de criação.
- `pthread_mutex_destroy`/`pthread_cond_destroy` SHALL devolver sucesso sem liberar
  o objeto no kernel — não existe `SYSCALL_MUTEX_DESTROY`/`SYSCALL_COND_DESTROY`, e as
  tabelas de handle do kernel já não suportam remoção por decisão deliberada anterior
  (`kernel-jvm-posix-syscalls`, "sem destruição" — tabela só cresce). Mesmo
  comportamento aceitável que POSIX permite (destruir não precisa liberar memória
  imediatamente), documentado explicitamente em vez de fingir uma liberação que não
  acontece.
- Tratar a inicialização estática preguiçosa: `PTHREAD_MUTEX_INITIALIZER`/
  `PTHREAD_COND_INITIALIZER` valem `0xFFFFFFFF` no newlib vendorizado (não um handle
  real) — `pthread_mutex_lock`/`pthread_cond_wait`/etc. SHALL criar o objeto real na
  primeira vez que encontrarem esse sentinela, em vez de tratá-lo como um handle
  inválido.
- **Extensão aditiva da ABI de syscalls do kernel** (nenhuma syscall existente muda de
  número, assinatura ou comportamento — não é uma mudança disruptiva): adicionar uma
  nova syscall, `SYSCALL_MUTEX_TRYLOCK` (número 22 — o kernel tem hoje
  22 chamadas, numeradas 0–21), expondo `kernel::pthread::Mutex::try_acquire`
  (hoje privado, não-bloqueante) a modo usuário. Nenhuma syscall existente muda de
  número ou de comportamento — ver design.md para o porquê de ser necessária
  (`PlatformMutex::try_lock()` não tem hoje nenhuma primitiva não-bloqueante
  equivalente: só existem `SYSCALL_MUTEX_LOCK`, que sempre bloqueia, e
  `SYSCALL_MUTEX_UNLOCK`).
- Implementar de verdade, em um novo arquivo `.cpp` do porte `os_javaos`
  (`vendor/jdk-27-ga/src/hotspot/os/javaos/mutex_javaos.cpp` — hoje inexistente),
  `PlatformMutex::PlatformMutex`/`~PlatformMutex`/`lock`/`unlock`/`try_lock` e
  `PlatformMonitor::PlatformMonitor`/`~PlatformMonitor`/`notify`/`notify_all`, todos
  chamando as funções `pthread_mutex_*`/`pthread_cond_*` acima.
- Implementar `PlatformMonitor::wait(uint64_t millis)` **só para o caso `millis == 0`**
  (esperar indefinidamente, convenção do HotSpot para "sem timeout") — o único caso
  que `pthread_cond_wait` real consegue honrar sem uma primitiva de espera com prazo
  no kernel (que não existe hoje: `kernel::wait_queue::WaitQueue::block_current()`
  bloqueia até ser despertada, sem parâmetro de prazo). Para `millis != 0`, a função
  SHALL abortar com uma mensagem de diagnóstico explícita (mesmo padrão de stub
  honesto usado por `os-javaos-memory-pd` para `pd_*` sem primitiva equivalente) em
  vez de aplicar um timeout que não existe de verdade ou de ignorá-lo silenciosamente.
- Manter `PlatformEvent`/`PlatformParker` (`park_javaos.hpp`) **inteiramente fora de
  escopo** — `park(millis)`/`park_nanos(nanos)` dependem da mesma primitiva de espera
  com prazo ausente, e `PlatformParker` depende adicionalmente da classe
  compartilhada `Parker` (`share/runtime/park.hpp`/`park.cpp`), ainda não examinada.
  Nenhuma mudança nesses dois arquivos.
- **Verificação em boot real limitada**: um novo programa C
  (`userland_c/javaos_pthread_test_program.c`, mesmo mecanismo ELF real de
  `javaos_vm_test_program.c`) comprova `pthread_mutex_init`/`lock`/`unlock`/`trylock`/
  `destroy` de verdade (operação real dentro de uma única thread, sem contenção) e
  `pthread_cond_init`/`signal`/`broadcast`/`destroy` de verdade **sem nenhum
  esperador** (mesmo padrão seguro já usado por `kernel::user_posix_demo` para as
  mesmas syscalls). **Não prova** bloqueio real + despertar de `pthread_cond_wait`
  entre threads/processos distintos — o kernel não tem hoje nenhuma syscall de
  criação de thread em modo usuário (22 syscalls hoje, nenhuma delas), então não há
  como um processo de teste chamar `pthread_cond_wait` com qualquer garantia de que
  outra execução vai notificá-lo de volta. Essa prova fica para uma change futura,
  quando uma syscall de criação de thread existir.

## Capabilities

### New Capabilities
- `libc-pthread-sync`: comportamento real das funções POSIX de sincronização
  (`pthread_mutex_*`/`pthread_cond_*`) da libc do Java OS, sobre as syscalls de
  mutex/condvar que o kernel já expõe.
- `os-javaos-platform-sync`: comportamento real (não mais ausente/stub) dos
  primitivos de sincronização da camada `os`/`os_cpu` do porte HotSpot
  (`PlatformMutex`/`PlatformMonitor`), incluindo o caso em que nenhuma primitiva de
  kernel equivalente existe (espera com prazo).

### Modified Capabilities
- `kernel-syscalls`: adiciona um novo requisito — chamada de tentativa de aquisição
  não-bloqueante de mutex nomeado (`SYSCALL_MUTEX_TRYLOCK`). Nenhum requisito
  existente muda de comportamento.

## Impact

- **Código afetado**: `userland_c/glue.c` (novas funções `pthread_mutex_*`/
  `pthread_cond_*`); `kernel/src/syscall.rs` (nova syscall `SYSCALL_MUTEX_TRYLOCK`,
  expondo `kernel::pthread::Mutex::try_acquire`, hoje privado — torná-lo
  `pub(crate)`); `kernel/src/pthread.rs` (visibilidade de `try_acquire`); um novo
  arquivo `vendor/jdk-27-ga/src/hotspot/os/javaos/mutex_javaos.cpp`; um novo programa
  de teste em `userland_c/` e sua integração em `xtask`/`kernel/src/main.rs`
  (mesmo padrão de `os-javaos-memory-pd`).
- **Extensão de ABI de syscalls do kernel**: 22 → 23 chamadas de sistema. Puramente
  aditiva — nenhuma syscall existente muda de número, assinatura ou comportamento.
  Destacado aqui porque é uma mudança de estrutura/contrato do kernel, não uma
  mudança de banco de dados (este projeto não tem banco de dados) — ver CLAUDE.md,
  "notificar qualquer modificação de estrutura" (interpretado, na ausência de banco,
  como qualquer alteração de contrato/ABI do kernel).
- **Sem mudança em**: `kernel-thread-sync` (primitivas internas do kernel,
  inalteradas — só uma nova forma de acessá-las de modo usuário), `kernel-virtual-memory`,
  `os-javaos-memory`, qualquer programa de usuário existente.
- **Limitação que permanece sem solução nesta change**: nenhuma espera com prazo
  (`pthread_cond_timedwait`-like) em lugar nenhum da pilha — reconhecida aqui, não
  corrigida; corrigi-la exigiria uma primitiva nova no `kernel::wait_queue`
  (temporizador + despertar), fora de escopo.
