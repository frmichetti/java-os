# Spec Delta

## Purpose

Permite a um programa C de modo usuário do Java OS criar e encerrar uma thread POSIX real, com um argumento passado à função da nova thread, através das funções padrão `pthread_create`/`pthread_exit`, implementadas sobre a chamada de sistema de criação de thread que o kernel já expõe.

## ADDED Requirements

### Requirement: Criação Real de Thread via `pthread_create`
Um programa C de modo usuário SHALL poder criar uma nova thread através da
função padrão `pthread_create`, informando uma função e um argumento, com
a nova thread executando essa função, recebendo esse argumento, de forma
concorrente com a thread chamadora.

#### Scenario: Thread criada executa a função informada com o argumento informado
- **WHEN** um programa C chama `pthread_create` informando uma função e um
  argumento
- **THEN** a nova thread SHALL chamar essa função, passando exatamente o
  argumento informado

#### Scenario: Thread criada compartilha memória com a thread chamadora
- **WHEN** uma nova thread é criada por `pthread_create`
- **THEN** ela SHALL enxergar e poder modificar qualquer memória já
  acessível à thread chamadora, exatamente como duas threads do mesmo
  processo

### Requirement: Encerramento de Thread via `pthread_exit`
Uma thread criada por `pthread_create` SHALL poder encerrar sua própria
execução através da função padrão `pthread_exit`, ou ao retornar
normalmente da função informada na criação, de forma que nenhuma
instrução sua volte a ser executada depois disso, sem afetar nenhuma
outra thread do mesmo processo ainda viva.

#### Scenario: Retornar da função informada encerra a thread
- **WHEN** a função informada a `pthread_create` retorna normalmente
- **THEN** a thread que a executava SHALL ser encerrada, sem nenhuma
  instrução sua voltar a ser executada depois disso

#### Scenario: Chamar pthread_exit encerra a thread chamadora
- **WHEN** uma thread criada por `pthread_create` chama `pthread_exit`
- **THEN** essa thread SHALL ser encerrada, sem nenhuma instrução sua
  voltar a ser executada depois disso

#### Scenario: Encerramento de uma thread não afeta outra ainda viva
- **WHEN** uma thread criada por `pthread_create` é encerrada, enquanto a
  thread que a criou (ou outra criada por ela) continua viva
- **THEN** a thread que continua viva SHALL continuar executando
  normalmente, sem ser afetada pelo encerramento
