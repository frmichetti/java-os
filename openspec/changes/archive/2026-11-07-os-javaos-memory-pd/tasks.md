# Tasks

## 1. API em C para as syscalls de VM (`javaos_vm.h`/`glue.c`)

- [x] 1.1 Adicionar `SYS_VM_DECOMMIT=4`/`SYS_VM_PROTECT=5`/`SYS_VM_RELEASE=6` a
      `userland_c/glue.c`, mesma convenção dos `SYS_VM_RESERVE`/`SYS_VM_COMMIT`
      existentes — verificar que os valores batem exatamente com
      `kernel::syscall::SYSCALL_VM_DECOMMIT`/`PROTECT`/`RELEASE`
      (`kernel/src/syscall.rs`).
- [x] 1.2 Criar `userland_c/include/javaos_vm.h` com vinculação `extern "C"` para
      `javaos_vm_reserve`/`javaos_vm_commit`/`javaos_vm_decommit`/`javaos_vm_protect`/
      `javaos_vm_release`, assinaturas em termos de `void*`/`size_t`/`int` (prot como
      bitmask `0b01`=gravável/`0b10`=executável, mesma convenção de
      `kernel::syscall::prot_to_page_flags`) — verificar que o header compila sozinho
      sob `g++ -fsyntax-only` (simula o uso por `os_javaos.cpp`, que só precisa da
      declaração).
- [x] 1.3 Implementar as cinco funções em `userland_c/glue.c` reaproveitando
      `javaos_syscall` já existente — verificar que `cargo xtask build`
      (toolchain cruzado do projeto) compila `userland_c` sem erro.

## 2. Programa de demonstração em boot real

**Correção de design feita durante a implementação** (ver `design.md`, Decisions —
"Verificação em boot real via um programa C real"): um módulo `kernel::*_demo` em
`global_asm!` (plano original desta seção) não pode chamar uma função de
`javaos_vm.h`/`glue.c` — é um blob de código copiado sem nenhum passo de link. As
tarefas abaixo usam em vez disso um programa C real, mesmo mecanismo exato de
`userland_c/libc_test_program.c`/`cxx_test_program.cpp`.

- [x] 2.1 Criar `userland_c/javaos_vm_test_program.c` incluindo `javaos_vm.h` e
      exercitando em sequência: reservar via `javaos_vm_reserve`, comprometer via
      `javaos_vm_commit`, escrever e reler o valor escrito, proteger como somente
      leitura via `javaos_vm_protect`, descomprometer via `javaos_vm_decommit`,
      liberar via `javaos_vm_release`, e por fim chamar `javaos_vm_release` com um
      endereço que não é a base de nenhuma reserva (confirma falha explícita, mesma
      garantia de `sys_vm_release` já testada em `kernel/src/syscall.rs`) — cada
      cenário reporta `P`/`F` numa única mensagem via `write()`, mesmo formato de
      `libc_test_program.c`.
- [x] 2.2 Adicionar `build_javaos_vm_test_program` a `xtask/src/main.rs` (mesmo
      corpo de `build_libc_test_program`, trocando o `.c` de entrada), chamá-la a
      partir de `build_image`, e copiar o ELF resultante para `::/bin/javaosvm.elf`
      na imagem de disco — verificar que `cargo xtask build-image` produz o arquivo
      sem erro.
- [x] 2.3 Adicionar `JAVAOS_VM_TEST_PROGRAM_STACK_VIRT`/`_STACK_SIZE` a
      `kernel/src/scheduler.rs` (mesmo padrão de `CXX_TEST_PROGRAM_STACK_VIRT`,
      endereço distinto) e carregar `/bin/javaosvm.elf` via
      `kernel::scheduler::spawn_from_elf` em `kernel/src/main.rs`, mesma disciplina de
      erro explícito (nunca pânico) dos três carregamentos ELF já existentes ali —
      verificar via `cargo xtask run` em QEMU que a mensagem de resultado aparece no
      console serial com todos os cenários `P`.

## 3. Corpos reais dos `pd_*` de memória em `os_javaos.cpp`

- [x] 3.1 Implementar `os::pd_reserve_memory` chamando `javaos_vm_reserve`,
      devolvendo `nullptr` em caso de erro — verificar com
      `g++ -fsyntax-only -std=gnu++17 ...` (mesma invocação documentada no `design.md`
      de `os-javaos-skeleton`) contra os headers reais vendorizados, incluindo agora
      `-include javaos_vm.h` ou `#include` direto no arquivo.
- [x] 3.2 Implementar as duas sobrecargas de `os::pd_commit_memory` chamando
      `javaos_vm_commit` com a máscara de proteção derivada de `executable` (sempre
      gravável, executável conforme o parâmetro) — verificar compilação como acima.
- [x] 3.3 Implementar as duas sobrecargas de `os::pd_commit_memory_or_exit`
      chamando a implementação de `pd_commit_memory` da task 3.2 e, em caso de falha,
      `vm_exit_out_of_memory(bytes, OOM_MMAP_ERROR, "%s", mesg)` (mesmo padrão de
      `os_bsd.cpp::pd_commit_memory_or_exit`, consultado como referência) — verificar
      compilação como acima.
- [x] 3.4 Implementar `os::pd_uncommit_memory` chamando `javaos_vm_decommit` —
      verificar compilação como acima.
- [x] 3.5 Implementar `os::pd_release_memory` chamando `javaos_vm_release` —
      verificar compilação como acima.
- [x] 3.6 Implementar `os::protect_memory` (declarada em `share/runtime/os.hpp`,
      ausente de `os_javaos.cpp` — achado documentado em `proposal.md`/Impact) chamando
      `javaos_vm_protect` para `MEM_PROT_READ`/`MEM_PROT_RW`/`MEM_PROT_RWX`, e
      devolvendo `false` sem chamar a syscall para `MEM_PROT_NONE` — verificar
      compilação como acima.
- [x] 3.7 Atualizar a tabela de compilação por arquivo (mesmo formato de
      `os-javaos-skeleton`/design.md) para `os_javaos.cpp`, registrando que os seis
      hooks acima deixaram de ser `fatal()` e agora têm corpo real — documentar nesta
      própria change (ver tarefa 4.1), não reescrever o relato arquivado de
      `os-javaos-skeleton`.

## 4. Documentação

- [x] 4.1 Adicionar a este change (`design.md` ou um novo "Resultado real da
      implementação", mesmo padrão de `os-javaos-skeleton`) a tabela de compilação
      atualizada e o relato de qualquer achado real durante a implementação —
      verificar que todo `pd_*` de memória tocado nesta change (6 com corpo real, 6
      que permanecem `fatal()`: `pd_attempt_reserve_memory_at`,
      `pd_attempt_map_memory_to_file_at`, `pd_map_memory`, `pd_unmap_memory`,
      `pd_reserve_memory_special`, `pd_realign_memory`) está listado com seu status
      final.
- [x] 4.2 Atualizar `ROADMAP.md` — mover o item concluído de "Próximo passo
      imediato" para a seção de implementado, e apontar o próximo item real (item 2
      do roadmap médio prazo: `PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/
      `PlatformParker`) como a continuação — verificar que o documento permanece
      consistente (sem referência pendente a um "próximo passo" já concluído).

## Workflow follow-up

- Arquivar esta change (`openspec archive os-javaos-memory-pd`) depois que a
  verificação em boot real (tarefa 2.2) e a compilação `-fsyntax-only` de todos os
  seis hooks (tarefas 3.1–3.6) estiverem confirmadas.
