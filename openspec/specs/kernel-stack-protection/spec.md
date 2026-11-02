# kernel-stack-protection Specification

## Purpose

Fornece ao kernel uma stack de execução própria, protegida por uma guard page, para que um estouro de stack resulte em uma falha de página imediata e diagnosticável em vez de corromper silenciosamente memória adjacente.

## Requirements

### Requirement: Stack Dedicada do Kernel com Guard Page
O kernel SHALL rodar sobre uma stack própria (não a stack fornecida pelo bootloader), mapeada em uma faixa virtual reservada, com a página imediatamente abaixo do seu limite inferior deliberadamente não mapeada.

#### Scenario: Stack do kernel está mapeada e utilizável
- **WHEN** o kernel termina a troca para sua stack própria
- **THEN** a execução continua normalmente, com leitura e escrita na stack funcionando como esperado

#### Scenario: Página logo abaixo da stack não está mapeada
- **WHEN** o endereço virtual imediatamente abaixo do limite inferior da stack do kernel é consultado
- **THEN** esse endereço SHALL não estar mapeado para nenhum frame físico

### Requirement: Estouro de Stack Falha Imediatamente
Um estouro da stack do kernel (uso de mais memória de stack do que a região mapeada comporta) SHALL resultar em uma falha de página assim que a guard page for alcançada, em vez de corromper memória adjacente silenciosamente.

#### Scenario: Recursão profunda demais aciona a guard page rapidamente
- **WHEN** uma sequência de chamadas consome toda a stack mapeada do kernel e alcança a guard page
- **THEN** o kernel SHALL sofrer uma exceção de page fault referenciando um endereço dentro da guard page, antes de qualquer escrita bem-sucedida além do limite inferior da stack mapeada

### Requirement: Toda Stack de Thread Alocada Dinamicamente Tem Guard Page
Além da stack principal do kernel já coberta por esta capability, toda stack de kernel criada dinamicamente para uma nova thread SHALL ser mapeada com a mesma proteção de guard page (página imediatamente abaixo do seu limite inferior deliberadamente não mapeada).

#### Scenario: Stack de uma nova thread tem guard page própria
- **WHEN** uma nova thread é criada e recebe sua própria stack de kernel
- **THEN** a página imediatamente abaixo do limite inferior dessa stack SHALL não estar mapeada para nenhum frame físico, exatamente como já exigido para a stack principal do kernel

#### Scenario: Guard pages de threads distintas não se confundem
- **WHEN** duas threads distintas têm suas stacks mapeadas
- **THEN** a guard page de uma thread SHALL NOT coincidir com nenhuma página mapeada (stack ou dado) de qualquer outra thread
