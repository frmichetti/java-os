# Spec Delta

## MODIFIED Requirements

### Requirement: Recursos Fecháveis de um Processo São Liberados ao Terminar
Quando um processo termina (voluntariamente ou por exceção de CPU em modo
usuário), o kernel SHALL fechar todo arquivo aberto e liberar toda reserva de
memória virtual anônima que esse processo possuía.

#### Scenario: Arquivos abertos de um processo terminado são fechados
- **WHEN** um processo com um ou mais arquivos abertos termina
- **THEN** cada um desses arquivos SHALL ser fechado como se o próprio processo
  tivesse fechado cada um antes de terminar

#### Scenario: Reservas de memória virtual de um processo terminado são liberadas
- **WHEN** um processo com uma ou mais reservas de memória virtual anônima
  termina
- **THEN** cada uma dessas reservas SHALL ser liberada como se o próprio processo
  tivesse liberado cada uma antes de terminar

#### Scenario: Processo com múltiplas threads só libera recursos quando a última termina
- **WHEN** uma thread de um processo com mais de uma thread viva termina,
  havendo ainda pelo menos outra thread viva nesse mesmo processo
- **THEN** nenhum arquivo aberto nem reserva de memória virtual pertencente
  a esse processo SHALL ser fechado ou liberado — eles permanecem
  utilizáveis pela thread que continua viva, exatamente como antes do
  término

#### Scenario: Última thread de um processo com múltiplas threads libera os recursos do processo
- **WHEN** a última thread viva de um processo que já teve mais de uma
  thread termina
- **THEN** o kernel SHALL fechar todo arquivo aberto e liberar toda reserva
  de memória virtual que esse processo possuía, exatamente como no caso de
  um processo de uma única thread
