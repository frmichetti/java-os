# Proposal

## Why

`os::create_main_thread`/`os::create_attached_thread` não existem em nenhum
arquivo do esqueleto `os_javaos`, apesar de `share/runtime/os.hpp` exigi-los —
`Threads::create_vm()` (`share/runtime/threads.cpp`) chama
`Thread::set_as_starting_thread()` → `os::create_main_thread()` logo no início
da inicialização da VM, antes de qualquer GC/interpretador/compilador, para
anexar a thread primordial (que já existe — o próprio processo criado pelo
carregador ELF) a um `JavaThread`. Sem isso, nenhuma JVM sobre este porte
consegue sequer começar `Threads::create_vm()`, mesmo com `os::create_thread`
(`pthread-create-thread`) já real.

## What Changes

- `os::create_main_thread`/`os::create_attached_thread` reais em
  `os_javaos.cpp`, replicando a forma real de `os_bsd.cpp` (aloca `OSThread`,
  marca `RUNNABLE`, associa ao `Thread`) — mesma disciplina de
  `os::create_thread` (`pthread-create-thread`): verificado só por
  `-fsyntax-only`, nunca linkado.
- Nova função `pthread_self()` real em `userland_c/glue.c` — achado da
  exploração: `create_attached_thread` precisa que a própria thread (já em
  execução, nunca criada via `pthread_create`) descubra seu próprio
  identificador nativo, e nenhuma função deste porte expõe isso hoje
  (`javaos_get_thread_id`, que já envolve a mesma `SYSCALL_GET_THREAD_ID`,
  é `static`, uso interno de `libc-retargetable-locking`). Delega
  diretamente a `SYSCALL_GET_THREAD_ID`, já existente.
- Novo programa de teste em `userland_c/`: cria uma thread via
  `pthread_create` e confirma, de dentro da própria thread criada, que
  `pthread_self()` bate exatamente com o `pthread_t` que a thread criadora
  recebeu de volta.

**Fora do escopo desta change**: o outro uso real de `os::create_attached_thread`
no HotSpot (JNI `AttachCurrentThread`, `share/prims/jni.cpp`) — `prims`/JNI é
item 6 do roadmap, ainda não explorado. Esta change implementa
`create_attached_thread` o suficiente para servir `create_main_thread` (a
thread primordial), não uma superfície JNI completa. Também fora de escopo:
qualquer coisa além dos dois hooks `os::` e de `pthread_self()` — GC,
interpretador, carregamento de classes, e a sequência real de
`share/runtime/init.cpp` (`vm_init_globals`/`init_globals`/`init_globals2`)
continuam completamente fora desta change (dependência muito mais distante,
roadmap itens 2-4/6).

## Capabilities

### New Capabilities
- `os-javaos-attach-thread`: define o comportamento real de
  `os::create_main_thread`/`os::create_attached_thread` no porte `os_javaos`
  — anexar uma thread do SO já em execução (nunca criada via
  `os::create_thread`) a um `Thread`/`JavaThread`.

### Modified Capabilities
- `libc-pthread-create`: adiciona o requisito de uma thread poder consultar
  seu próprio identificador nativo via `pthread_self()`.

## Impact

- `vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp`:
  `os::create_main_thread`/`os::create_attached_thread` reais (vendorizado,
  não versionado no Git — mesma convenção já usada por `os-javaos-skeleton`/
  `os-javaos-platform-sync`/`os-javaos-memory-pd`/`pthread-create-thread`).
- `userland_c/glue.c`: nova função `pthread_self()`.
- `userland_c/`: novo programa de teste de `pthread_self()`.
- Nenhuma mudança no kernel (`SYSCALL_GET_THREAD_ID` já existe,
  `libc-retargetable-locking`) nem em nenhum programa de teste já existente.
