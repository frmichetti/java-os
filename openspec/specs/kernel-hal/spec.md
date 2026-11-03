# kernel-hal Specification

## Purpose

Fornece a primeira abstração da camada de HAL (hardware abstraction layer) do kernel: um trait para saída de texto, independente do dispositivo físico que a implementa, para que diagnósticos de boot (e drivers futuros) não precisem conhecer qual console específico está em uso.

## Requirements

### Requirement: Abstração de Console Independente de Dispositivo
O kernel SHALL fornecer uma interface comum para escrita de texto em um console, de forma que o mesmo código de diagnóstico funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do dispositivo físico.

#### Scenario: Mesmo texto escrito através de dispositivos diferentes produz a mesma saída lógica
- **WHEN** a mesma sequência de texto é escrita através da interface comum em dois dispositivos diferentes que a implementam
- **THEN** ambos SHALL produzir a mesma saída lógica (o mesmo conteúdo textual), cada um em seu próprio formato de exibição

### Requirement: Abstração de Dispositivo de Teclado Independente de Hardware
O kernel SHALL fornecer uma interface comum para obter eventos de tecla, de forma que código consumidor de entrada funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do dispositivo físico.

#### Scenario: Consultar eventos de tecla não exige conhecer o dispositivo físico
- **WHEN** código do kernel consulta eventos de tecla através da interface comum
- **THEN** essa consulta SHALL funcionar independentemente de qual dispositivo de teclado físico a implementa

### Requirement: Abstração de Dispositivo de Armazenamento em Blocos Independente de Hardware
O kernel SHALL fornecer uma interface comum para ler e escrever blocos de um dispositivo de armazenamento por índice (LBA), de forma que código consumidor funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do controlador físico.

#### Scenario: Ler e escrever blocos não exige conhecer o controlador físico
- **WHEN** código do kernel lê ou escreve um bloco através da interface comum
- **THEN** essa operação SHALL funcionar independentemente de qual controlador de armazenamento físico a implementa

### Requirement: Abstração de Dispositivo de Rede Independente de Hardware
O kernel SHALL fornecer uma interface comum para enviar e consultar quadros Ethernet de um dispositivo de rede, de forma que código consumidor funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do controlador físico.

#### Scenario: Enviar e consultar quadros não exige conhecer o controlador físico
- **WHEN** código do kernel envia ou consulta um quadro através da interface comum
- **THEN** essa operação SHALL funcionar independentemente de qual controlador de rede físico a implementa
