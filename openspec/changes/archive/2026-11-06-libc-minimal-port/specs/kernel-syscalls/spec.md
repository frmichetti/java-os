# Spec Delta

## ADDED Requirements

### Requirement: Chamada de Término de Processo/Thread
O kernel SHALL fornecer uma chamada de sistema que termina a thread chamadora, removendo-a permanentemente da rotação do escalonador, de forma que ela nunca mais receba tempo de CPU.

#### Scenario: Thread terminada nunca mais é escalonada
- **WHEN** um processo em modo usuário invoca a chamada de término
- **THEN** a thread chamadora SHALL deixar de competir pelo escalonamento, e nenhuma execução futura SHALL retomá-la

#### Scenario: Outras threads continuam sendo escalonadas normalmente após uma terminar
- **WHEN** uma thread termina via a chamada de término, havendo outras threads prontas
- **THEN** o escalonador SHALL continuar dando tempo de CPU normalmente às threads restantes, sem travar nem pular nenhuma delas
