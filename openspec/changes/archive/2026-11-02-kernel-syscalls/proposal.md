# Proposal

## Why

Hoje todo código do kernel roda em anel 0 (modo kernel) — não existe nenhuma forma de executar código com privilégio reduzido, nem de esse código solicitar serviços do kernel de forma controlada. Isso bloqueia o próximo item do roadmap (`docs/architecture/overview.md`, seção 15, item 5) e é um pré-requisito direto de tudo que vem depois: a JVM (e, no futuro, aplicações Java) precisa rodar isolada do kernel, pedindo serviços (E/S, memória, etc.) através de uma fronteira bem definida, não compartilhando o mesmo nível de privilégio e confiança do próprio kernel. Com processos/threads e um escalonador preemptivo já implementados (`kernel-process-scheduler`), e com o suporte a páginas acessíveis por usuário já existente em `kernel-paging` (`PageFlags.user_accessible`, nunca usado até agora), as peças de base para introduzir modo usuário e uma primeira chamada de sistema real já existem.

## What Changes

- Novos segmentos de GDT para código/dados de usuário (anel 3), na ordem exigida pelo par de instruções `syscall`/`sysret` (ver design.md) — complementando os segmentos de kernel já existentes.
- `kernel::process` ganha a capacidade de criar uma thread que executa em **modo usuário** (anel 3): código e stack próprios, mapeados como acessíveis por usuário, em vez de executar com o privilégio do kernel.
- `kernel::scheduler` passa a manter `TSS.rsp0` (a stack de kernel confiável usada pela CPU em qualquer transição de anel 3 para anel 0) sempre apontando para a stack de kernel da thread atualmente em execução — necessário tanto para interrupções quanto para `syscall` atingirem uma thread de usuário com segurança.
- Nova interface de chamada de sistema via `syscall`/`sysret` (MSRs `STAR`/`LSTAR`/`SFMASK`, `EFER.SCE`): uma rotina de entrada em assembly que troca para a stack de kernel da thread corrente antes de tocar qualquer dado vindo do modo usuário, despacha para um handler Rust por número de chamada, e retorna via `sysret`.
- Convenção de ABI de syscall definida nesta change: registradores usados para número/argumentos/retorno, e numeração das primeiras chamadas.
- Duas primeiras chamadas de sistema reais: uma que escreve dados no console serial (prova de E/S do modo usuário para o kernel) e uma que consulta o contador de ticks do kernel (prova de retorno de valor do kernel para o modo usuário).
- Um pequeno programa de teste em modo usuário (código de máquina gerado via `global_asm!`, copiado para uma página acessível por usuário em tempo de execução — não há carregador de executáveis ainda, ver Non-Goals em design.md) para verificar o mecanismo ponta a ponta.

## Capabilities

### New Capabilities
- `kernel-user-mode`: execução de uma thread em anel 3 (modo usuário) — segmentos de GDT, código e stack mapeados como acessíveis por usuário, e a garantia de que código em anel 3 não acessa memória exclusiva do kernel.
- `kernel-syscalls`: a interface `syscall`/`sysret` em si — a troca segura para a stack de kernel da thread corrente, a convenção de ABI (número/argumentos/retorno), o despacho por número de chamada, tratamento explícito de número de chamada inválido, e as primeiras chamadas reais.

### Modified Capabilities
- `kernel-processes`: adiciona a capacidade de uma thread executar em modo usuário (com sua própria stack de usuário), além do já existente modo kernel.
- `kernel-scheduler`: a troca de contexto passa também a manter a stack de kernel confiável para transições de privilégio (`TSS.rsp0`) sempre correta para a thread corrente — necessário para que interrupções e `syscall` vindas de uma thread de usuário aterrissem numa stack de kernel válida e isolada.

## Impact

- **Novo módulo no crate `kernel`**: `kernel::syscall` (mecanismo `syscall`/`sysret`, despacho, as primeiras chamadas).
- **`kernel::gdt`**: novos segmentos de código/dados de usuário; configuração dos MSRs `STAR`/`LSTAR`/`SFMASK` e do bit `EFER.SCE`.
- **`kernel::process`**: `Thread` ganha uma stack de usuário opcional e uma forma de ser criada para rodar em anel 3; o campo usado para `TSS.rsp0` (topo fixo da stack de kernel da thread, distinto do `rsp` corrente que já existe) passa a ser rastreado.
- **`kernel::scheduler`**: a troca de contexto (tick do timer e, agora, também a entrada/saída de `syscall`) atualiza `TSS.rsp0` a cada troca.
- **Nenhuma dependência externa nova esperada** (implementação própria, consistente com o padrão já usado no projeto).
