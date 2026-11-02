# Proposal

## Why

Hoje o kernel executa como um único fluxo sequencial: depois do boot, ele entra em `idle_loop()` para sempre, só acordando para tratar interrupções (timer, etc.). Não existe nenhuma forma de rodar múltiplas unidades de execução independentes, nem de isolar uma da outra, nem de interromper uma tarefa em andamento para dar tempo de CPU a outra. Isso bloqueia o próximo item do roadmap (`docs/architecture/overview.md`, seção 15, item 4) e, por extensão, tudo que depende de concorrência real no kernel: system calls atendendo múltiplos chamadores, drivers rodando ao lado de outro trabalho e, no fim, aplicações Java independentes rodando sob JVMs isoladas. Com paginação própria (`kernel-memory-management`), interrupções/timer (`kernel-interrupts-timer`) e uma stack do kernel protegida por guard page (`kernel-stack-guard-page`) já implementadas, as peças de base para processos, threads e um escalonador preemptivo já existem — falta apenas combiná-las.

## What Changes

- Nova abstração de **processo**: dono de seu próprio espaço de endereçamento (tabelas de página/CR3 próprios), construído sobre `kernel-paging`.
- Nova abstração de **thread**: a unidade de escalonamento dentro de um processo — sua própria stack do kernel protegida por guard page (generalizando `kernel-stack-protection`, hoje limitada a uma única stack estática de boot) e um contexto de registradores da CPU salvo.
- API em nível de kernel para criar novos processos/threads executando funções Rust em modo kernel (anel 0) — **sem** modo usuário/anel 3 e **sem** carregamento de executáveis (ELF) nesta change; isso fica para a change futura de "System Calls" (roadmap item 5) e além.
- Escalonador **preemptivo**, round-robin, disparado pelo timer periódico do APIC já existente (`kernel-timer`): a cada tick, o contexto da thread corrente é salvo e a próxima thread pronta é retomada.
- O `idle_loop()` final de `kernel_main` passa a significar "iniciar o escalonador"; quando nenhuma thread está pronta, a thread ociosa do escalonador faz exatamente o mesmo loop de halt já exigido pelo contrato de halt de `kernel-boot`.
- `kernel-paging` ganha a capacidade de criar um novo espaço de endereçamento (nova PML4, com o mapeamento higher-half do próprio kernel replicado nela) e de direcionar operações de mapeamento a um espaço de endereçamento explícito, não apenas ao que está ativo em `CR3` no momento.
- `kernel-stack-protection` generaliza de "a stack principal estática do kernel tem guard page" para "qualquer stack de thread alocada dinamicamente tem guard page", reaproveitando o mesmo mecanismo.

## Capabilities

### New Capabilities
- `kernel-processes`: processos (donos de um espaço de endereçamento) e threads (unidade de escalonamento dentro de um processo, com stack própria e contexto de registradores), incluindo a criação de novos processos/threads em modo kernel.
- `kernel-scheduler`: escalonamento preemptivo round-robin de threads, disparado pelo timer periódico já existente, incluindo a troca de contexto e o comportamento ocioso quando nenhuma thread está pronta.

### Modified Capabilities
- `kernel-paging`: adiciona a criação de um novo espaço de endereçamento isolado e a capacidade de direcionar operações de mapeamento a um espaço de endereçamento explícito (não apenas ao ativo em `CR3`) — necessário para montar o espaço de um processo antes de trocar para ele.
- `kernel-stack-protection`: generaliza a garantia de guard page de "a stack estática de boot do kernel" para "qualquer stack de kernel alocada para uma thread".

## Impact

- **Novos módulos no crate `kernel`**: `kernel::process` (processos/threads) e `kernel::scheduler` (escalonamento/troca de contexto).
- **`kernel::paging`**: nova API para criar/trocar espaços de endereçamento e para operar sobre um espaço explícito em vez de sempre o ativo.
- **`kernel::stack`**: generaliza de uma única stack estática para alocação de uma stack (com guard page) por thread.
- **`kernel_main`**: a cauda (`idle_loop()`) passa a iniciar o escalonador em vez de haltar para sempre.
- **`kernel::timer`**: o handler de interrupção do timer passa a acionar uma decisão de escalonamento a cada tick.
- **Nenhuma dependência externa nova esperada** (implementação própria, consistente com o padrão já usado no projeto).
