# os-javaos-platform-sync Specification

## Purpose

Define o comportamento real (não mais ausente/stub) dos primitivos de sincronização
da camada `os`/`os_cpu` do porte HotSpot (`PlatformMutex`/`PlatformMonitor`) sobre as
funções `pthread_mutex_*`/`pthread_cond_*` reais da libc, incluindo o caso em que
nenhuma primitiva de kernel equivalente existe (espera com prazo).

## Requirements

### Requirement: Exclusão Mútua Real via `PlatformMutex`
`PlatformMutex` SHALL fornecer exclusão mútua real — construção, destruição,
aquisição bloqueante (`lock`), liberação (`unlock`) e tentativa de aquisição
não-bloqueante (`try_lock`) — apoiada nas funções `pthread_mutex_*` reais da libc.

#### Scenario: Construir, adquirir e liberar um PlatformMutex tem sucesso
- **WHEN** um `PlatformMutex` é construído e, em seguida, `lock()` e `unlock()` são
  chamados sobre ele
- **THEN** ambas as chamadas SHALL ter efeito real de exclusão mútua (um `lock()`
  concorrente só teria sucesso depois do `unlock()`)

#### Scenario: `try_lock` sobre um PlatformMutex livre tem sucesso imediato
- **WHEN** `try_lock()` é chamado sobre um `PlatformMutex` atualmente livre
- **THEN** a função SHALL devolver `true` imediatamente, e o mutex passa a estar
  tomado

#### Scenario: `try_lock` sobre um PlatformMutex já tomado falha sem bloquear
- **WHEN** `try_lock()` é chamado sobre um `PlatformMutex` já tomado pela própria
  thread chamadora
- **THEN** a função SHALL devolver `false` imediatamente, sem bloquear a thread
  chamadora

### Requirement: Notificação Real via `PlatformMonitor`
`PlatformMonitor` SHALL fornecer notificação real de uma thread (`notify`) ou de
todas as threads (`notify_all`) atualmente esperando nele, apoiada nas funções
`pthread_cond_*` reais da libc.

#### Scenario: Notificar um PlatformMonitor sem nenhum esperador tem sucesso
- **WHEN** `notify()` ou `notify_all()` é chamado sobre um `PlatformMonitor` sem
  nenhuma thread atualmente esperando nele
- **THEN** a chamada SHALL ter sucesso, sem efeito colateral observável

### Requirement: Espera Sem Prazo Real via `PlatformMonitor::wait`
`PlatformMonitor::wait(millis)` chamado com `millis == 0` (convenção do HotSpot para
"esperar indefinidamente, sem timeout") SHALL bloquear a thread chamadora de verdade
até ser notificada, apoiado em `pthread_cond_wait` real.

#### Scenario: `wait(0)` bloqueia até ser notificado
- **WHEN** `wait(0)` é chamado sobre um `PlatformMonitor`, e outra execução
  eventualmente o notifica
- **THEN** a chamada SHALL permanecer bloqueada até a notificação, e retornar
  normalmente depois dela

### Requirement: Espera Com Prazo Real via `PlatformMonitor::wait`
`PlatformMonitor::wait(millis)` chamado com `millis != 0` (um prazo real pedido)
SHALL bloquear a thread chamadora de verdade até ser notificada ou até esse prazo
expirar, o que ocorrer primeiro, apoiado em `pthread_cond_timedwait` real — em vez de
abortar o processo ou de ignorar o prazo pedido.

#### Scenario: `wait(millis)` com millis > 0 expira após o prazo sem notificação
- **WHEN** `wait(millis)` é chamado sobre um `PlatformMonitor` com `millis` maior que
  zero, e nenhuma execução o notifica antes desse prazo
- **THEN** a chamada SHALL retornar, indicando expiração do prazo, somente depois de
  `millis` terem decorrido (dentro da granularidade do temporizador do kernel)

#### Scenario: `wait(millis)` com millis > 0 notificado antes de expirar retorna com sucesso
- **WHEN** `wait(millis)` é chamado sobre um `PlatformMonitor` com `millis` maior que
  zero, e outra execução o notifica antes desse prazo expirar
- **THEN** a chamada SHALL retornar indicando sucesso (não expiração), sem esperar o
  restante do prazo pedido
