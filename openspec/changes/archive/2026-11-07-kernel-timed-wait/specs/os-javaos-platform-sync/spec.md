# Spec Delta

## REMOVED Requirements

### Requirement: Espera Com Prazo Permanece Explicitamente Não Suportada
**Reason**: `kernel-timed-wait` entrega uma primitiva real de espera com prazo no
kernel (`WaitQueue`/`CondVar`) e a libc (`pthread_cond_timedwait`) — não é mais
verdade que nenhuma primitiva de tempo real exista para `PlatformMonitor::wait`
honrar. O comportamento de recusar explicitamente um prazo real pedido deixa de
existir; substituído pelo requisito "Espera Com Prazo Real via
`PlatformMonitor::wait`" abaixo.
**Migration**: Nenhuma ação necessária por quem chama `PlatformMonitor::wait(millis)`
— `millis != 0` passa a funcionar de verdade em vez de abortar o processo.

## ADDED Requirements

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
