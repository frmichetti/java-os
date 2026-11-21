# Proposal

## Why

`SYSCALL_THREAD_CREATE` (`kernel-user-thread-create`) e a TLS por thread
(`kernel-thread-local-storage`) já existem, mas nenhum programa C deste
projeto consegue de fato criar uma thread pela API POSIX padrão
(`pthread_create`) — o único caminho hoje é a *syscall* bruta, sem
nenhuma forma de passar um argumento à função da nova thread. Sem isso, o
HotSpot (que chama `os::create_thread`, nunca a *syscall* diretamente)
continua sem conseguir criar nenhuma de suas threads nativas
(`VMThread`/`ServiceThread`/etc.), o bloqueio duro já identificado no
roadmap. `os::create_thread`/`os::free_thread` também nunca foram
declaradas em `os_javaos.cpp`, apesar de `share/runtime/os.hpp` exigi-las.

## What Changes

- `SYSCALL_THREAD_CREATE` passa a aceitar um terceiro argumento (`arg`),
  entregue à nova thread no registrador `RDI` quando ela começa a
  executar — hoje a chamada zera todos os registradores de uso geral da
  nova thread, inclusive `RDI`.
- `pthread_create`/`pthread_exit` reais em `userland_c/glue.c`, sobre a
  `SYSCALL_THREAD_CREATE` estendida: `pthread_create` aloca um pequeno
  bloco com `(start_routine, arg)`, passa seu endereço como `arg` da
  *syscall*, e um trampolim comum (`entry`) o lê e chama
  `start_routine(arg)`, encerrando via `pthread_exit` (que apenas chama
  `SYSCALL_EXIT`) quando `start_routine` retorna.
- `os::create_thread`/`os::free_thread`/`os_javaos::pd_start_thread`
  reais em `os_javaos.cpp`, reaproveitando `OSThread::startThread_lock()`/
  `Monitor` e `ThreadState`/`get_state`/`set_state` (já existentes do
  esqueleto, `os-javaos-skeleton`) e chamando o `pthread_create` real
  acima — mesmo padrão de `os-javaos-platform-sync` (que conectou
  `PlatformMutex`/`PlatformMonitor` sobre `pthread_mutex_*`/
  `pthread_cond_*` reais).
- Novo programa de teste em `userland_c/` provando `pthread_create` real
  com argumento (a thread-irmã recebe e usa o argumento passado, não
  apenas variáveis globais como os testes anteriores de thread).

**Fora do escopo desta change**:
- `os::create_main_thread`/`os::create_attached_thread` (anexar a thread
  primordial a um `JavaThread` já em execução) — problema diferente, de
  sequenciamento de inicialização da VM (ver `ROADMAP.md`, item 6.5), não
  de criação de uma nova thread.
- `pthread_join`/`pthread_detach` — `os::create_thread` sempre cria
  threads destacadas (`PTHREAD_CREATE_DETACHED`); nenhum consumidor real
  precisa esperar o término de uma thread criada por ele.
- Qualquer arquivo vendorizado do HotSpot além de `os_javaos.cpp` — a
  verificação do lado `os_javaos.cpp` continua só por `-fsyntax-only`
  contra os headers reais (nunca linkado nem executado, mesma disciplina
  de toda mudança em `os_javaos.cpp` até hoje, `os-javaos-skeleton`).

**Dependência**: esta change pressupõe `libc-retargetable-locking`
implementada e arquivada primeiro — `pthread_create`/`malloc` dentro do
trampolim da nova thread só são seguros sob concorrência real depois que
aquela change existir.

## Capabilities

### New Capabilities
- `libc-pthread-create`: permite a um programa C de modo usuário criar e
  encerrar uma thread POSIX real, com um argumento passado à função da
  nova thread, através de `pthread_create`/`pthread_exit`.

### Modified Capabilities
- `kernel-syscalls`: a chamada de criação de thread passa a aceitar um
  argumento adicional, repassado à nova thread.

## Impact

- `kernel/src/process.rs`: `init_stack` ganha um parâmetro para o valor
  inicial de `RDI`; `create_user_thread` repassa o novo argumento da
  *syscall*.
- `kernel/src/syscall.rs`: `sys_thread_create` ganha um terceiro
  argumento.
- `userland_c/glue.c`: `pthread_create`/`pthread_exit` reais.
- `vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp`: `os::create_thread`/
  `os::free_thread`/`os::pd_start_thread` reais (vendorizado, não
  versionado no Git — mesma convenção já usada por `os-javaos-skeleton`/
  `os-javaos-platform-sync`/`os-javaos-memory-pd`).
- `userland_c/`: novo programa de teste de `pthread_create` com argumento.
- Nenhuma mudança de comportamento observável para um chamador que ignora
  o novo argumento (equivalente a passar `0`, mesmo efeito de hoje).
