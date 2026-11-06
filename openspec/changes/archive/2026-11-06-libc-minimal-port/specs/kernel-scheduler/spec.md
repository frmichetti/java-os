# Spec Delta

## ADDED Requirements

### Requirement: Remoção Permanente de Thread Terminada da Rotação
Uma vez que uma thread é marcada como terminada, o escalonador SHALL excluí-la permanentemente da rotação round-robin, sem jamais escolhê-la novamente, mesmo que seu identificador ainda exista em estruturas internas do kernel.

#### Scenario: Thread terminada é removida da fila de prontos na sua próxima vez
- **WHEN** o escalonador está prestes a devolver a uma thread marcada como terminada mais uma fatia de tempo
- **THEN** o escalonador SHALL, em vez disso, excluí-la da rotação e escolher a próxima thread pronta

#### Scenario: Escalonamento continua funcionando normalmente com apenas threads não-terminadas
- **WHEN** uma ou mais threads já terminaram e outras continuam prontas
- **THEN** o escalonamento round-robin SHALL continuar alternando normalmente só entre as threads não-terminadas

#### Scenario: Todas as threads terminadas não deixa nenhuma pronta
- **WHEN** todas as threads da rotação terminaram
- **THEN** o kernel SHALL se comportar como no caso de nenhuma thread pronta (comportamento ocioso já especificado), em vez de falhar ou escalonar uma thread terminada
