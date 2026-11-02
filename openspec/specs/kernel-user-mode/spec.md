# kernel-user-mode Specification

## Purpose

Permite ao kernel executar código em modo usuário (anel 3, privilégio reduzido), com seu próprio código e stack mapeados como acessíveis por usuário, sem acesso à memória exclusiva do kernel.

## Requirements

### Requirement: Thread Executa em Modo Usuário com Privilégio Reduzido
O kernel SHALL permitir que uma thread execute seu código em anel 3 (modo usuário), com uma stack própria mapeada como acessível por usuário, distinta da stack de kernel dessa mesma thread.

#### Scenario: Thread de usuário executa com privilégio reduzido
- **WHEN** uma thread criada para rodar em modo usuário é escalonada
- **THEN** ela SHALL executar com o nível de privilégio da CPU em anel 3, usando sua própria stack de usuário

### Requirement: Código em Modo Usuário Não Acessa Memória Exclusiva do Kernel
Páginas de código e dados exclusivas do kernel (não marcadas como acessíveis por usuário) SHALL permanecer inacessíveis a partir de código executando em anel 3.

#### Scenario: Acesso a memória do kernel a partir do modo usuário falha
- **WHEN** código executando em anel 3 tenta ler, escrever ou executar um endereço mapeado apenas para o kernel (sem acesso de usuário)
- **THEN** a CPU SHALL gerar uma falha (ex.: page fault), em vez de permitir o acesso
