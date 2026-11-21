# Proposal

## Why

O newlib vendorizado já chama uma API de *locking* interna
(`__lock_acquire_recursive`/etc., via `MALLOC_LOCK`/`MALLOC_UNLOCK` em
`newlib/libc/stdlib/_mallocr.c` e pontos análogos em `stdio`/`atexit`/
`env`/`tz`) sempre que precisa proteger seus próprios recursos internos
contra acesso concorrente — mas, sem a flag de build `_RETARGETABLE_LOCKING`
(nunca ativada por `scripts/cross/build-newlib.sh`), toda essa API vira
no-op em tempo de pré-processador (`newlib/libc/include/sys/lock.h`). Até
`kernel-user-thread-create`, isso era inofensivo: nunca existiu mais de uma
thread por processo, então nenhum desses recursos era de fato acessado
concorrentemente. Agora que `SYSCALL_THREAD_CREATE` existe, o HotSpot (que
cria várias threads nativas fazendo `malloc`/`free` concorrentemente) exige
que esse locking seja real — sem isso, o heap do processo pode corromper
silenciosamente.

## What Changes

- `scripts/cross/build-newlib.sh` passa a configurar o newlib com
  `_RETARGETABLE_LOCKING` ativada, exigindo uma reconstrução do newlib
  vendorizado.
- Novo `struct __lock` (opaco para o newlib) e as funções
  `__retarget_lock_init`/`_recursive`, `__retarget_lock_acquire`/
  `_recursive`, `__retarget_lock_release`/`_recursive`,
  `__retarget_lock_close`/`_recursive`, `__retarget_lock_try_acquire`/
  `_recursive` em `userland_c/glue.c`, implementadas sobre o mutex real já
  existente (`SYSCALL_MUTEX_CREATE`/`LOCK`/`UNLOCK`/`TRYLOCK`).
- As 8 instâncias `struct __lock` que o próprio newlib referencia
  internamente como `extern`, definidas em `glue.c`:
  `__malloc_recursive_mutex`, `__sfp_recursive_mutex` (stdio),
  `__env_recursive_mutex`, `__atexit_recursive_mutex` (recursivas); e
  `__dd_hash_mutex`, `__at_quick_exit_mutex`, `__tz_mutex`,
  `__arc4random_mutex` (não recursivas).
- Nova chamada de sistema `SYSCALL_GET_THREAD_ID` (devolve
  `kernel::scheduler::current_thread_id()`): as variantes recursivas
  precisam saber se a thread chamadora já é dona do lock (para não se
  auto-travar), e hoje nenhuma thread tem como perguntar sua própria
  `ThreadId` ao kernel.
- Novos programas de teste em `userland_c/`: um exercitando `malloc`/`free`
  concorrente real (duas threads via `SYSCALL_THREAD_CREATE`) e outro
  provando reentrância recursiva (a mesma thread adquire o mesmo lock duas
  vezes sem travar).

**Fora do escopo desta change** (fica para a change seguinte, que
reaproveita este *locking* real): `pthread_create`/`os::create_thread`/
`os_javaos::pd_start_thread` reais, e qualquer arquivo vendorizado do
HotSpot.

## Capabilities

### New Capabilities
- `libc-retargetable-locking`: garante que os recursos internos do newlib
  usados por múltiplas threads do mesmo processo (heap de `malloc`,
  buffers de `stdio`, tabela de `atexit`, cache de ambiente/timezone) são
  protegidos por exclusão mútua real, via a API de *locking* retargetável
  do próprio newlib.

### Modified Capabilities
- `kernel-syscalls`: adiciona o requisito da nova chamada de sistema de
  consulta da `ThreadId` corrente.

## Impact

- `scripts/cross/build-newlib.sh`: nova flag de configure, exigindo
  reconstrução do newlib (o usuário precisa rodar o script novamente).
- `userland_c/glue.c`: novo `struct __lock`, as funções `__retarget_lock_*`,
  e as 8 instâncias `extern` que o newlib referencia.
- `kernel/src/syscall.rs`: novo número de chamada
  `SYSCALL_GET_THREAD_ID = 24` e seu handler.
- `userland_c/`: dois novos programas de teste (malloc concorrente,
  reentrância recursiva).
- Nenhuma mudança de comportamento observável para um programa de uma
  única thread (todo programa de teste já existente hoje).
