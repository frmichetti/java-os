# Spec Delta

## ADDED Requirements

### Requirement: Bloqueio de Thread com Prazo
O kernel SHALL permitir que uma thread bloqueie numa fila de espera com um prazo
máximo (em tempo monotônico), sendo despertada por uma notificação explícita ou pela
expiração do prazo, o que ocorrer primeiro — nunca pelas duas causas, nunca por
nenhuma delas.

#### Scenario: Notificação antes do prazo expirar desperta a thread por notificação
- **WHEN** uma thread bloqueia com um prazo numa fila de espera, e é notificada antes
  desse prazo expirar
- **THEN** a thread SHALL retomar a execução por causa da notificação, não por
  expiração do prazo

#### Scenario: Prazo expirando sem notificação desperta a thread por expiração
- **WHEN** uma thread bloqueia com um prazo numa fila de espera, e nenhuma
  notificação a alcança antes desse prazo expirar
- **THEN** a thread SHALL retomar a execução automaticamente, assim que o prazo
  expirar, sem precisar de nenhuma notificação explícita

#### Scenario: Thread despertada sabe distinguir notificação de expiração de prazo
- **WHEN** uma thread bloqueada com prazo retoma a execução
- **THEN** ela SHALL conseguir determinar se retomou por ter sido notificada ou por o
  prazo ter expirado

#### Scenario: Um prazo expirado de uma espera já concluída não afeta uma espera futura
- **WHEN** uma thread é notificada antes de um prazo expirar, e em seguida inicia uma
  nova espera (com ou sem prazo) antes do prazo antigo chegar a expirar
- **THEN** a expiração do prazo antigo SHALL NOT afetar essa nova espera de forma
  alguma — nem acordando-a prematuramente, nem marcando-a como expirada por engano
