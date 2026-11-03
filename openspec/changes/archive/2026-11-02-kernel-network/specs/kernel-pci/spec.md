# Spec Delta

## ADDED Requirements

### Requirement: Escrita de Configuração de um Dispositivo PCI Já Localizado
Para um dispositivo PCI já localizado pela enumeração, o kernel SHALL ser capaz de escrever um registrador de sua configuração, de forma que um driver consiga habilitar capacidades do dispositivo que dependem disso (ex.: Bus Mastering, necessário para DMA).

#### Scenario: Registrador escrito reflete o valor escrito numa leitura seguinte
- **WHEN** o kernel escreve um valor num registrador de configuração de um dispositivo PCI já localizado e, em seguida, lê esse mesmo registrador
- **THEN** o valor lido SHALL refletir a escrita realizada, não o valor anterior
