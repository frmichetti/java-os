# Spec Delta

## ADDED Requirements

### Requirement: Bloqueio Voluntário de Thread sem Busy-Wait
O escalonador SHALL permitir que a thread em execução saia voluntariamente da rotação de escalonamento (bloqueie) sem consumir CPU em busca ativa, até ser explicitamente retomada por outra thread ou por uma rotina de interrupção.

#### Scenario: Thread bloqueada não recebe tempo de CPU enquanto bloqueada
- **WHEN** uma thread bloqueia voluntariamente e nenhuma outra thread a desperta
- **THEN** o escalonador SHALL nunca escolhê-la para executar, dando tempo de CPU apenas às threads que continuam prontas

#### Scenario: Thread bloqueada continua de onde parou ao ser despertada
- **WHEN** uma thread bloqueada é despertada
- **THEN** ela SHALL voltar a competir normalmente pelo escalonamento round-robin e, quando escolhida, continuar sua execução exatamente do ponto em que bloqueou, com seus registradores e stack preservados

### Requirement: Despertar de Thread Bloqueada a Partir de uma Rotina de Interrupção
O escalonador SHALL permitir que uma rotina de tratamento de interrupção (ISR) desperte uma thread bloqueada, sem causar impasse (deadlock) com a thread que ela interrompeu.

#### Scenario: Uma ISR desperta uma thread bloqueada com segurança
- **WHEN** uma rotina de tratamento de interrupção desperta uma thread que está bloqueada
- **THEN** essa thread SHALL passar a competir pelo escalonamento normalmente, sem que a ISR trave esperando um recurso que a própria thread interrompida possa já deter
