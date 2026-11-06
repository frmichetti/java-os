# kernel-scheduler Specification

## Purpose

Escalona preemptivamente as threads do kernel entre si, usando o timer periódico já existente como gatilho, e define o comportamento do kernel quando nenhuma thread está pronta para executar.

## Requirements

### Requirement: Escalonamento Preemptivo Round-Robin
O escalonador SHALL interromper a thread em execução periodicamente (a cada tick do timer) e retomar a próxima thread pronta, em ordem round-robin, sem exigir que a thread em execução ceda o controle voluntariamente.

#### Scenario: Uma thread que nunca cede o controle ainda é interrompida
- **WHEN** uma thread em execução nunca cede a CPU voluntariamente (ex.: está em um laço infinito de CPU)
- **THEN** o escalonador SHALL, ainda assim, interrompê-la no próximo tick do timer e retomar outra thread pronta

#### Scenario: Todas as threads prontas recebem tempo de CPU
- **WHEN** existem múltiplas threads prontas para executar
- **THEN** o escalonador SHALL, ao longo do tempo, dar oportunidade de execução a cada uma delas, em vez de executar sempre a mesma

### Requirement: Troca de Contexto Preserva Estado da Thread
Ao trocar de uma thread para outra, o escalonador SHALL salvar o estado de registradores da CPU da thread interrompida — incluindo registradores de uso geral e estado de FPU/SSE/XMM — e restaurar exatamente o estado salvo da thread retomada, de forma que cada thread continue sua execução exatamente de onde parou, inclusive quanto a cálculos de ponto flutuante em andamento.

#### Scenario: Thread retomada continua com seus valores de registradores/stack anteriores
- **WHEN** uma thread é interrompida e, mais tarde, retomada pelo escalonador
- **THEN** seus registradores de uso geral e seu ponteiro de stack SHALL refletir exatamente o estado que tinham no momento em que foi interrompida

#### Scenario: Thread retomada continua com seu estado de ponto flutuante anterior
- **WHEN** uma thread com um cálculo de ponto flutuante em andamento (registradores de FPU/SSE/XMM com valores não-triviais) é interrompida e, mais tarde, retomada pelo escalonador
- **THEN** seus registradores de FPU/SSE/XMM SHALL refletir exatamente o estado que tinham no momento em que foi interrompida, sem nenhuma mistura com o estado de FPU/SSE/XMM de qualquer outra thread que tenha executado nesse intervalo

### Requirement: Comportamento Ocioso sem Threads Prontas
Quando nenhuma thread está pronta para executar, o kernel SHALL entrar em um estado ocioso que aguarda a próxima interrupção, em vez de ocupar a CPU em busca ativa (busy-wait) ou falhar.

#### Scenario: Nenhuma thread pronta não trava nem ocupa a CPU em busy-wait
- **WHEN** o escalonador não encontra nenhuma thread pronta para executar
- **THEN** o kernel SHALL aguardar em um estado ocioso que responde normalmente à próxima interrupção (ex.: o próximo tick do timer), em vez de girar em busy-wait ou parar de responder

### Requirement: Stack de Kernel Confiável Sempre Atualizada para a Thread Corrente
A cada troca de contexto, o escalonador SHALL garantir que a stack de kernel usada pela CPU em qualquer transição de privilégio (interrupção ou chamada de sistema) seja a stack de kernel da thread que passou a ser a corrente.

#### Scenario: Interrupção durante execução em modo usuário usa a stack de kernel correta
- **WHEN** uma thread em modo usuário é interrompida (ex.: pelo timer) ou faz uma chamada de sistema
- **THEN** a CPU SHALL passar a executar sobre a stack de kernel dessa mesma thread, nunca sobre a stack de kernel de outra thread

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
