# Tasks

## 1. Argumento extra em `SYSCALL_THREAD_CREATE`

- [ ] 1.1 Estender `kernel::process::init_stack` com um parâmetro de valor
  inicial de `RDI` (índice `i = 5` do laço de zeragem — ver design.md),
  atualizando os 4 pontos de chamada (`init_kernel_stack`/
  `create_kernel_thread`/`create_user_thread`/o teste
  `init_stack_for_user_mode_uses_user_selectors_and_user_landing_rsp`),
  todos os existentes passando `0` (comportamento idêntico a antes).
  Verificar com teste de host que o valor gravado no índice correto é
  exatamente o informado, e que os outros 14 índices continuam zerados.
- [ ] 1.2 Repassar o novo argumento por `create_user_thread` →
  `Process::finish_new_user`/`Process::spawn_user_thread` →
  `kernel::scheduler::spawn_user_thread_in_process` →
  `kernel::syscall::sys_thread_create(entry, stack_size, arg)` (novo
  terceiro parâmetro, de `arg2`/`RDX`); verificar com teste de host que o
  despacho da *syscall* passa `arg2` corretamente.
- [ ] 1.3 Verificar em boot real que `sibling_thread_test_program.c`
  (`kernel-user-thread-create`, que nunca passa argumento explícito)
  continua reportando exatamente as mesmas 8 strings P/F de antes — prova
  de que a extensão não quebrou o caso sem argumento.

## 2. `pthread_create`/`pthread_exit` reais em `glue.c`

- [ ] 2.1 Implementar `pthread_create` real: valida `attr` (só
  `stacksize`/`detachstate` lidos, mesmo subconjunto de `os::create_thread`
  real), aloca `struct pthread_trampoline_args { start_routine, arg }`,
  invoca `SYSCALL_THREAD_CREATE` com `pthread_trampoline` como entry e o
  endereço desse bloco como `arg`, devolve o `ThreadId` resultante como
  `pthread_t`. Verificar com teste de host (chamando `pthread_create`
  diretamente via o binário de teste cross-compilado, não em boot real
  ainda) ou documentar que só é testável em boot real (criação de thread
  real exige o kernel).
- [ ] 2.2 Implementar `pthread_trampoline`/`pthread_exit`:
  `pthread_trampoline(void *args)` lê `(start_routine, arg)`, libera o
  bloco, chama `start_routine(arg)`, e chama `pthread_exit` com o valor
  de retorno; `pthread_exit` chama `SYSCALL_EXIT` (mesmo padrão de
  `_exit`), ignorando o valor (nenhum `pthread_join` real existe para
  recebê-lo — Non-Goal documentado em design.md).

## 3. `os::create_thread`/`os::free_thread`/`os::pd_start_thread` reais

- [ ] 3.1 Em `vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp`,
  implementar `os::create_thread(Thread*, ThreadType, size_t)`: aloca
  `OSThread`, marca `ALLOCATED`, constrói o `Monitor` de
  `osthread->startThread_lock()`, decide o tamanho de stack
  (`req_stack_size` ou `JAVAOS_DEFAULT_STACK_SIZE`), chama o
  `pthread_create` real (tarefa 2.1) com `thread_native_entry` (novo,
  mesmo papel do de `os_bsd.cpp`: `thread->record_stack_base_and_size()`,
  `thread->initialize_thread_current()`, espera em `startThread_lock` até
  `INITIALIZED`, então `thread->call_run()`), repassando `thread` como
  argumento. Verificar só por `-fsyntax-only` contra os headers reais
  (mesmo comando de `os-javaos-skeleton`/`os-javaos-platform-sync`) — não
  linkado, não executado (Non-Goal documentado em design.md).
- [ ] 3.2 Implementar `os::free_thread(OSThread*)` (`delete osthread`) e
  `os::pd_start_thread(Thread*)` (notifica `startThread_lock`, idêntico a
  `os_bsd.cpp:800-806`). Verificar só por `-fsyntax-only`.

## 4. Verificação em boot real

- [ ] 4.1 Criar `userland_c/pthread_create_test_program.c`: chama
  `pthread_create` real passando um argumento não-trivial (ex.: um
  ponteiro para uma struct com um valor conhecido), a thread criada o lê
  e escreve um resultado derivado dele numa variável compartilhada; a
  thread principal confirma o resultado esperado, provando que o
  argumento chegou intacto através do trampolim.
- [ ] 4.2 Adicionar o programa ao processo de build (`xtask`) e à lista
  de programas verificados em boot real; verificar em QEMU que reporta
  sucesso, e que todos os programas de teste já existentes (incluindo
  `sibling_thread_test_program`/`tls_test_program`/os dois novos de
  `libc-retargetable-locking`) continuam reportando exatamente as mesmas
  strings P/F de antes.

## 5. Documentação

- [ ] 5.1 Atualizar `ROADMAP.md`: registrar `pthread_create`/
  `os::create_thread` real como concluído, e identificar o próximo passo
  real seguinte (ex.: item 6.5 — sequenciamento de `runtime/init.cpp`,
  `os::create_main_thread`, ou a escolha de GC/interpretador).
- [ ] 5.2 Registrar em `design.md` desta change (seção "Resultado real da
  implementação") qualquer achado descoberto durante a implementação ou
  verificação em boot real.

## Workflow follow-up

- Confirmar que `libc-retargetable-locking` está arquivada antes de
  iniciar a tarefa 1 desta change.
- Arquivar esta change (`/opsx:archive pthread-create-thread`) depois que
  o usuário revisar a implementação.
