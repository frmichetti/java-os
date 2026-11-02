# Spec Delta

## ADDED Requirements

### Requirement: Thread com Stack de Usuário Opcional
Além de sua stack de kernel, uma thread SHALL poder ter uma stack de usuário própria, mapeada como acessível por usuário, para ser usada quando essa thread executa em modo usuário.

#### Scenario: Thread de modo usuário tem stacks de kernel e de usuário distintas
- **WHEN** uma thread é criada para rodar em modo usuário
- **THEN** ela SHALL ter uma stack de kernel (não acessível por usuário) e uma stack de usuário (acessível por usuário) distintas uma da outra
