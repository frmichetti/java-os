# Spec Delta

## Purpose

Permite reservar faixas de memória virtual anônima sem comprometer memória física imediatamente, e comprometer, descomprometer, proteger e liberar sub-regiões dessas faixas sob demanda — a base de `mmap`/`munmap`/`mprotect` POSIX.

## ADDED Requirements

### Requirement: Reserva de Faixa de Memória Virtual Anônima
O kernel SHALL permitir reservar uma faixa de endereço virtual de tamanho arbitrário (arredondado para múltiplo de página), sem comprometer nenhuma memória física no momento da reserva.

#### Scenario: Reservar não consome memória física
- **WHEN** uma faixa de memória virtual é reservada
- **THEN** nenhum frame físico é alocado como resultado dessa reserva, e a contagem de frames físicos livres permanece inalterada

#### Scenario: Reserva maior que o espaço virtual disponível é rejeitada
- **WHEN** é solicitada uma reserva maior do que o espaço virtual livre atualmente disponível para reservas
- **THEN** a operação SHALL ser rejeitada de forma explícita, em vez de retornar uma faixa inválida ou sobreposta a outra já reservada

### Requirement: Comprometimento Sob Demanda de uma Sub-região Reservada
O kernel SHALL permitir comprometer uma sub-região (offset e tamanho) de uma faixa previamente reservada, mapeando frames físicos zerados para ela com as permissões de acesso especificadas.

#### Scenario: Memória comprometida é entregue zerada
- **WHEN** uma sub-região é comprometida
- **THEN** a leitura de qualquer endereço dentro dela, antes de qualquer escrita, retorna exclusivamente bytes zero

#### Scenario: Acesso respeita exatamente as permissões do comprometimento
- **WHEN** uma sub-região é comprometida com um conjunto de permissões
- **THEN** acessos a essa sub-região respeitam exatamente as permissões especificadas (ex.: uma escrita só é permitida se a permissão de escrita foi concedida)

#### Scenario: Comprometer fora dos limites da reserva é rejeitado
- **WHEN** é solicitado o comprometimento de uma sub-região cujo offset e tamanho ultrapassam os limites da faixa reservada
- **THEN** a operação SHALL ser rejeitada, e nenhum frame físico SHALL ser alocado como efeito colateral

#### Scenario: Comprometer uma sub-região já comprometida sem solicitar substituição é rejeitado
- **WHEN** é solicitado o comprometimento de uma sub-região que já está total ou parcialmente comprometida, sem indicar explicitamente que o comprometimento existente deve ser substituído
- **THEN** a operação SHALL ser rejeitada em vez de vazar os frames físicos já comprometidos

### Requirement: Acesso a Sub-região Não Comprometida Falha
Acessar um endereço dentro de uma faixa reservada, mas cuja sub-região correspondente ainda não foi comprometida, SHALL falhar, em vez de ser silenciosamente permitido ou de expor memória física arbitrária.

#### Scenario: Leitura ou escrita numa sub-região reservada e não comprometida gera falha
- **WHEN** ocorre um acesso a um endereço dentro de uma faixa reservada cuja sub-região correspondente nunca foi comprometida
- **THEN** o acesso SHALL falhar (ex.: page fault) em vez de retornar dados ou permitir a escrita

### Requirement: Descomprometimento de uma Sub-região Comprometida
O kernel SHALL permitir descomprometer uma sub-região previamente comprometida, liberando os frames físicos de volta ao alocador físico e preservando a reserva virtual para um comprometimento futuro.

#### Scenario: Frames descomprometidos voltam a ficar disponíveis
- **WHEN** uma sub-região comprometida é descomprometida
- **THEN** os frames físicos que a sustentavam voltam a estar disponíveis para outras alocações

#### Scenario: Acesso após descomprometimento falha até novo comprometimento
- **WHEN** uma sub-região é descomprometida e, em seguida, ocorre um acesso a um endereço dentro dela
- **THEN** o acesso SHALL falhar, exatamente como antes de qualquer comprometimento, até que essa sub-região seja comprometida novamente

### Requirement: Alteração de Proteção de uma Sub-região Comprometida
O kernel SHALL permitir alterar as permissões de acesso de uma sub-região já comprometida, sem alterar seu conteúdo nem os frames físicos que a sustentam.

#### Scenario: Escrita rejeitada após remoção da permissão de escrita
- **WHEN** a permissão de escrita de uma sub-região comprometida é removida
- **THEN** uma escrita subsequente a essa sub-região SHALL falhar, enquanto leituras continuam retornando o conteúdo previamente escrito, inalterado

#### Scenario: Alterar proteção de uma sub-região não comprometida é rejeitada
- **WHEN** é solicitada a alteração de proteção de uma sub-região que não está comprometida (dentro ou fora dos limites de alguma reserva)
- **THEN** a operação SHALL ser rejeitada em vez de comprometer memória nova silenciosamente

### Requirement: Liberação de uma Reserva
O kernel SHALL permitir liberar uma faixa de memória virtual previamente reservada por completo, descomprometendo qualquer sub-região ainda comprometida e tornando toda a faixa disponível para reservas futuras.

#### Scenario: Liberar descompromete tudo que ainda estava comprometido
- **WHEN** uma reserva com sub-regiões ainda comprometidas é liberada
- **THEN** todos os frames físicos que sustentavam essas sub-regiões voltam a estar disponíveis para outras alocações

#### Scenario: Acesso a uma reserva liberada falha
- **WHEN** uma reserva é liberada e, em seguida, ocorre um acesso a um endereço que pertencia a ela
- **THEN** o acesso SHALL falhar, mesmo que esse endereço tivesse sido comprometido antes da liberação

### Requirement: Reaproveitamento de Faixas Virtuais Liberadas
O espaço de endereço virtual de uma reserva liberada SHALL voltar a ficar disponível para reservas futuras, em vez de permanecer permanentemente indisponível.

#### Scenario: Reserva após liberação reaproveita o espaço virtual
- **WHEN** uma faixa é reservada, liberada, e uma nova faixa de tamanho igual ou menor é reservada em seguida
- **THEN** a nova reserva SHALL ser bem-sucedida, mesmo que o espaço virtual total disponível para reservas sem reaproveitamento não fosse suficiente para ambas simultaneamente
