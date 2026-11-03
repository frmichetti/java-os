# Spec Delta

## ADDED Requirements

### Requirement: Abstração de Dispositivo de Armazenamento em Blocos Independente de Hardware
O kernel SHALL fornecer uma interface comum para ler e escrever blocos de um dispositivo de armazenamento por índice (LBA), de forma que código consumidor funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do controlador físico.

#### Scenario: Ler e escrever blocos não exige conhecer o controlador físico
- **WHEN** código do kernel lê ou escreve um bloco através da interface comum
- **THEN** essa operação SHALL funcionar independentemente de qual controlador de armazenamento físico a implementa
