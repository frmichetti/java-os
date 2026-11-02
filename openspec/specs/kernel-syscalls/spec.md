# kernel-syscalls Specification

## Purpose

Fornece a interface de chamada de sistema (`syscall`/`sysret`) pela qual código em modo usuário solicita serviços do kernel, com uma stack de kernel confiável, uma convenção de ABI explícita, e despacho por número de chamada.

## Requirements

### Requirement: Chamada de Sistema Troca para Stack de Kernel Confiável
Ao receber uma chamada de sistema, o kernel SHALL passar a executar sobre a stack de kernel da thread chamadora antes de interpretar qualquer dado vindo do modo usuário, em vez de continuar sobre a stack de usuário (não confiável) da thread chamadora.

#### Scenario: Chamada de sistema nunca processa dados sobre a stack de usuário
- **WHEN** uma thread em modo usuário executa a instrução de chamada de sistema
- **THEN** o handler da chamada SHALL executar inteiramente sobre a stack de kernel dessa thread, nunca sobre a stack de usuário dela

### Requirement: Retorno ao Modo Usuário Após a Chamada
Após atender uma chamada de sistema, o kernel SHALL retornar a execução ao modo usuário, no ponto exatamente seguinte à instrução de chamada, com o resultado da chamada disponível para o código chamador.

#### Scenario: Thread chamadora retoma a execução em modo usuário com o resultado
- **WHEN** o kernel termina de atender uma chamada de sistema
- **THEN** a thread chamadora SHALL retomar a execução em modo usuário imediatamente após a instrução de chamada, com o valor de retorno da chamada acessível a ela

### Requirement: Despacho por Número de Chamada
O kernel SHALL identificar qual serviço está sendo solicitado por um número de chamada explícito, e SHALL rejeitar de forma explícita um número de chamada não reconhecido, em vez de executar um serviço arbitrário ou travar.

#### Scenario: Número de chamada inválido é rejeitado explicitamente
- **WHEN** uma chamada de sistema é feita com um número de chamada que o kernel não reconhece
- **THEN** o kernel SHALL retornar um erro explícito para quem chamou, em vez de executar um serviço não intencional, travar, ou produzir comportamento indefinido

### Requirement: Chamada de Escrita no Console
O kernel SHALL fornecer uma chamada de sistema que escreve uma sequência de bytes fornecida pelo chamador no console de saída do kernel.

#### Scenario: Dados escritos pela chamada aparecem no console
- **WHEN** código em modo usuário invoca a chamada de escrita com uma sequência de bytes
- **THEN** essa sequência de bytes SHALL aparecer no console de saída do kernel

### Requirement: Chamada de Consulta do Contador de Ticks
O kernel SHALL fornecer uma chamada de sistema que devolve ao chamador o valor atual do contador de ticks do kernel.

#### Scenario: Valor devolvido reflete o contador real do kernel
- **WHEN** código em modo usuário invoca a chamada de consulta de ticks
- **THEN** o valor devolvido SHALL corresponder ao contador de ticks do kernel no momento da chamada
