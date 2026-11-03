# kernel-pci Specification

## Purpose

Permite ao kernel descobrir dispositivos PCI (e seus endereços de configuração, como BARs de MMIO) por enumeração do barramento, pré-requisito para qualquer driver de um dispositivo PCI (armazenamento, e futuramente rede).

## Requirements

### Requirement: Enumeração de Dispositivos PCI por Classe
O kernel SHALL ser capaz de enumerar os dispositivos PCI presentes e identificar, para cada um, sua classe, subclasse e interface de programação, de forma que um driver possa localizar o dispositivo correspondente à sua classe de hardware sem precisar conhecer de antemão em qual posição do barramento ele está.

#### Scenario: Dispositivo de uma classe conhecida é encontrado pela enumeração
- **WHEN** o kernel enumera os dispositivos PCI presentes e há um dispositivo de uma classe/subclasse/interface de programação específica conectado
- **THEN** essa enumeração SHALL identificar esse dispositivo, devolvendo informação suficiente para localizá-lo novamente (sua posição no barramento)

### Requirement: Endereços de Configuração de um Dispositivo PCI São Obteníveis
Para um dispositivo PCI já localizado pela enumeração, o kernel SHALL ser capaz de obter seus endereços base (BARs) registrados na configuração PCI, de forma que um driver consiga acessar os registradores do dispositivo.

#### Scenario: Endereço base de um dispositivo é obtido corretamente
- **WHEN** o kernel consulta o endereço base registrado na configuração PCI de um dispositivo já localizado
- **THEN** o kernel SHALL devolver o endereço correspondente ao que o próprio dispositivo reportou na sua configuração, sem valores inventados ou de outro dispositivo

### Requirement: Escrita de Configuração de um Dispositivo PCI Já Localizado
Para um dispositivo PCI já localizado pela enumeração, o kernel SHALL ser capaz de escrever um registrador de sua configuração, de forma que um driver consiga habilitar capacidades do dispositivo que dependem disso (ex.: Bus Mastering, necessário para DMA).

#### Scenario: Registrador escrito reflete o valor escrito numa leitura seguinte
- **WHEN** o kernel escreve um valor num registrador de configuração de um dispositivo PCI já localizado e, em seguida, lê esse mesmo registrador
- **THEN** o valor lido SHALL refletir a escrita realizada, não o valor anterior
