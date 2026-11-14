# Proposal

## Why

O esqueleto do porte `os_javaos` do HotSpot (`os-javaos-skeleton`) já declara
`PlatformEvent`/`PlatformParker` (`park_javaos.hpp`) — a última dupla de primitivas
de sincronização de baixo nível que o HotSpot usa (suporte por-thread a
`park`/`unpark`, base do JSR166 `java.util.concurrent`). Hoje elas não têm nenhum
corpo de método implementado em lugar nenhum da árvore vendorizada: compilam só
porque nada ainda as referencia de verdade. Agora que `kernel-timed-wait` entrega uma
primitiva real de espera com prazo no kernel, e `os-javaos-platform-sync` já provou o
padrão de implementar uma classe `Platform*` sobre `pthread_mutex_*`/`pthread_cond_*`
reais, esta é a peça que falta para fechar a base de sincronização que o HotSpot
precisa antes do próprio runtime (GC/JIT/interpretador) poder ser examinado.

## What Changes

- Implementar `PlatformEvent::PlatformEvent()`/`park()`/`park(jlong millis)`/
  `park_nanos(jlong nanos)`/`unpark()` de verdade, em um novo arquivo
  `park_javaos.cpp` — mesmo protocolo de semáforo binário de 3 estados (`_event`:
  `-1`/`0`/`1`) do porte POSIX real já vendorizado (`os/posix/os_posix.cpp`), sobre
  `pthread_mutex_t`/`pthread_cond_t` reais (a mesma libc de `os-javaos-platform-sync`)
  e `pthread_cond_timedwait` real (`kernel-timed-wait`) para o caso com prazo.
- Implementar `PlatformParker::PlatformParker()`/`~PlatformParker()` de verdade, no
  mesmo arquivo — inicializa/destrói o mutex e os dois `pthread_cond_t` (`REL_INDEX`/
  `ABS_INDEX`) de verdade.
- **Não muda `park_javaos.hpp`** — as assinaturas já declaradas pelo esqueleto
  (`os-javaos-skeleton`) permanecem exatamente como estão; esta change só escreve os
  corpos que faltam.
- **Fora de escopo, explícito**: os métodos da classe **compartilhada** `Parker`
  (`Parker::park(bool isAbsolute, jlong time)`/`Parker::unpark()`, declarados em
  `share/runtime/park.hpp`, com corpo esperado em `os_<os>.cpp`) **não** são
  implementados nesta change — ver "Impact" abaixo para o motivo.

## Capabilities

### New Capabilities
- `os-javaos-park-sync`: comportamento real de `PlatformEvent`/`PlatformParker` (o
  par de primitivas de sincronização por-thread do porte `os_javaos`, base de
  `park`/`unpark`) sobre `pthread_mutex_*`/`pthread_cond_*`/`pthread_cond_timedwait`
  reais da libc — mesma natureza de capability que `os-javaos-platform-sync`
  (`PlatformMutex`/`PlatformMonitor`), para a dupla de classes seguinte do mesmo
  arquivo de esqueleto (`park_javaos.hpp`).

### Modified Capabilities
(nenhuma — `os-javaos-platform-sync` cobre só `PlatformMutex`/`PlatformMonitor`, uma
dupla de classes distinta; `PlatformEvent`/`PlatformParker` são uma capacidade nova,
não uma extensão da existente)

## Impact

- **Código afetado**: `vendor/jdk-27-ga/src/hotspot/os/javaos/park_javaos.cpp` (novo
  arquivo); nenhuma mudança em `park_javaos.hpp`, `mutex_javaos.{hpp,cpp}`, ou em
  qualquer código do kernel/libc já entregue (`kernel-timed-wait`,
  `os-javaos-platform-sync`) — puramente consumidor do que já existe.
- **Verificação**: a mesma disciplina de `g++ -fsyntax-only -std=gnu++17` contra a
  árvore real vendorizada já usada por `os-javaos-memory-pd`/`os-javaos-platform-sync`
  (`PlatformEvent`/`PlatformParker` não são tipos `pthread_*` nem têm wrapper C em
  `userland_c/glue.c` — ao contrário de `PlatformMutex`/`PlatformMonitor`, não há como
  testá-las em boot real através de um programa C comum). **Achado de pesquisa desta
  proposta**: como os métodos de `PlatformEvent` não dependem de nenhuma
  infraestrutura de `JavaThread`/safepoint (confirmado lendo o corpo real vendorizado
  em `os/posix/os_posix.cpp` — usam só `_event`/`_nParked`/`_mutex`/`_cond`,
  `AtomicAccess`, `OrderAccess::fence()`), um programa C++ de teste dedicado,
  instanciando um `PlatformEvent` isolado diretamente (sem passar por `ParkEvent`/
  `Thread`), é tecnicamente possível — mas incluir os cabeçalhos reais do HotSpot
  (`memory/allocation.hpp` etc.) num executável `-ffreestanding -nostdlib` de
  userland é uma extensão de escopo própria (o toolchain cruzado deste projeto nunca
  compilou um `.cpp` que inclui a árvore do HotSpot — só os arquivos do próprio
  HotSpot são compilados com o g++ do host). Fica como decisão de design (ver
  `design.md`), não resolvida aqui na proposta.
- **Por que `Parker::park`/`unpark` ficam de fora**: dependem de `JavaThread::current()`,
  `ThreadBlockInVM`, `OSThreadWaitState` (ver corpo real em `os/posix/os_posix.cpp`,
  linhas ~1894-1989) — infraestrutura de estado de thread e integração com safepoint
  que este porte (`os_javaos`) nunca referenciou em nenhuma change anterior
  (confirmado: `grep -rl JavaThread vendor/.../os/javaos/` não encontra nada). Exigir
  isso agora seria uma expansão de escopo muito além de "mais uma dupla de classes
  `Platform*`" — é o próprio início da integração do runtime/estado de thread do
  HotSpot, melhor tratado como seu próprio marco (`ROADMAP.md`, item "Isolamento de
  falha por processo" e além), não uma sub-tarefa incidental desta change.
