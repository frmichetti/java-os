# Spec Delta

## Purpose

Define o comportamento real (não mais ausente) de `PlatformEvent`/`PlatformParker`,
o par de primitivas de sincronização por-thread (base de `park`/`unpark`) da camada
`os`/`os_cpu` do porte `os_javaos` do HotSpot.

## ADDED Requirements

### Requirement: `PlatformEvent` Funciona como Semáforo Binário Real
`PlatformEvent` SHALL se comportar como um semáforo binário real de três estados
(sinalizado, neutro, com esperador bloqueado): `park()` SHALL retornar imediatamente
se já havia uma sinalização pendente, e SHALL bloquear a thread chamadora até
`unpark()` caso contrário; `unpark()` SHALL sempre deixar o evento sinalizado,
despertando a thread bloqueada em `park()` se houver uma.

#### Scenario: `unpark()` antes de `park()` não bloqueia a thread chamadora
- **WHEN** `unpark()` é chamado sobre um `PlatformEvent`, e só depois `park()` é
  chamado sobre o mesmo evento, sem nenhuma outra sinalização entre as duas chamadas
- **THEN** `park()` SHALL retornar imediatamente, sem bloquear a thread chamadora

#### Scenario: `park()` sem sinalização pendente bloqueia até `unpark()`
- **WHEN** `park()` é chamado sobre um `PlatformEvent` sem nenhuma sinalização
  pendente
- **THEN** a thread chamadora SHALL bloquear até outra execução chamar `unpark()`
  sobre o mesmo evento, retomando então

#### Scenario: Uma sinalização só desbloqueia um `park()`
- **WHEN** `unpark()` é chamado uma única vez sobre um `PlatformEvent` com uma thread
  bloqueada em `park()`
- **THEN** essa thread SHALL retomar, e uma chamada a `park()` seguinte (sem nova
  sinalização) SHALL bloquear normalmente de novo, em vez de retornar imediatamente

### Requirement: `PlatformEvent::park` com Prazo Expira sem Sinalização
`PlatformEvent` SHALL fornecer uma variante de `park` que aceita um prazo máximo
(relativo, em milissegundos, ou em nanossegundos), retornando indicando sucesso se
sinalizada antes do prazo, ou indicando expiração se o prazo decorrer primeiro.

#### Scenario: `park` com prazo sinalizado antes de expirar retorna indicando sucesso
- **WHEN** `park` com um prazo é chamado sobre um `PlatformEvent`, e `unpark()` é
  chamado sobre o mesmo evento antes desse prazo expirar
- **THEN** a chamada SHALL retornar indicando sucesso, antes do prazo decorrer por
  completo

#### Scenario: `park` com prazo sem sinalização expira após o prazo decorrer
- **WHEN** `park` com um prazo é chamado sobre um `PlatformEvent` sem nenhuma
  sinalização, e nenhuma chamada a `unpark()` alcança o evento antes desse prazo
- **THEN** a chamada SHALL retornar indicando expiração do prazo, somente depois do
  prazo ter decorrido (dentro da granularidade do temporizador do kernel)

### Requirement: `PlatformParker` Inicializa e Libera Seus Recursos Reais
`PlatformParker` SHALL inicializar, na construção, um mutex real e dois objetos de
variável de condição reais (um para espera relativa, outro para espera absoluta) — e
SHALL liberar os três de volta ao sistema na destruição.

#### Scenario: Construção de um `PlatformParker` produz recursos utilizáveis
- **WHEN** um `PlatformParker` é construído
- **THEN** seu mutex e suas duas variáveis de condição internas SHALL estar prontos
  para uso imediato, sem exigir nenhuma inicialização adicional por quem o construiu

#### Scenario: Destruição de um `PlatformParker` libera seus recursos
- **WHEN** um `PlatformParker` é destruído
- **THEN** seu mutex e suas duas variáveis de condição internas SHALL ser liberados de
  volta ao sistema, sem vazamento
