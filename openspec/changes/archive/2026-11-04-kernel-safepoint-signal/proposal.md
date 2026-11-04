# Proposal

## Why

O inventário de `jvm-posix-compat-strategy` (ver `docs/architecture/overview.md`, seção 6) identifica o "sinal de safepoint" como uma primitiva que falta para o porte do HotSpot: uma página de *polling* protegida contra acesso, usada para parar todas as threads Java simultaneamente nos pontos de segurança do JIT sem polling explícito em cada ponto — equivalente a `sigaction(SIGSEGV, ...)` sobre uma página específica. Hoje, `kernel::idt::page_fault_handler` trata **toda** falta de página como evento fatal (reporta e halta o kernel inteiro) — não existe nenhum mecanismo de "esta falta de página é esperada, resolva e continue". Isso já foi registrado como Non-Goal explícito em `kernel-posix-virtual-memory` e `kernel-jvm-posix-syscalls` (ambas arquivadas), que descreveram o gap sem implementá-lo. Esta change fecha esse gap específico do roadmap (item 8, seção 15).

## What Changes

- O kernel passa a permitir registrar uma faixa de endereço virtual como "ponto de poll de safepoint": uma falta de página do tipo "página ausente" (não uma violação de permissão) dentro de uma faixa registrada deixa de ser fatal — o kernel simplesmente retoma a execução da thread no ponto exatamente anterior à instrução que faltou (`iret` sem nenhuma outra ação), fazendo-a tentar de novo. Combinado com o par já existente `SYSCALL_VM_DECOMMIT`/`SYSCALL_VM_COMMIT` (`kernel-jvm-posix-syscalls`), isso dá exatamente o comportamento do safepoint do HotSpot: descomprometer a página faz qualquer thread que a esteja sondando "parar" (repetindo a falta indefinidamente, sem busy-wait explícito em código de usuário, sem derrubar o kernel); recomprometê-la faz todas elas continuarem, na própria próxima tentativa, sem nenhuma outra sincronização.
- **Decisão de escopo** (ver `design.md` para alternativas descartadas): o mecanismo **não** implementa um desvio genérico de execução para um handler escolhido pelo processo (como um `sigaction(SIGSEGV, ...)` completo, com *trampoline* e salvamento de contexto). Em vez disso, implementa exatamente a semântica "retry automático" que o safepoint da JVM precisa — mais simples, sem o risco de reentrância na stack dedicada de IST que um desvio genérico exigiria (ver design.md), e suficiente para a necessidade real identificada no inventário.
- **BREAKING**: nenhuma — puramente aditivo. O comportamento de halt para qualquer outra falta de página (não registrada, ou um erro de permissão/escrita) continua exatamente como hoje.
- A exposição deste mecanismo a modo usuário (uma syscall de registro) fica para uma sub-change futura — mesma disciplina incremental já seguida por `kernel-wait-queue`, `kernel-posix-virtual-memory`, `kernel-pthread-sync` e `kernel-wall-clock` (todas implementaram a primitiva em modo kernel primeiro, com a exposição via syscall feita depois, de uma vez, em `kernel-jvm-posix-syscalls`). Esta change prova o mecanismo com um demo em modo kernel (mesmo padrão de `vmm_demo`/`pthread_demo`).

## Capabilities

### New Capabilities
- `kernel-safepoint`: registro de faixas de endereço virtual cuja falta de página (por ausência, não por violação de permissão) é recuperável — a thread automaticamente tenta de novo em vez do kernel haltar.

### Modified Capabilities
- `kernel-interrupts`: o requisito "Tratamento de Exceções de CPU" passa a descrever o novo comportamento de page fault para uma faixa registrada (retry em vez de halt), preservando o comportamento de halt para todo o resto (general protection fault, double fault, e qualquer page fault fora de uma faixa registrada).

## Impact

- **Código afetado**: `kernel/src/idt.rs` (`page_fault_handler` consulta o novo registro antes de haltar); novo módulo `kernel::safepoint` (registro de faixas — lógica pura, testável em host); um novo demo em modo kernel (dois ou mais threads: um "sondando" a página registrada, outro descomprometendo/recomprometendo-a via `kernel::vmm`), análogo a `kernel::vmm_demo`/`kernel::pthread_demo`.
- **Specs**: nova capability `kernel-safepoint`; delta em `kernel-interrupts` (requisito "Tratamento de Exceções de CPU" alterado, demais requisitos da capability inalterados).
- **Dependências**: reaproveita `kernel::vmm` (reserva/comprometimento/descomprometimento, já existentes) para criar e armar/desarmar a página de poll — nenhuma primitiva nova de memória virtual é necessária.
- **Compatibilidade**: puramente aditivo. Nenhuma falta de página hoje fatal passa a ser recuperável, a menos que a faixa correspondente seja explicitamente registrada (nenhuma faixa é registrada por padrão).
