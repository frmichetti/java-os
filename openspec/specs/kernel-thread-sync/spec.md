# kernel-thread-sync Specification

## Purpose

Fornece exclusão mútua (`Mutex`) e variável de condição (`CondVar`) com bloqueio real de thread (sem busy-wait), construídas sobre a primitiva de bloqueio/despertar do escalonador — a base de `pthread_mutex`/`pthread_cond` POSIX, usados por `PlatformEvent`/`Parker` do HotSpot (`park`/`unpark`, `Object.wait`/`notify`, `java.util.concurrent`).

## Requirements

### Requirement: Exclusão Mútua com Bloqueio Real
O kernel SHALL fornecer um mutex que garanta que no máximo uma thread o detenha por vez, bloqueando (sem busy-wait) qualquer thread que tente adquiri-lo enquanto outra já o detém, até ele ser liberado.

#### Scenario: Acesso exclusivo impede atualização perdida sob disputa
- **WHEN** múltiplas threads incrementam repetidamente um mesmo contador, cada incremento protegido pelo mesmo mutex
- **THEN** o valor final do contador reflete exatamente a soma de todos os incrementos, sem nenhum perdido

#### Scenario: Thread bloqueada esperando o mutex não consome CPU em busy-wait
- **WHEN** uma thread tenta adquirir um mutex já detido por outra
- **THEN** ela sai da rotação do escalonador (sem busy-wait) até o mutex ser liberado, em vez de girar ativamente verificando seu estado

#### Scenario: Thread em espera adquire o mutex após liberação, mesmo sob disputa com a própria liberação
- **WHEN** um mutex é liberado exatamente enquanto outra thread está prestes a bloquear nele
- **THEN** essa thread SHALL, ainda assim, eventualmente adquirir o mutex, em vez de permanecer bloqueada indefinidamente (nenhum despertar perdido)

### Requirement: Variável de Condição Libera e Reaquire o Mutex Associado Atomicamente
O kernel SHALL fornecer uma variável de condição cuja operação de espera libere o mutex associado e bloqueie a thread como uma única operação atômica (sem janela em que o mutex pareça detido por uma thread que já está bloqueada), reaquirindo o mesmo mutex antes de retornar ao ser despertada.

#### Scenario: Mutex fica disponível para outras threads enquanto uma thread espera na condição
- **WHEN** uma thread chama a espera da variável de condição, detendo um mutex no momento da chamada
- **THEN** esse mutex SHALL ficar disponível para outras threads adquirirem enquanto a chamada ainda não retornou

#### Scenario: Thread volta a deter o mutex ao retornar da espera
- **WHEN** uma thread é despertada e a espera na variável de condição retorna
- **THEN** essa thread SHALL deter o mesmo mutex associado novamente, exatamente como antes de ter chamado a espera

### Requirement: Despertar de Threads em Espera de uma Variável de Condição
O kernel SHALL permitir notificar uma ou todas as threads atualmente esperando numa variável de condição.

#### Scenario: Notificar uma thread acorda só a que espera há mais tempo
- **WHEN** várias threads esperam na mesma variável de condição e ela é notificada para despertar uma
- **THEN** apenas a thread que começou a esperar há mais tempo entre elas retoma

#### Scenario: Notificar todas acorda cada thread em espera
- **WHEN** várias threads esperam na mesma variável de condição e ela é notificada para despertar todas
- **THEN** cada uma delas retoma eventualmente, cada uma reaquirindo o mutex associado em sua vez

### Requirement: Notificação sem Despertar Perdido ao Seguir a Convenção de Uso
Uma notificação enviada a uma variável de condição enquanto o chamador detém o mutex associado SHALL NOT ser perdida para nenhuma thread que já estivesse esperando naquela condição antes da notificação.

#### Scenario: Notificação concorrente com o início da espera não se perde
- **WHEN** uma thread notifica a variável de condição exatamente enquanto detém o mutex associado, e outra thread está no processo de começar a esperar na mesma condição (tendo liberado o mesmo mutex como parte dessa chamada)
- **THEN** a notificação SHALL alcançar essa thread (ela desperta), em vez de ser perdida por ela ainda não estar oficialmente registrada como em espera
