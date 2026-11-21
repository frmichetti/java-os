# Spec Delta

## Purpose

Define o comportamento real de `os::create_main_thread`/`os::create_attached_thread` no porte `os_javaos` — anexar uma thread do sistema operacional já em execução (nunca criada via `os::create_thread`) a um `Thread`/`JavaThread`, condição necessária antes de qualquer outra parte da inicialização da VM poder prosseguir.

## ADDED Requirements

### Requirement: Anexação Real da Thread Primordial via `os::create_main_thread`
`os::create_main_thread` SHALL anexar a thread primordial (a thread do
sistema operacional já em execução no momento em que a inicialização da VM
começa, nunca criada via `os::create_thread`) ao `Thread` informado,
tornando-o pronto para uso pelo restante da inicialização da VM.

#### Scenario: Anexação bem-sucedida associa a thread do SO ao `Thread`
- **WHEN** `os::create_main_thread` é chamado com um `Thread` recém-alocado,
  a partir da própria thread primordial
- **THEN** esse `Thread` SHALL ficar associado à thread primordial, pronto
  para uso pelo restante da inicialização da VM, sem criar nenhuma nova
  thread do sistema operacional

### Requirement: Anexação Real de uma Thread Já em Execução via `os::create_attached_thread`
`os::create_attached_thread` SHALL anexar a thread do sistema operacional
já em execução que o chama (nunca criada via `os::create_thread`) ao
`Thread` informado, descobrindo o identificador nativo dessa thread sem
depender de nenhum valor produzido no momento da criação dela.

#### Scenario: Anexação bem-sucedida descobre o identificador nativo da thread chamadora
- **WHEN** `os::create_attached_thread` é chamado a partir de uma thread do
  sistema operacional já em execução, com um `Thread` recém-alocado
- **THEN** esse `Thread` SHALL ficar associado a essa thread, com o
  identificador nativo correto dela — descoberto consultando a própria
  thread chamadora, nunca reaproveitando um identificador de uma thread
  diferente
