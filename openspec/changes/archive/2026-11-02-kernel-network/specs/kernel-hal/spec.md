# Spec Delta

## ADDED Requirements

### Requirement: Abstração de Dispositivo de Rede Independente de Hardware
O kernel SHALL fornecer uma interface comum para enviar e consultar quadros Ethernet de um dispositivo de rede, de forma que código consumidor funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do controlador físico.

#### Scenario: Enviar e consultar quadros não exige conhecer o controlador físico
- **WHEN** código do kernel envia ou consulta um quadro através da interface comum
- **THEN** essa operação SHALL funcionar independentemente de qual controlador de rede físico a implementa
