# kernel-physical-memory Specification

## Purpose

Gerencia a alocação e liberação de frames físicos de 4 KiB a partir das regiões de memória marcadas como usáveis pelo bootloader, servindo de base para o heap do kernel e para o gerenciamento de paginação.

## Requirements

### Requirement: Inicialização a partir do Memory Map
O alocador físico SHALL inicializar seu pool de frames livres usando exclusivamente as regiões marcadas como usáveis no memory map reportado pelo bootloader, sem considerar alocável nenhuma outra região (reservada, ACPI, do próprio kernel/módulos, etc.).

#### Scenario: Apenas regiões usáveis entram no pool de frames livres
- **WHEN** o alocador físico é inicializado com um memory map contendo regiões usáveis e não-usáveis
- **THEN** somente os frames dentro das regiões usáveis ficam disponíveis para alocação

#### Scenario: Memory map indisponível impede a inicialização
- **WHEN** o bootloader não fornece um memory map
- **THEN** o alocador físico SHALL reportar a falha de inicialização em vez de assumir um layout de memória arbitrário

### Requirement: Alocação de Frames Físicos
O alocador físico SHALL permitir a alocação de um frame livre de 4 KiB sob demanda.

#### Scenario: Alocação bem-sucedida quando há memória livre
- **WHEN** existe pelo menos um frame livre no pool
- **THEN** a alocação retorna o endereço físico de um frame livre e esse frame deixa de estar disponível para outras alocações até ser liberado

#### Scenario: Falha explícita quando não há memória livre
- **WHEN** não existe nenhum frame livre no pool
- **THEN** a alocação SHALL falhar de forma explícita e detectável, em vez de retornar um frame inválido, duplicado ou fora do intervalo gerenciado

### Requirement: Liberação de Frames Físicos
O alocador físico SHALL permitir devolver ao pool um frame previamente alocado por ele.

#### Scenario: Frame liberado pode ser realocado
- **WHEN** um frame previamente alocado é liberado
- **THEN** esse frame volta a estar disponível para alocações futuras

#### Scenario: Liberação de frame inválido é rejeitada
- **WHEN** é solicitada a liberação de um endereço que não corresponde a um frame atualmente alocado por este alocador (fora do intervalo gerenciado, ou já livre)
- **THEN** o alocador SHALL rejeitar a operação em vez de corromper seu estado interno silenciosamente

### Requirement: Relato de Estatísticas de Memória Física
O alocador físico SHALL expor a contagem total de frames gerenciados e quantos deles estão livres no momento.

#### Scenario: Contagem refletida corretamente após alocações e liberações
- **WHEN** frames são alocados e liberados ao longo do tempo
- **THEN** a contagem de frames livres relatada reflete exatamente o número de frames atualmente disponíveis no pool
