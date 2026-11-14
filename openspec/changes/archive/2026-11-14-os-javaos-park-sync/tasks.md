# Tasks

## 1. `PlatformEvent` real em `park_javaos.cpp`

- [x] 1.1 Criar `vendor/jdk-27-ga/src/hotspot/os/javaos/park_javaos.cpp` com
      `PlatformEvent::PlatformEvent()`: `pthread_cond_init`/`pthread_mutex_init`
      reais sobre `_cond[0]`/`_mutex[0]`, `_event = 0`, `_nParked = 0` — mesmo padrão
      de include (`runtime/os.hpp`, `utilities/debug.hpp`, `<errno.h>`, `<time.h>`,
      `park_javaos.hpp`) de `mutex_javaos.cpp` — verificar que o arquivo existe e
      tem a estrutura esperada. **Achado**: o mesmo bug de comentário C (`*/` dentro
      de `pthread_mutex_*/pthread_cond_*`, já visto em `glue.c`/`mutex_javaos.hpp`)
      se repetiu no comentário de topo deste arquivo — corrigido evitando a
      sequência `_*/` no comentário.
- [x] 1.2 Implementar `PlatformEvent::park()`: decremento atômico de `_event`
      (`AtomicAccess::cmpxchg` em loop), retorno imediato se o valor antes do
      decremento já era maior que zero, caso contrário bloqueio real em
      `pthread_cond_wait` dentro de `while (_event < 0)` (ignorando *spurious
      wakeups*) — mesmo protocolo de `os/posix/os_posix.cpp`
      (`PlatformEvent::park()`, já vendorizado, usado como referência literal) —
      verificar com `g++ -fsyntax-only -std=gnu++17` (mesma invocação de
      `os-javaos-skeleton`/`os-javaos-memory-pd`, rodada de dentro de
      `src/hotspot`) sem erro.
- [x] 1.3 Implementar `PlatformEvent::park(jlong millis)`/`park_nanos(jlong nanos)`:
      mesmo protocolo de 1.2, mas com prazo absoluto (`clock_gettime(CLOCK_REALTIME)`
      + conversão de millis/nanos para `timespec`, mesmo padrão de
      `PlatformMonitor::wait(millis)` em `mutex_javaos.cpp`) e `pthread_cond_timedwait`
      real em vez de `pthread_cond_wait`, retornando `OS_OK`/`OS_TIMEOUT` conforme o
      caso — verificar compilação como acima.
- [x] 1.4 Implementar `PlatformEvent::unpark()`: troca atômica de `_event` para `1`
      (`AtomicAccess::xchg`), e só sinaliza o `pthread_cond_t` via
      `pthread_cond_signal` se havia de fato um esperador (`_nParked != 0`),
      consultado com o mutex detido e já liberado antes de sinalizar (mesma ordem de
      `os_posix.cpp`, para evitar acordar um esperador à toa) — verificar compilação
      como acima.

## 2. `PlatformParker` real

- [x] 2.1 Implementar `PlatformParker::PlatformParker()` (inicializa `_counter = 0`,
      `_cur_index = -1`, `pthread_cond_init` em `_cond[REL_INDEX]` e
      `_cond[ABS_INDEX]`, `pthread_mutex_init` em `_mutex`) e
      `PlatformParker::~PlatformParker()` (destrói os mesmos três objetos) — mesmo
      arquivo da seção 1 — verificar compilação como acima.

## 3. Verificação e documentação

- [x] 3.1 Rodar `g++ -fsyntax-only -std=gnu++17 <mesmas flags de os-javaos-skeleton>
      park_javaos.cpp` uma última vez, de ponta a ponta (todos os métodos das
      tarefas 1-2 juntos), e confirmar nenhum erro nem warning novo (fora o
      `-Winvalid-offsetof` pré-existente já documentado em `os-javaos-memory-pd`,
      alheio a esta change). **Confirmado**: compila sem erro, único warning é o
      `-Winvalid-offsetof` pré-existente em `share/utilities/exceptions.hpp`, alheio
      a esta change.
- [x] 3.2 Adicionar a este change (`design.md`, "Resultado real da implementação") a
      confirmação da compilação e o relato de qualquer achado real durante a
      implementação (ex.: alguma dependência adicional de `share/utilities`/
      `share/memory` não prevista no design, ou alguma particularidade de
      `AtomicAccess`/`OrderAccess` sob o esqueleto `os_cpu/javaos_zero` atual).
- [x] 3.3 Atualizar `ROADMAP.md` — no destaque "o que falta para a JVM rodar", marcar
      o item 2 (`PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/`PlatformParker`)
      como concluído por completo (restando só `Parker::park`/`unpark`, explicitamente
      fora de escopo — ver proposal.md/design.md desta change — como dependência
      futura de integração de `JavaThread`/safepoint, não mais bloqueado por falta de
      primitiva de prazo), e revisar se o "Próximo passo imediato" deve apontar para
      outro item (ex.: exceções C++/RTTI/`libstdc++`, ou isolamento de falha por
      processo) — verificar que o documento permanece consistente. **Decisão**:
      "Próximo passo imediato" passa a apontar para "Isolamento de falha por
      processo" — a base de sincronização de baixo nível do HotSpot está completa, e
      esse item é o próximo passo kernel-side concretamente acionável (ao contrário
      de exceções C++/RTTI, condicionado a examinar o runtime/GC/JIT/interpretador
      primeiro, e de `Parker::park`/`unpark`, que depende de `JavaThread` real).

## Workflow follow-up

- Arquivar esta change (`openspec archive os-javaos-park-sync`) depois que a
  compilação real (tarefas 1.1-3.1) estiver confirmada.
