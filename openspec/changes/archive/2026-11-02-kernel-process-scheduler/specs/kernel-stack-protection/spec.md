# Spec Delta

## ADDED Requirements

### Requirement: Toda Stack de Thread Alocada Dinamicamente Tem Guard Page
Além da stack principal do kernel já coberta por esta capability, toda stack de kernel criada dinamicamente para uma nova thread SHALL ser mapeada com a mesma proteção de guard page (página imediatamente abaixo do seu limite inferior deliberadamente não mapeada).

#### Scenario: Stack de uma nova thread tem guard page própria
- **WHEN** uma nova thread é criada e recebe sua própria stack de kernel
- **THEN** a página imediatamente abaixo do limite inferior dessa stack SHALL não estar mapeada para nenhum frame físico, exatamente como já exigido para a stack principal do kernel

#### Scenario: Guard pages de threads distintas não se confundem
- **WHEN** duas threads distintas têm suas stacks mapeadas
- **THEN** a guard page de uma thread SHALL NOT coincidir com nenhuma página mapeada (stack ou dado) de qualquer outra thread
