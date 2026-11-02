# Spec Delta

## ADDED Requirements

### Requirement: Abstração de Dispositivo de Teclado Independente de Hardware
O kernel SHALL fornecer uma interface comum para obter eventos de tecla, de forma que código consumidor de entrada funcione sobre qualquer dispositivo que implemente essa interface, sem conhecer os detalhes do dispositivo físico.

#### Scenario: Consultar eventos de tecla não exige conhecer o dispositivo físico
- **WHEN** código do kernel consulta eventos de tecla através da interface comum
- **THEN** essa consulta SHALL funcionar independentemente de qual dispositivo de teclado físico a implementa
