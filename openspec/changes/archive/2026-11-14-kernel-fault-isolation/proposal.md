# Proposal

## Why

Hoje, qualquer exceção de CPU (`#PF`/`#GP`) — mesmo uma causada por código de modo
usuário comum, como desreferenciar um ponteiro inválido — entra em `hlt_loop()`
incondicional (`kernel::idt::page_fault_handler`/`general_protection_fault_handler`),
derrubando o kernel inteiro junto com o processo que causou a falta. Isso é aceitável
para um "hello world" de prova de conceito, mas inviabiliza qualquer teste de boot
real que rode código de usuário com risco real de falhar (incluindo o próprio HotSpot,
eventualmente) — hoje toda a bateria de programas de teste em boot real só funciona
porque nenhum deles jamais falha de propósito. Este é o "Próximo passo imediato" do
`ROADMAP.md` depois de `os-javaos-park-sync`: a base de sincronização de baixo nível
do HotSpot está completa, e isolamento de falha é o próximo passo kernel-side
concretamente acionável antes de examinar o runtime/GC/JIT/interpretador do HotSpot.

## What Changes

- `kernel::idt::page_fault_handler`/`general_protection_fault_handler` passam a
  distinguir a origem da exceção pelo *Current Privilege Level* (`RPL`, 2 bits baixos
  de `CS` no frame de interrupção): uma falta vinda de modo usuário (`RPL == 3`)
  encerra **só a thread causadora** (reaproveitando o mesmo mecanismo já existente de
  `SYSCALL_EXIT`/`mark_terminated` — a thread nunca mais volta à `ReadyQueue`), em vez
  de `hlt_loop()`; uma falta vinda de modo kernel (`RPL == 0`) continua se comportando
  exatamente como hoje (diagnóstico + halt total) — **isso não muda**.
- Extrai o laço final de `sys_exit` (`mark_terminated` + `sti`/`hlt` até a próxima
  troca de contexto) para uma função reaproveitável em `kernel::scheduler`, chamada
  tanto por `sys_exit` (encerramento voluntário, comportamento inalterado) quanto
  pelos dois handlers de exceção acima (encerramento involuntário, novo).
- Um novo programa de teste C (`userland_c/fault_test_program.c`), carregado em boot
  real, que desreferencia deliberadamente um ponteiro inválido depois de reportar que
  chegou até ali — prova que (a) o kernel não trava, (b) nenhuma instrução do programa
  depois da falta executa, e (c) os demais programas/threads já em execução continuam
  normalmente depois.

## Capabilities

### New Capabilities
(nenhuma)

### Modified Capabilities
- `kernel-interrupts`: o requisito "Tratamento de Exceções de CPU" (`#PF`/`#GP`)
  muda de "sempre reporta e entra em halt total" para "reporta e entra em halt total
  só quando a exceção veio de modo kernel; quando veio de modo usuário, encerra só a
  thread causadora e o resto do sistema continua". `#DF` (double fault, requisito
  separado "Double Fault com Stack Dedicada") **não muda** — continua sempre fatal,
  por ser causado por um estado já corrompido demais para confiar em qualquer
  distinção.
- `kernel-processes`: novo requisito sobre o encerramento involuntário de uma thread
  de modo usuário por exceção de CPU — mesma garantia observável de "nunca mais
  executa" que o encerramento voluntário já documentado (`libc-runtime`, "Término de
  Processo"), mas disparado pelo kernel em vez de pedido explícito do programa.

## Impact

- **Código afetado**: `kernel/src/idt.rs` (os dois handlers acima), `kernel/src/
  scheduler.rs` (nova função reaproveitável), `kernel/src/syscall.rs` (`sys_exit`
  passa a chamá-la em vez de duplicar o laço), `userland_c/fault_test_program.c`
  (novo), `kernel/src/main.rs`/`xtask/src/main.rs` (carregar o novo programa de
  teste, mesmo padrão dos já existentes).
- **Não afetado**: `#DF` (double fault) continua sempre fatal, sem distinção de
  modo — ver "Non-Goals" em `design.md`. Nenhuma mudança em `AddressSpace`/
  `pthread_handles`/`file_handles`/`PROCESSES` — os recursos do processo encerrado
  continuam vazando (mesma disciplina de "leak aceitável por enquanto" já aplicada a
  `SYSCALL_EXIT`); liberação real de recursos fica para uma change futura.
- **Verificação**: diferente das duas changes anteriores (`os-javaos-platform-sync`/
  `os-javaos-park-sync`, que não tinham como provar o caminho de falha em boot real),
  esta change pode ser totalmente provada em boot real — basta um programa que
  deliberadamente falhe e observar que o resto do boot continua.
