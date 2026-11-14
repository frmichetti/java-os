# Design

## Context

`kernel::idt::page_fault_handler`/`general_protection_fault_handler`
(`kernel/src/idt.rs`) hoje reportam a exceção e chamam `crate::hlt_loop()`
incondicionalmente — nenhuma distinção entre uma falta causada pelo próprio kernel
(um bug real, merece derrubar tudo) e uma causada por código de modo usuário comum
(deveria afetar só esse processo). `kernel::syscall::sys_exit` já resolve exatamente
o problema de "terminar uma thread sem derrubar o resto do sistema" para o caso
**voluntário** (`SYSCALL_EXIT`): marca a thread como terminada
(`scheduler::mark_terminated`) e entra num laço `sti`/`hlt` que nunca mais retoma —
o próprio tick do timer, no seu curso normal, vê `is_terminated(was_running) ==
true` em `schedule_next` e simplesmente não a devolve à `ReadyQueue`, escolhendo a
próxima thread pronta via o `iretq` já existente do caminho do timer. Ver
proposal.md para a motivação completa.

## Goals / Non-Goals

**Goals:**
- `#PF`/`#GP` originados em modo usuário (`RPL == 3` em `CS`, disponível no
  `InterruptStackFrame` que a ABI `extern "x86-interrupt"` já entrega) encerram
  permanentemente só a thread causadora, reaproveitando o mecanismo já existente de
  `mark_terminated`/remoção da `ReadyQueue` — nenhuma primitiva de escalonamento
  nova.
- `#PF`/`#GP` originados em modo kernel continuam se comportando exatamente como
  hoje (diagnóstico + halt total) — nenhuma regressão no caminho já existente.
- Reaproveitar, não duplicar: extrair o laço `mark_terminated` + `sti`/`hlt` de
  `sys_exit` para uma função única em `kernel::scheduler`, chamada pelos três
  pontos (`sys_exit`, `page_fault_handler`, `general_protection_fault_handler`).
- Verificação real em boot: um programa de teste que causa um `#PF` de propósito em
  modo usuário, confirmando que o kernel não trava e que outras threads continuam.

**Non-Goals:**
- `#DF` (double fault) — continua sempre fatal, sem nenhuma distinção de modo. Um
  double fault, por definição, já ocorre com o estado da CPU/stack comprometido o
  bastante para não ser seguro confiar em mais nenhuma decisão além de diagnosticar
  e parar (mesmo racional de já usar uma stack dedicada via IST em vez de tentar
  "continuar" de algum jeito).
- Liberação de recursos do processo encerrado (`AddressSpace`/frames físicos,
  entradas em `pthread_handles`/`file_handles`, o `Process`/`Thread` em si,
  `kernel::vmm`) — continuam vazando, mesma disciplina de "leak aceitável por
  enquanto" já aplicada ao encerramento voluntário (`SYSCALL_EXIT`) desde
  `libc-minimal-port`. Liberação real é um passo futuro distinto (ver `ROADMAP.md`,
  "Isolar as tabelas de handle hoje globais... por processo").
- Encerrar **todas** as threads de um processo com múltiplas threads quando uma
  delas falha — nenhum processo de modo usuário hoje tem mais de uma thread (não
  existe syscall de criação de thread em modo usuário ainda, mesma limitação já
  documentada em `os-javaos-platform-sync`/`kernel-timed-wait`), então "encerrar a
  thread causadora" e "encerrar o processo inteiro" são, na prática,
  **exatamente a mesma coisa** hoje. Propagar o encerramento para outras threads do
  mesmo processo fica para quando essa syscall existir.
- Adicionar handlers para exceções de CPU ainda sem handler dedicado (`#DE`, `#UD`,
  etc.) — fora de escopo; só `#PF`/`#GP` (os dois já tratados hoje) ganham a
  distinção de modo.
- Qualquer mudança em `kernel::wait_queue`/`kernel::pthread` — uma thread encerrada
  por exceção nunca está no meio de um bloqueio voluntário quando a exceção ocorre
  (a exceção interrompe código de usuário rodando livremente, não uma seção crítica
  do kernel), então nenhuma `WaitQueue` pode estar "presa" esperando por ela.

## Decisions

### Distinção pelo `RPL` de `CS` no frame de interrupção, não por qualquer outro sinal

O `InterruptStackFrame` que a ABI `extern "x86-interrupt"` do Rust já entrega
(`code_segment: u64`) contém o seletor de `CS` ativo no momento da exceção — seus 2
bits baixos são o `RPL` (*Current Privilege Level* efetivo): `3` para
`USER_CODE_SELECTOR` (`kernel::gdt`), `0` para `KERNEL_CODE_SELECTOR`. Checar
`frame.code_segment & 0x3 == 3` é suficiente e já é exatamente a informação que a
própria CPU usa para decidir se uma transição de privilégio ocorreu — nenhum estado
adicional do escalonador precisa ser consultado. **Alternativa rejeitada**: inferir
"modo usuário" a partir de alguma flag própria do escalonador (ex.: "a thread
corrente é de um processo com `AddressSpace` própria") — rejeitada por ser
redundante e menos direta que o que a própria CPU já informa no frame, além de
exigir manter esse estado sincronizado à parte.

### Reaproveitar o laço de `sys_exit`, extraído para `kernel::scheduler`

`sys_exit` já resolve com segurança o problema de "parar de executar esta thread
para sempre, sem travar o resto do sistema": `mark_terminated(current)` seguido de
`loop { sti; hlt }`. Isso funciona porque o **próprio tick seguinte do timer**
(no máximo ~10ms depois, a 100 Hz) processa esta thread como `was_running` em
`schedule_next`, vê `is_terminated() == true`, e simplesmente não a recoloca na
`ReadyQueue` — a próxima thread é escolhida e retomada via o `iretq` que o caminho
do timer já usa normalmente, sem nenhum código adicional de "troca de contexto
manual" dentro do handler de exceção. Extraída para
`kernel::scheduler::terminate_current_thread() -> !`, chamada por `sys_exit` (sem
mudança de comportamento) e pelos dois handlers de exceção (novo).

**Por que isto é seguro a partir de um handler de exceção, inclusive na stack IST
dedicada de `#PF`**: no instante em que a exceção ocorreu, `CS` tinha `RPL == 3` —
ou seja, a CPU estava executando código de modo usuário, nunca dentro de uma seção
crítica do kernel (`with_interrupts_disabled`/`Spinlock::lock`), então nenhum lock
global pode estar retido. Como este kernel é single-core (sem SMP, decisão já
registrada no `ROADMAP.md`), só uma thread executa por vez: ao entrar no laço
`sti`/`hlt` desta thread (ainda na stack IST do `#PF`, ou na stack de kernel normal
para `#GP`, que usa `ist_index = 0`), nenhuma outra thread pode estar
simultaneamente usando a mesma stack IST — a próxima vez que `#PF` for entregue (a
qualquer thread, inclusive uma completamente diferente), a thread atual já terá
sido removida da `ReadyQueue` pelo tick do timer muito antes de voltar a executar
qualquer coisa que pudesse causar um novo `#PF`.

### `sys_exit` passa a delegar, sem duplicar

`fn sys_exit() -> ! { crate::scheduler::terminate_current_thread() }` — nenhuma
mudança de comportamento observável, só elimina a duplicação do laço
`mark_terminated`/`sti`/`hlt` entre `syscall.rs` e `idt.rs`.

## Risks / Trade-offs

- **[Risco] Um handler de exceção que nunca retorna (em vez de completar o `iretq`
  normal da ABI `extern "x86-interrupt"`) é um padrão incomum — um erro aqui poderia
  deixar a CPU presa de um jeito sutil** → Mitigação: o padrão (`mark_terminated` +
  `sti`/`hlt` infinito) já está em produção via `sys_exit` desde `libc-minimal-port`,
  verificado em boot real múltiplas vezes desde então (`exit_demo`) — esta change só
  adiciona dois novos pontos de entrada para o mesmo laço já comprovado, não uma
  lógica nova.
- **[Risco] `#DF`/outras exceções sem handler dedicado continuam sempre fatais,
  mesmo se também causadas por modo usuário** → Aceito, documentado como Non-Goal:
  `#DF` já ocorre com estado comprometido demais para confiar em qualquer decisão
  além de parar; exceções sem handler dedicado são uma lacuna pré-existente, fora do
  escopo desta change.
- **[Risco] Vazamento de recursos por processo encerrado (`AddressSpace`, handles)
  cresce sem limite se faltas em modo usuário se tornarem frequentes** → Aceito,
  mesma disciplina já aplicada a `SYSCALL_EXIT`; liberação real de recursos é
  trabalho futuro já registrado no `ROADMAP.md`.

## Migration Plan

Puramente aditivo no comportamento observável para o caso que já existia (falta em
modo kernel, inalterado) — o único comportamento que muda é o de um caso que hoje
sempre derruba o kernel (falta em modo usuário), substituído por um comportamento
estritamente melhor (isolamento em vez de halt total). Nenhum passo de migração
necessário.

## Resultado real da implementação

Toda a implementação seguiu exatamente o planejado acima, sem nenhum desvio de
design: `kernel::scheduler::terminate_current_thread()` extraída do laço já existente
de `sys_exit`; `page_fault_handler`/`general_protection_fault_handler` checam
`is_user_mode(frame.code_segment)` (função pura, testada em host) e chamam essa
função em vez de `hlt_loop()` quando a falta veio de modo usuário. Nenhum achado
real inesperado durante a implementação — a reutilização do laço `sti`/`hlt` já
comprovado por `sys_exit` funcionou exatamente como previsto na primeira tentativa.

**Verificado em boot real** (`userland_c/fault_test_program.c`, carregado como
`/bin/fault.elf`, depois de `/bin/timedw.elf` e vários outros processos/threads já
em execução):
```
fault_isolation: /bin/fault.elf carregado e em execucao
fault_test_program: chegou ate aqui, vou causar um page fault de proposito
EXCECAO #PF (page fault): endereco=0x0 codigo_erro=0x6 rip=0x4000d5 cr3=0xde00000 rsp=0xd0000000f30 modo_usuario=true - thread encerrada
timedwait_test_program: resultado PP (ordem: etimedout,elapsed_ge_requested)
```
- `modo_usuario=true` confirma a distinção de `RPL` funcionando corretamente.
- Zero ocorrências da mensagem "nunca deveria aparecer" — nenhuma instrução do
  programa depois da escrita em `endereco=0x0` executou.
- O boot continuou normalmente logo em seguida (sem `hlt_loop`/pânico): todos os
  demais programas de teste já existentes (`user_posix_demo`, `pthread`,
  `libc_test_program`, `safepoint`, `DLOPEN/DLSYM`, etc.) continuaram reportando
  exatamente os mesmos resultados de antes desta change.
- `cargo test -p kernel --lib`: 271 testes passando (269 de antes + os 2 novos de
  `is_user_mode`).
