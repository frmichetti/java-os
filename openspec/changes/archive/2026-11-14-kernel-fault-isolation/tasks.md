# Tasks

## 1. Encerramento reaproveitável em `kernel::scheduler`

- [x] 1.1 Adicionar `pub(crate) fn terminate_current_thread() -> !` a
      `kernel/src/scheduler.rs`: `mark_terminated(current_thread_id())` seguido de
      `loop { sti; hlt }` — corpo extraído literalmente de `sys_exit`
      (`kernel/src/syscall.rs`) — verificar que `cargo kernel-build` continua
      compilando.
- [x] 1.2 Atualizar `kernel::syscall::sys_exit` para `fn sys_exit() -> ! {
      crate::scheduler::terminate_current_thread() }`, removendo o laço duplicado —
      verificar com `cargo test -p kernel --lib` que nenhum teste existente quebra
      (nenhum teste chama `sys_exit` diretamente hoje, já que usa instruções
      privilegiadas — confirmar que essa suposição continua válida) e com `cargo
      xtask run` que `exit_demo` continua reportando sua mensagem antes de
      `SYSCALL_EXIT` normalmente (mesmo comportamento de antes). **Confirmado**:
      271 testes passando (269 + os 2 novos de `is_user_mode`, tarefa 2.3).

## 2. Distinção de modo nos handlers de `#PF`/`#GP`

- [x] 2.1 Em `kernel::idt::page_fault_handler`, depois do diagnóstico já existente
      (e depois de checar a exceção de safepoint, que continua recuperável e
      retornando antes de qualquer coisa abaixo): se `frame.code_segment & 0x3 ==
      3` (modo usuário), reportar a falta (mesmo formato de hoje, acrescentando
      que a thread será encerrada) e chamar
      `kernel::scheduler::terminate_current_thread()` em vez de `hlt_loop()`; caso
      contrário (modo kernel), comportamento inalterado (`hlt_loop()`) — verificar
      com `cargo test -p kernel --lib` (nenhum teste de host deve testar o corpo do
      handler diretamente, só a lógica pura de RPL se for extraída — ver tarefa
      2.3) e com `cargo kernel-build` que compila.
- [x] 2.2 Mesma mudança em `kernel::idt::general_protection_fault_handler`: `RPL ==
      3` reporta e chama `terminate_current_thread()`; `RPL == 0` mantém
      `hlt_loop()` inalterado — verificar compilação como acima.
- [x] 2.3 Extrair a checagem de `RPL` (`frame.code_segment & 0x3 == 3`) para uma
      função pura `fn is_user_mode(code_segment: u64) -> bool` em `kernel/src/
      idt.rs`, testável em host sem hardware (mesmo padrão de `is_page_absent`,
      já existente no mesmo arquivo) — adicionar testes cobrindo
      `KERNEL_CODE_SELECTOR` (não é modo usuário) e `USER_CODE_SELECTOR` (é modo
      usuário) — verificar com `cargo test -p kernel --lib`.

## 3. Verificação em boot real

- [x] 3.1 Criar `userland_c/fault_test_program.c`: imprime uma mensagem confirmando
      que chegou até ali (`write()`), depois desreferencia deliberadamente um
      ponteiro inválido (ex.: `*(volatile int *)0 = 1`) — qualquer instrução depois
      dessa SHALL NOT executar (sem mensagem de sucesso depois dela, ao contrário
      de todo programa de teste anterior, que sempre reporta `P`/`F` ao final).
- [x] 3.2 Adicionar `build_fault_test_program` a `xtask/src/main.rs` (mesmo corpo
      dos `build_*_test_program` anteriores), chamar a partir de `build_image`, e
      copiar o ELF resultante para `::/bin/fault.elf` (nome curto 8.3 válido) —
      verificar que `cargo xtask build-image` produz o arquivo sem erro.
- [x] 3.3 Adicionar `FAULT_TEST_PROGRAM_STACK_VIRT`/`_STACK_SIZE` a `kernel/src/
      scheduler.rs` (endereço distinto dos já existentes) e carregar
      `/bin/fault.elf` via `kernel::scheduler::spawn_from_elf` em
      `kernel/src/main.rs`, **depois** de pelo menos um dos programas de teste já
      existentes continuarem rodando em laço (ex.: depois de `/bin/timedw.elf`),
      para que a verificação da tarefa 3.4 tenha threads concorrentes já em
      execução no momento da falta — verificar via `cargo xtask run` em QEMU que a
      mensagem de "chegou até ali" aparece no console serial.
- [x] 3.4 Confirmar, no mesmo log de boot: (a) a mensagem de diagnóstico do `#PF`
      em modo usuário aparece, identificando a thread encerrada; (b) nenhuma
      mensagem de sucesso de `fault_test_program` aparece depois dela (prova que
      nada depois da falta executou); (c) nenhum `EXCECAO`/pânico leva a um
      `hlt_loop` permanente — o boot continua, e todos os programas de teste já
      existentes (incluindo os que rodam em laço, como `test.elf`) continuam
      reportando exatamente os mesmos resultados de antes desta change, sem
      nenhuma regressão. **Confirmado**:
      ```
      fault_isolation: /bin/fault.elf carregado e em execucao
      fault_test_program: chegou ate aqui, vou causar um page fault de proposito
      EXCECAO #PF (page fault): endereco=0x0 codigo_erro=0x6 rip=0x4000d5 cr3=0xde00000 rsp=0xd0000000f30 modo_usuario=true - thread encerrada
      timedwait_test_program: resultado PP (ordem: etimedout,elapsed_ge_requested)
      ```
      Zero ocorrências de "ISTO NUNCA DEVERIA APARECER". `user_posix_demo`,
      `pthread`, `libc_test_program`, `safepoint`, `DLOPEN/DLSYM` e todos os
      demais continuam reportando exatamente os mesmos resultados logo depois da
      falta, sem regressão. `cargo test -p kernel --lib`: 271 testes passando.

## 4. Documentação

- [x] 4.1 Adicionar a este change (`design.md`, "Resultado real da implementação")
      a confirmação do que foi verificado em boot real e o relato de qualquer
      achado real durante a implementação.
- [x] 4.2 Atualizar `ROADMAP.md` — mover "Isolamento de falha por processo" para a
      seção de implementado (incluindo a ressalva de que libera-se só a thread
      causadora, equivalente a todo o processo hoje por não existir thread
      múltipla em modo usuário ainda, e que os recursos do processo continuam
      vazando), e apontar o próximo item real de médio prazo como o próximo passo
      — verificar que o documento permanece consistente. **Decisão**: "Próximo
      passo imediato" passa a apontar para "Liberação real de recursos do processo
      encerrado" — consequência direta e imediata do que esta change deixou como
      vazamento aceito.

## Workflow follow-up

- Arquivar esta change (`openspec archive kernel-fault-isolation`) depois que a
  verificação em boot real (tarefas 3.1-3.4) e `cargo test -p kernel --lib`
  estiverem confirmadas.
