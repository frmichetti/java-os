# os-javaos-memory Specification

## Purpose

Define o comportamento real (não mais stub) dos hooks de memória da camada
`os`/`os_cpu` do porte HotSpot (`os_javaos.cpp`) sobre as syscalls de memória virtual
que o kernel do Java OS já expõe, incluindo os casos em que nenhuma primitiva de
kernel equivalente existe hoje.

## Requirements

### Requirement: Reserva de Memória Virtual via `pd_reserve_memory`
`os::pd_reserve_memory` SHALL reservar uma faixa de memória virtual anônima do
tamanho pedido através da syscall de reserva do kernel, devolvendo o endereço base da
faixa reservada em vez de abortar o processo.

#### Scenario: Reserva bem-sucedida devolve um endereço válido e não nulo
- **WHEN** `os::pd_reserve_memory` é chamado com um tamanho de pelo menos uma página
- **THEN** a função SHALL devolver um ponteiro não nulo correspondente ao início de
  uma faixa de memória virtual reservada, sem comprometer memória física

#### Scenario: Reserva sem espaço virtual disponível falha de forma explícita
- **WHEN** a syscall de reserva do kernel rejeita o pedido (sem espaço virtual livre
  suficiente)
- **THEN** `os::pd_reserve_memory` SHALL devolver `nullptr`, sem abortar o processo

### Requirement: Comprometimento de Memória via `pd_commit_memory`
`os::pd_commit_memory` (ambas as sobrecargas exigidas pela interface do HotSpot)
SHALL comprometer, através da syscall de comprometimento do kernel, a sub-região
pedida de uma faixa previamente reservada, com permissão de escrita sempre concedida
e permissão de execução condicionada ao parâmetro `executable`.

#### Scenario: Comprometimento bem-sucedido torna a memória gravável
- **WHEN** `os::pd_commit_memory` é chamado para uma sub-região dentro de uma reserva
  viva, com `executable = false`
- **THEN** a função SHALL devolver `true`, e uma escrita subsequente a qualquer
  endereço dentro dessa sub-região SHALL ser aceita pelo kernel

#### Scenario: Comprometimento fora de qualquer reserva viva falha de forma explícita
- **WHEN** `os::pd_commit_memory` é chamado com um endereço que não cai dentro de
  nenhuma faixa reservada
- **THEN** a função SHALL devolver `false`, sem abortar o processo

### Requirement: Comprometimento ou Encerramento via `pd_commit_memory_or_exit`
`os::pd_commit_memory_or_exit` (ambas as sobrecargas) SHALL tentar o mesmo
comprometimento de `pd_commit_memory` e, caso falhe, SHALL encerrar o processo através
do mecanismo padrão de falta de memória do HotSpot (`vm_exit_out_of_memory`) com a
mensagem fornecida pelo chamador, em vez de retornar um valor de erro silencioso.

#### Scenario: Falha de comprometimento aciona encerramento por falta de memória
- **WHEN** a tentativa interna de comprometimento falha
- **THEN** `os::pd_commit_memory_or_exit` SHALL encerrar o processo através de
  `vm_exit_out_of_memory`, passando adiante a mensagem recebida do chamador

### Requirement: Descomprometimento de Memória via `pd_uncommit_memory`
`os::pd_uncommit_memory` SHALL descomprometer, através da syscall de
descomprometimento do kernel, a sub-região pedida de uma faixa previamente
comprometida, liberando os frames físicos associados de volta ao kernel.

#### Scenario: Descomprometimento bem-sucedido torna a sub-região inacessível
- **WHEN** `os::pd_uncommit_memory` é chamado para uma sub-região previamente
  comprometida
- **THEN** a função SHALL devolver `true`, e um acesso subsequente a um endereço
  dentro dessa sub-região SHALL falhar, em vez de ser silenciosamente permitido

### Requirement: Liberação de Memória via `pd_release_memory`
`os::pd_release_memory` SHALL liberar, através da syscall de liberação do kernel, a
reserva completa cuja base seja exatamente o endereço informado.

#### Scenario: Liberação bem-sucedida permite reutilização do espaço virtual
- **WHEN** `os::pd_release_memory` é chamado com a base exata de uma reserva viva
- **THEN** a função SHALL devolver `true`, e uma reserva futura do mesmo tamanho SHALL
  poder reutilizar o mesmo espaço de endereço virtual

#### Scenario: Liberação de um endereço que não é a base de uma reserva falha
- **WHEN** `os::pd_release_memory` é chamado com um endereço que cai no meio de uma
  reserva viva, mas não é sua base
- **THEN** a função SHALL devolver `false`, sem liberar nenhuma memória

### Requirement: Alteração de Permissões via `os::protect_memory`
`os::protect_memory` SHALL alterar, através da syscall de proteção do kernel, as
permissões de acesso de uma sub-região previamente comprometida, para cada nível de
`ProtType` que o esquema de permissões do kernel consegue expressar fielmente.

#### Scenario: Proteger como somente leitura impede escrita subsequente
- **WHEN** `os::protect_memory` é chamado com `ProtType::MEM_PROT_READ` sobre uma
  sub-região comprometida e gravável
- **THEN** a função SHALL devolver `true`, e uma escrita subsequente a essa
  sub-região SHALL falhar

#### Scenario: Pedido de MEM_PROT_NONE é recusado em vez de simulado
- **WHEN** `os::protect_memory` é chamado com `ProtType::MEM_PROT_NONE` — que exigiria
  revogar a permissão de leitura, algo que o kernel não expressa hoje (toda página
  mapeada permanece legível)
- **THEN** a função SHALL devolver `false`, sem alterar as permissões atuais da
  sub-região e sem aplicar uma proteção mais fraca do que a pedida

### Requirement: Hooks de Memória Sem Primitiva de Kernel Equivalente Permanecem Stub Explícito
Todo `pd_*` de memória para o qual nenhuma syscall do kernel oferece a primitiva
necessária (reserva em endereço fixo, mapeamento de arquivo em memória, páginas
grandes, realinhamento de uma região já reservada) SHALL continuar abortando o
processo com uma mensagem de diagnóstico explícita identificando a função, em vez de
falhar silenciosamente ou devolver um resultado parcialmente correto.

#### Scenario: Reserva em endereço fixo continua explicitamente não suportada
- **WHEN** `os::pd_attempt_reserve_memory_at` é chamado
- **THEN** o processo SHALL abortar com uma mensagem de diagnóstico identificando
  `pd_attempt_reserve_memory_at` como não implementado

#### Scenario: Mapeamento de arquivo em memória continua explicitamente não suportado
- **WHEN** `os::pd_map_memory`, `os::pd_attempt_map_memory_to_file_at` ou
  `os::pd_unmap_memory` é chamado
- **THEN** o processo SHALL abortar com uma mensagem de diagnóstico identificando a
  função chamada como não implementada

#### Scenario: Páginas grandes continuam explicitamente não suportadas
- **WHEN** `os::pd_reserve_memory_special` é chamado
- **THEN** o processo SHALL abortar com uma mensagem de diagnóstico identificando
  `pd_reserve_memory_special` como não implementado

#### Scenario: Realinhamento de região reservada continua explicitamente não suportado
- **WHEN** `os::pd_realign_memory` é chamado
- **THEN** o processo SHALL abortar com uma mensagem de diagnóstico identificando
  `pd_realign_memory` como não implementado
