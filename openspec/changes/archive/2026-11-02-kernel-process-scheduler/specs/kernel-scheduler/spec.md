# Spec Delta

## Purpose

Escalona preemptivamente as threads do kernel entre si, usando o timer periódico já existente como gatilho, e define o comportamento do kernel quando nenhuma thread está pronta para executar.

## ADDED Requirements

### Requirement: Escalonamento Preemptivo Round-Robin
O escalonador SHALL interromper a thread em execução periodicamente (a cada tick do timer) e retomar a próxima thread pronta, em ordem round-robin, sem exigir que a thread em execução ceda o controle voluntariamente.

#### Scenario: Uma thread que nunca cede o controle ainda é interrompida
- **WHEN** uma thread em execução nunca cede a CPU voluntariamente (ex.: está em um laço infinito de CPU)
- **THEN** o escalonador SHALL, ainda assim, interrompê-la no próximo tick do timer e retomar outra thread pronta

#### Scenario: Todas as threads prontas recebem tempo de CPU
- **WHEN** existem múltiplas threads prontas para executar
- **THEN** o escalonador SHALL, ao longo do tempo, dar oportunidade de execução a cada uma delas, em vez de executar sempre a mesma

### Requirement: Troca de Contexto Preserva Estado da Thread
Ao trocar de uma thread para outra, o escalonador SHALL salvar o estado de registradores da CPU da thread interrompida e restaurar exatamente o estado salvo da thread retomada, de forma que cada thread continue sua execução exatamente de onde parou.

#### Scenario: Thread retomada continua com seus valores de registradores/stack anteriores
- **WHEN** uma thread é interrompida e, mais tarde, retomada pelo escalonador
- **THEN** seus registradores de uso geral e seu ponteiro de stack SHALL refletir exatamente o estado que tinham no momento em que foi interrompida

### Requirement: Comportamento Ocioso sem Threads Prontas
Quando nenhuma thread está pronta para executar, o kernel SHALL entrar em um estado ocioso que aguarda a próxima interrupção, em vez de ocupar a CPU em busca ativa (busy-wait) ou falhar.

#### Scenario: Nenhuma thread pronta não trava nem ocupa a CPU em busy-wait
- **WHEN** o escalonador não encontra nenhuma thread pronta para executar
- **THEN** o kernel SHALL aguardar em um estado ocioso que responde normalmente à próxima interrupção (ex.: o próximo tick do timer), em vez de girar em busy-wait ou parar de responder
