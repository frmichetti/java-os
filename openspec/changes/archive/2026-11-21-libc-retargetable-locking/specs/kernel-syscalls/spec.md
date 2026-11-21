# Spec Delta

## ADDED Requirements

### Requirement: Chamada de Consulta da ThreadId Corrente
O kernel SHALL fornecer uma chamada de sistema que devolve ao chamador o
identificador da própria thread chamadora, distinto do de qualquer outra
thread já criada.

#### Scenario: Consulta devolve um identificador que distingue a thread chamadora
- **WHEN** um processo em modo usuário invoca a chamada de consulta da
  `ThreadId` corrente
- **THEN** o kernel devolve um identificador que corresponde exatamente à
  thread chamadora, distinto do identificador de qualquer outra thread já
  criada

#### Scenario: Duas threads do mesmo processo obtêm identificadores diferentes
- **WHEN** duas threads distintas do mesmo processo invocam a chamada de
  consulta da `ThreadId` corrente, cada uma a partir de si mesma
- **THEN** cada uma SHALL receber o identificador que corresponde a si
  mesma, nunca o identificador da outra
