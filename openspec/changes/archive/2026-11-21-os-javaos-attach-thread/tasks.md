# Tasks

## 1. `pthread_self()` real em `glue.c`

- [x] 1.1 Implementar `pthread_t pthread_self(void)` em `userland_c/glue.c`,
  delegando diretamente a `SYSCALL_GET_THREAD_ID` via `javaos_syscall`
  (mesmo padrão de `SYS_GET_THREAD_ID`/`javaos_get_thread_id` já
  existentes — ver design.md, "`pthread_self()` chama a syscall
  diretamente"). Verificar compilando `glue.c` isoladamente com o wrapper
  de cross-compilador (mesmo comando usado para validar mudanças
  anteriores em `glue.c`).

## 2. `os::create_main_thread`/`os::create_attached_thread` reais

- [x] 2.1 Em `vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp`,
  adicionar `#include "runtime/javaThread.hpp"` e implementar
  `os::create_attached_thread(JavaThread*)` (aloca `OSThread`, grava
  `pthread_self()` como `thread_id`, marca `RUNNABLE`, associa via
  `thread->set_osthread`) e `os::create_main_thread(JavaThread*)`
  (delega inteiramente a `create_attached_thread`) — replicando
  `os_bsd.cpp:754-798` (ver design.md, "Decisions").
- [x] 2.2 Verificar por compilação real (`g++ -fsyntax-only -std=gnu++17`,
  mesma invocação já usada por `os-javaos-skeleton`/`os-javaos-memory-pd`/
  `os-javaos-park-sync`/`pthread-create-thread`) contra a árvore
  vendorizada: `os_javaos.cpp` compila sem erro novo. Como regressão
  (nenhum arquivo além de `os_javaos.cpp` deveria precisar mudar, mas
  `osThread_javaos.hpp` não é tocado por esta change — diferente de
  `pthread-create-thread`), reverificar também `mutex_javaos.cpp` e
  `park_javaos.cpp` continuam compilando.

## 3. Verificação em boot real

- [x] 3.1 Criar `userland_c/pthread_self_test_program.c`: a thread
  principal chama `pthread_create`, guarda o `pthread_t` devolvido; a
  thread criada chama `pthread_self()` e grava o resultado numa variável
  compartilhada; a thread principal confirma que os dois valores são
  idênticos. Cobre as duas cenas de
  `specs/libc-pthread-create/spec.md` (delta desta change): o próprio
  identificador bate com o devolvido à criadora, e (criando uma segunda
  thread) identificadores distintos nunca se confundem.
- [x] 3.2 Adicionar o programa ao processo de build (`xtask`) e à lista de
  programas verificados em boot real; verificar em QEMU que reporta
  sucesso, e que todos os programas de teste já existentes continuam
  reportando exatamente as mesmas strings P/F de antes (nenhuma
  regressão).

## 4. Documentação

- [x] 4.1 Atualizar `ROADMAP.md`: registrar
  `os::create_main_thread`/`os::create_attached_thread`/`pthread_self`
  como concluídos, e identificar o próximo passo real seguinte (mapear
  `vm_init_globals`/`init_globals`/`init_globals2` de
  `share/runtime/init.cpp` contra a escolha de GC/interpretador — ainda
  sem exploração).
- [x] 4.2 Registrar em `design.md` desta change (seção "Resultado real da
  implementação") qualquer achado descoberto durante a implementação ou
  verificação em boot real.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive os-javaos-attach-thread`) depois
  que o usuário revisar a implementação.
