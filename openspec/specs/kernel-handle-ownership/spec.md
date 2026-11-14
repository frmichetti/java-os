# kernel-handle-ownership Specification

## Purpose

Garante que cada handle de recurso nomeado — mutex/condvar nomeado, arquivo
aberto, reserva de memória virtual anônima — pertence a um único processo (o que
o criou), que nenhum outro processo pode operar sobre ele, e que os recursos
fecháveis de um processo são liberados quando ele termina.

## Requirements

### Requirement: Handle Pertence ao Processo que o Criou
Todo handle de mutex/condvar nomeado, arquivo aberto, ou reserva de memória
virtual anônima SHALL ser associado ao processo que o criou, no momento da
criação.

#### Scenario: Handle recém-criado é utilizável pelo próprio processo que o criou
- **WHEN** um processo cria um mutex, condvar, arquivo aberto, ou reserva de
  memória virtual, e em seguida opera sobre o handle resultante
- **THEN** a operação SHALL ter sucesso, exatamente como se nenhum rastreamento de
  posse existisse

### Requirement: Handle Não É Acessível a Partir de Outro Processo
Um processo que não é dono de um handle de mutex/condvar nomeado, arquivo aberto,
ou reserva de memória virtual SHALL ser rejeitado ao tentar operar sobre ele —
com o mesmo resultado observável de um handle que nunca existiu.

#### Scenario: Operar sobre o mutex/condvar de outro processo é rejeitado
- **WHEN** um processo invoca uma operação de mutex ou condvar nomeados (ex.:
  travar, destravar, notificar, esperar) usando o `handle` de um mutex/condvar
  criado por outro processo
- **THEN** a chamada SHALL ser rejeitada com o mesmo sentinela de erro de um
  `handle` desconhecido, sem afetar o estado do mutex/condvar

#### Scenario: Ler ou fechar o arquivo aberto de outro processo é rejeitado
- **WHEN** um processo invoca uma leitura ou fechamento usando o `handle` de um
  arquivo aberto por outro processo
- **THEN** a chamada SHALL ser rejeitada com o mesmo sentinela de erro de um
  `handle` desconhecido, sem afetar o cursor nem o conteúdo desse arquivo

#### Scenario: Comprometer, descomprometer, proteger ou liberar a reserva de memória virtual de outro processo é rejeitado
- **WHEN** um processo invoca uma operação de comprometimento, descomprometimento,
  alteração de proteção, ou liberação sobre uma reserva de memória virtual
  pertencente a outro processo
- **THEN** a chamada SHALL ser rejeitada com o mesmo sentinela de erro de uma
  reserva desconhecida, sem afetar essa reserva

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
