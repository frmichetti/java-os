# Spec Delta

## ADDED Requirements

### Requirement: Consulta do Próprio Identificador de Thread via `pthread_self`
Uma thread SHALL poder consultar seu próprio identificador nativo (`pthread_t`)
através da função padrão `pthread_self`, a partir de qualquer ponto de sua
execução, sem depender de nenhum valor produzido no momento em que foi
criada.

#### Scenario: Thread criada por `pthread_create` consulta seu próprio identificador
- **WHEN** uma thread criada por `pthread_create` chama `pthread_self` a
  partir de qualquer ponto de sua própria execução
- **THEN** o identificador devolvido SHALL ser exatamente o mesmo
  `pthread_t` que a thread criadora recebeu de volta de `pthread_create`
  para essa mesma thread

#### Scenario: Duas threads distintas consultam identificadores diferentes
- **WHEN** duas threads distintas do mesmo processo chamam `pthread_self`,
  cada uma a partir de si mesma
- **THEN** cada uma SHALL receber o identificador que corresponde a si
  mesma, nunca o identificador da outra
