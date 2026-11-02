# Spec Delta

## Purpose

Define as abstrações de processo (dono de um espaço de endereçamento isolado) e thread (unidade de escalonamento dentro de um processo, com stack própria protegida por guard page e contexto de registradores), além da criação de novos processos/threads executando código do próprio kernel.

## ADDED Requirements

### Requirement: Processo com Espaço de Endereçamento Próprio
O kernel SHALL permitir criar um processo com seu próprio espaço de endereçamento virtual, isolado do espaço de endereçamento de qualquer outro processo.

#### Scenario: Memória mapeada em um processo não é acessível a partir de outro
- **WHEN** um endereço virtual é mapeado dentro do espaço de endereçamento de um processo
- **THEN** esse mesmo endereço virtual, consultado no espaço de endereçamento de outro processo, SHALL NOT estar mapeado para o mesmo frame físico, a menos que explicitamente compartilhado

#### Scenario: Processo criado tem pelo menos uma thread
- **WHEN** um novo processo é criado
- **THEN** ele SHALL ter pelo menos uma thread pronta para ser escalonada

### Requirement: Thread com Stack Própria Protegida por Guard Page
Cada thread SHALL ter sua própria stack de kernel, mapeada com uma guard page imediatamente abaixo, e seu próprio contexto de registradores da CPU, independente de qualquer outra thread.

#### Scenario: Estouro da stack de uma thread não corrompe a stack de outra
- **WHEN** uma thread estoura sua própria stack
- **THEN** o resultado SHALL ser uma falha de página referenciando a guard page dessa thread, em vez de corromper a stack de qualquer outra thread

#### Scenario: Duas threads têm contextos de registradores independentes
- **WHEN** duas threads distintas são criadas
- **THEN** alterar o contexto de registradores salvo de uma SHALL NOT afetar o contexto salvo da outra

### Requirement: Criação de Processos e Threads em Modo Kernel
O kernel SHALL fornecer uma forma de criar um novo processo ou uma nova thread que comece a executar uma função do próprio kernel, sem exigir carregamento de um executável externo nesta capability.

#### Scenario: Nova thread criada executa a função fornecida
- **WHEN** uma nova thread é criada a partir de uma função do kernel
- **THEN** essa thread, quando escalonada pela primeira vez, SHALL começar a executar a partir dessa função
