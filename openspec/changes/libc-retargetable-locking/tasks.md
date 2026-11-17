# Tasks

## 1. `SYSCALL_GET_THREAD_ID` no kernel

- [ ] 1.1 Adicionar `SYSCALL_GET_THREAD_ID = 24` em `kernel::syscall`, com
  handler trivial que devolve `kernel::scheduler::current_thread_id().0`;
  verificar com teste de host que a chamada despacha corretamente (mesmo
  padrão de `get_ticks_dispatches_to_the_real_tick_counter`).

## 2. Reconstrução do newlib com locking retargetável

- [ ] 2.1 Atualizar `scripts/cross/build-newlib.sh` para passar
  `--enable-newlib-retargetable-locking` ao `configure`; documentar que
  isso exige apagar/reconfigurar `vendor/newlib-build` (o script já
  reconfigura quando o `Makefile` não existe — adicionar uma nota clara se
  o `Makefile` de uma build anterior, sem a flag, puder ficar "preso").
  Verificar rodando o script e confirmando, via `grep`/inspeção do
  `newlib.h` gerado em `vendor/newlib-build`, que `_RETARGETABLE_LOCKING`
  está definida.

## 3. `struct __lock` e as funções `__retarget_lock_*` em `glue.c`

- [ ] 3.1 Adicionar `struct __lock { long mutex_handle; long owner_tid;
  int depth; }` e as 8 instâncias `static struct __lock __lock_<nome>`
  (`__malloc_recursive_mutex`, `__sfp_recursive_mutex`,
  `__env_recursive_mutex`, `__atexit_recursive_mutex`,
  `__dd_hash_mutex`, `__at_quick_exit_mutex`, `__tz_mutex`,
  `__arc4random_mutex`) em `userland_c/glue.c`.
- [ ] 3.2 Adicionar `__javaos_init_internal_locks(void)`, chamando
  `SYSCALL_MUTEX_CREATE` para cada uma das 8 instâncias e preenchendo seu
  `mutex_handle`; chamá-la em `userland_c/crt0.S` antes de `call main`.
- [ ] 3.3 Implementar as 10 funções `__retarget_lock_init`/
  `_init_recursive`/`_acquire`/`_acquire_recursive`/`_release`/
  `_release_recursive`/`_close`/`_close_recursive`/`_try_acquire`/
  `_try_acquire_recursive`, as variantes não-recursivas delegando direto
  para `SYSCALL_MUTEX_LOCK`/`UNLOCK`/`TRYLOCK` sobre `mutex_handle`, as
  recursivas usando `SYSCALL_GET_THREAD_ID` para verificar
  `lock->owner_tid` antes de bloquear (ver design.md). `_init`/
  `_init_recursive` chamam `SYSCALL_MUTEX_CREATE` (para um `_LOCK_T`
  alocado dinamicamente por código de aplicação, fora dos 8 já
  inicializados pela tarefa 3.2); `_close`/`_close_recursive` ficam
  deliberadamente sem liberação real (Non-Goal, mesma disciplina de
  `pthread_handles`).

## 4. Verificação em boot real

- [ ] 4.1 Criar `userland_c/malloc_concurrency_test_program.c`: duas
  threads (principal + uma criada via `SYSCALL_THREAD_CREATE`) chamando
  `malloc`/`free` repetidamente e concorrentemente (laço apertado, sem
  sincronização externa entre elas — o próprio `libc-retargetable-locking`
  é quem deve evitar corrupção), cada alocação escrevendo e relendo um
  padrão conhecido para detectar sobreposição; reportar P/F.
- [ ] 4.2 Criar `userland_c/recursive_lock_test_program.c`: chama
  `__retarget_lock_acquire_recursive`/`__retarget_lock_release_recursive`
  diretamente (não há API pública de mais alto nível para isso neste
  projeto) duas vezes seguidas na mesma thread sobre o mesmo lock,
  confirmando que a segunda aquisição não bloqueia, e que outra thread só
  consegue adquiri-lo depois das duas liberações correspondentes.
- [ ] 4.3 Adicionar os dois programas ao processo de build (`xtask`) e à
  lista de programas verificados em boot real; verificar em QEMU que
  ambos reportam sucesso, e que todos os programas de teste já existentes
  continuam reportando exatamente as mesmas strings P/F de antes (nenhuma
  regressão da reconstrução do newlib).

## 5. Documentação

- [ ] 5.1 Atualizar `ROADMAP.md`: registrar `libc-retargetable-locking`
  como concluída, e apontar `pthread_create`/`os::create_thread` real
  como o novo próximo passo imediato.
- [ ] 5.2 Registrar em `design.md` desta change (seção "Resultado real da
  implementação") qualquer achado descoberto durante a implementação ou
  verificação em boot real.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive libc-retargetable-locking`) depois
  que o usuário revisar a implementação.
