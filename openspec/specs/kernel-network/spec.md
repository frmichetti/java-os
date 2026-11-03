# kernel-network Specification

## Purpose

Fornece ao kernel acesso de envio e recepção de quadros Ethernet brutos através de uma placa de rede, sem depender de nenhuma pilha de protocolo (ARP, IP, TCP/UDP, sockets).

## Requirements

### Requirement: Envio de um Quadro Ethernet é Aceito pela Placa
O kernel SHALL ser capaz de enviar um quadro Ethernet através da placa de rede, de forma que o conteúdo exato do quadro seja entregue ao meio físico (ou, no ambiente de teste, ao backend de rede emulado).

#### Scenario: Enviar um quadro de tamanho válido é aceito
- **WHEN** o kernel envia um quadro Ethernet de tamanho válido através da placa de rede
- **THEN** a placa SHALL aceitar o quadro para transmissão, em vez de rejeitá-lo ou descartá-lo silenciosamente

### Requirement: Quadros Recebidos Ficam Disponíveis para Consulta
Quando a placa de rede recebe um quadro Ethernet, o kernel SHALL disponibilizar esse quadro para consulta por outro código do kernel, com o mesmo conteúdo exato recebido.

#### Scenario: Quadro recebido é obtido com o conteúdo correto
- **WHEN** a placa de rede recebe um quadro Ethernet e, em algum momento depois, algum código do kernel consulta por quadros pendentes
- **THEN** esse quadro SHALL ser obtido nessa consulta, com o mesmo conteúdo exato que chegou à placa, não dados inventados ou de outro quadro

### Requirement: Endereço Físico (MAC) da Placa é Obtível
O kernel SHALL ser capaz de obter o endereço físico (MAC) da placa de rede, de forma que código consumidor saiba se identificar na rede.

#### Scenario: Endereço MAC obtido corresponde ao da placa
- **WHEN** o kernel consulta o endereço MAC da placa de rede já inicializada
- **THEN** o endereço devolvido SHALL ser exatamente o que a própria placa reporta, não um valor inventado
