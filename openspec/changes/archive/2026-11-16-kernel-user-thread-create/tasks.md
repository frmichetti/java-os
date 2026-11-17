# Tasks

## 1. Janela de stack de usuário para threads adicionais

- [x] 1.1 Adicionar `USER_THREAD_STACK_POOL_BASE` e um contador
  `AtomicU64` global de próximo slot em `kernel::stack` (ou novo módulo),
  seguindo o mesmo padrão de `THREAD_STACK_POOL_BASE`/
  `NEXT_THREAD_STACK_SLOT`, mas mapeando via `Mapper::for_root` (metade
  inferior, privada por processo) em vez de `Mapper::new`; verificar com
  teste de host (`cargo test -p kernel --lib`) que dois slots consecutivos
  nunca se sobrepõem e nunca colidem com nenhuma constante de stack de
  programa de teste já existente.

## 2. Busca de `Process` por `ProcessId` e gate de "última thread"

- [x] 2.1 Adicionar função de busca de `Process` em `PROCESSES` por
  `ProcessId` em `scheduler.rs`; verificar com teste de host cobrindo
  processo existente e `ProcessId` desconhecido.
- [x] 2.2 Em `schedule_next` (`scheduler.rs:795-835`), antes das três
  chamadas de limpeza existentes, remover o `ThreadId` de `was_running` de
  `process.threads` (via a busca de 2.1) e só executar
  `free_user_mappings`/`close_all_owned_by`/`release_all_owned_by` quando
  `process.threads` ficar vazio; verificar com teste de host que um
  processo de uma única thread continua liberando tudo ao terminar
  (comportamento idêntico ao de antes desta change).
- [x] 2.3 Verificar em boot real (`cargo xtask test` / QEMU) que todos os
  programas de teste existentes (`file_syscalls_demo`,
  `javaos_vm_test_program`, `user_posix_demo`, `libc_test_program`,
  `javaos_pthread_test_program`, `timedwait_test_program`,
  `efault_test_program`, `cxx_test_program`, `libm_test_program`) ainda
  reportam exatamente as mesmas strings P/F de antes, confirmando que o
  gate não alterou nenhum comportamento de processo de uma única thread.

## 3. Syscall de criação de thread

- [x] 3.1 Adicionar `SYSCALL_THREAD_CREATE = 23` em `syscall.rs` e seu
  handler: localizar o processo do chamador (via `CURRENT_PROCESS_ID` + a
  busca de 2.1), validar o ponto de entrada com
  `Mapper::validate_user_range` (rejeitar com `EFAULT`), validar o tamanho
  de stack (`arg1 != 0` → senão `EINVAL`), alocar um slot da janela de 1.1,
  chamar `Process::spawn_user_thread`, registrar a nova `Thread` em
  `CONTEXTS`/`READY_QUEUE`, devolver o `ThreadId` como `u64`; verificar com
  teste de host que a chamada devolve um identificador diferente do da
  thread chamadora e que os dois casos de erro devolvem os códigos POSIX
  esperados.
- [x] 3.2 Verificar (em boot real — não testável em host, mesma limitação
  já documentada para `validate_user_range`/`translate`: a caminhada real
  de tabelas de página só é verificável com hardware) que a nova thread
  criada, uma vez escalonada, começa a executar exatamente a partir do
  ponto de entrada fornecido, usando a stack de usuário alocada. Coberto
  junto com a tarefa 4.2 pelo programa de teste de thread-irmã.

## 4. Programa de teste de thread-irmã

- [x] 4.1 Criar `userland_c/sibling_thread_test_program.c`: cria um mutex
  nomeado e um arquivo aberto, invoca `SYSCALL_THREAD_CREATE`, a
  thread-irmã opera sobre o mesmo mutex/arquivo e incrementa um contador
  compartilhado antes de encerrar via `SYSCALL_EXIT`; a thread principal
  espera esse contador mudar (poll simples, sem `pthread_join` — Non-Goal
  documentado em `design.md`), então verifica que o mutex/arquivo
  continuam utilizáveis, e por fim também encerra.
- [x] 4.2 Adicionar `sibling_thread_test_program` ao processo de build
  (`xtask`) e à lista de programas verificados em boot real; verificar em
  QEMU que o programa reporta sucesso (contador incrementado por ambas as
  threads, mutex/arquivo utilizáveis após o término da thread-irmã, e
  nenhum travamento/pane ao encerrar a thread principal por último).

## 5. Documentação

- [x] 5.1 Atualizar `ROADMAP.md`: mover o item 6.1 ("Criação de thread
  nativa em modo usuário") para "implementado", registrando que o
  primitivo de kernel está pronto e que `os_javaos::pd_start_thread`/
  `pthread_create` real fica para uma change seguinte (ver `design.md` —
  Non-Goals).
- [x] 5.2 Registrar em cada `design.md` desta change (seção "Resultado
  real da implementação") qualquer achado descoberto durante a
  implementação ou verificação em boot real, seguindo o padrão já usado
  pelas changes anteriores desta sessão.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive kernel-user-thread-create`) depois
  que o usuário revisar a implementação.
