# kernel-paging Specification

## Purpose

Permite ao kernel mapear e desmapear páginas virtuais de 4 KiB para frames físicos arbitrários, com controle de permissões, sobre a estrutura de paginação de 4 níveis do x86_64 já ativa quando o bootloader entrega o controle ao kernel.

## Requirements

### Requirement: Mapeamento de Página Virtual
O kernel SHALL permitir mapear uma página virtual de 4 KiB para um frame físico arbitrário, especificando as permissões de acesso (leitura/escrita, execução, nível de privilégio).

#### Scenario: Página mapeada se torna acessível com as permissões especificadas
- **WHEN** uma página virtual é mapeada para um frame físico com um conjunto de permissões
- **THEN** acessos a essa página virtual respeitam exatamente as permissões especificadas (ex.: uma escrita é permitida apenas se a permissão de escrita foi concedida)

#### Scenario: Mapear um endereço já mapeado sem solicitar substituição é rejeitado
- **WHEN** é solicitado o mapeamento de uma página virtual que já está mapeada, sem indicar explicitamente que o mapeamento existente deve ser substituído
- **THEN** a operação SHALL ser rejeitada em vez de sobrescrever o mapeamento existente silenciosamente

### Requirement: Desmapeamento de Página Virtual
O kernel SHALL permitir desmapear uma página virtual previamente mapeada, tornando esse endereço virtual inacessível novamente.

#### Scenario: Acesso a uma página desmapeada não é silenciosamente permitido
- **WHEN** uma página virtual previamente mapeada é desmapeada e, em seguida, ocorre um acesso a esse endereço
- **THEN** o acesso SHALL falhar (ex.: page fault) em vez de retornar dados obsoletos ou permitir o acesso

### Requirement: Preservação do Mapeamento do Kernel
Operações de mapeamento e desmapeamento de páginas de aplicação SHALL NOT remover ou corromper o mapeamento higher-half do próprio código e dados do kernel estabelecido no boot.

#### Scenario: Mapear/desmapear páginas de aplicação não afeta o próprio kernel
- **WHEN** páginas virtuais fora do espaço de endereçamento do kernel são mapeadas ou desmapeadas
- **THEN** o código e os dados do kernel continuam acessíveis e executáveis exatamente como antes dessas operações
