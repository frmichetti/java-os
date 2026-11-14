# Design

## Context

`park_javaos.hpp` (criado por `os-javaos-skeleton`) já declara `PlatformEvent` e
`PlatformParker` com exatamente os mesmos campos/assinaturas de
`os/posix/park_posix.hpp` — mas nenhum método tem corpo em lugar nenhum da árvore
vendorizada ainda (`park_javaos.cpp` não existe). `os-javaos-platform-sync` já
estabeleceu o padrão de arquivo (`mutex_javaos.cpp`) para a dupla de classes
`PlatformMutex`/`PlatformMonitor`, sobre `pthread_mutex_*`/`pthread_cond_*` reais da
libc (`userland_c/glue.c`); `kernel-timed-wait` entregou `pthread_cond_timedwait`
real, usado por `PlatformMonitor::wait(millis != 0)`. Ver proposal.md para a
motivação completa e o porquê de `Parker::park`/`unpark` (classe compartilhada,
corpo esperado em `os_<os>.cpp`) ficarem fora de escopo.

A implementação de referência real do upstream OpenJDK já está vendorizada em
`vendor/jdk-27-ga/src/hotspot/os/posix/os_posix.cpp` (linhas ~1703-1989,
`PlatformEvent`/`PlatformParker`/`Parker`) — este design segue literalmente o mesmo
protocolo, só trocando a implementação de mutex/condvar/relógio pela libc deste
projeto.

## Goals / Non-Goals

**Goals:**
- `PlatformEvent` com comportamento real e correto de semáforo binário de 3 estados
  (`_event`: `-1` bloqueado / `0` neutro / `1` sinalizado), incluindo a variante com
  prazo (`park(jlong millis)`/`park_nanos(jlong nanos)`), sobre `pthread_mutex_t`/
  `pthread_cond_t`/`pthread_cond_timedwait` reais.
- `PlatformParker::PlatformParker()`/`~PlatformParker()` com inicialização/liberação
  reais do mutex e dos dois `pthread_cond_t` (`REL_INDEX`/`ABS_INDEX`).
- Verificação de compilação real (`g++ -fsyntax-only -std=gnu++17`) contra a árvore
  vendorizada, mesma invocação de `os-javaos-memory-pd`/`os-javaos-platform-sync`.

**Non-Goals:**
- `Parker::park(bool isAbsolute, jlong time)`/`Parker::unpark()` (classe `Parker`,
  compartilhada, declarada em `share/runtime/park.hpp`) — dependem de
  `JavaThread::current()`/`ThreadBlockInVM`/`OSThreadWaitState`/safepoint, nunca
  referenciados em nenhuma change anterior deste porte (ver proposal.md, "Impact").
  `PlatformParker` fica com seus dados prontos para uso (`_counter`/`_cur_index`/
  `_mutex`/`_cond[2]`), mas nenhum consumidor real os usa ainda.
- Verificação em boot real via um programa de teste rodando no QEMU — ver Decisions,
  "Verificação só por compilação, não em boot real" abaixo.
- Qualquer mudança em `park_javaos.hpp`, `mutex_javaos.{hpp,cpp}`, no kernel, ou na
  libc (`userland_c/glue.c`) — esta change só adiciona `park_javaos.cpp`.

## Decisions

### `PlatformEvent::park()`/`unpark()` seguem o protocolo real do upstream, não uma versão simplificada

Em vez de implementar `park`/`unpark` como um `CondVar::wait`/`notify` ingênuo (que
perderia uma sinalização que chega antes do `park()` correspondente), a
implementação segue exatamente o protocolo de 3 estados já vendorizado em
`os_posix.cpp`: `_event` é decrementado atomicamente antes de decidir bloquear (só
bloqueia se o resultado for `< 0`), e `unpark()` sempre força `_event` para `1` via
troca atômica, sinalizando o `pthread_cond_t` só se havia mesmo um esperador
(`_nParked`). **Alternativa rejeitada**: implementar sobre `kernel::pthread::CondVar`
diretamente (como `PlatformMonitor` faz) — rejeitada porque `PlatformEvent` precisa
da garantia "um `unpark()` antes do `park()` correspondente não se perde", que um
`CondVar::wait`/`notify` comum não oferece (uma notificação sem esperador nenhum é
simplesmente descartada) — exatamente o motivo de o upstream usar um contador restrito
(`_event`) em vez de depender só do condvar.

### `AtomicAccess`/`OrderAccess` usados diretamente, sem adaptação

`AtomicAccess::cmpxchg`/`xchg` e `OrderAccess::fence()` são utilitários genéricos do
HotSpot (`share/runtime/atomicAccess.hpp`/`orderAccess.hpp`), já vendorizados,
independentes de SO — nenhuma das changes anteriores deste porte precisou adaptá-los
(eles compilam contra qualquer target que defina os tipos/intrínsecos de CPU
esperados, já cobertos pelo esqueleto `os_cpu/javaos_zero` existente). Usados
exatamente como no upstream, sem nenhuma camada extra deste projeto por cima.

### Verificação só por compilação, não em boot real

Ao contrário de `PlatformMutex`/`PlatformMonitor` (verificadas em boot real via
`userland_c/javaos_pthread_test_program.c`, porque mapeiam 1:1 em cima de
`pthread_mutex_t`/`pthread_cond_t`, já expostos a C por `userland_c/glue.c`),
`PlatformEvent`/`PlatformParker` são classes C++ internas do HotSpot, sem nenhuma
superfície chamável a partir de C. Testá-las em boot real exigiria compilar um
programa de teste que inclui a árvore real do HotSpot (`memory/allocation.hpp` e
companhia) sob o toolchain cruzado `-ffreestanding -nostdlib` deste projeto — nenhum
arquivo deste projeto jamais fez isso (só os próprios arquivos do HotSpot são
compilados com o g++ do host, nunca com o `scripts/cross/x86_64-elf-g++`). Fazer isso
funcionar (resolver todas as dependências transitivas de `share/utilities`/
`share/memory` sob um ambiente *freestanding*) é uma extensão de escopo própria,
não incidental a "implementar dois corpos de método". **Decisão**: esta change se
limita à mesma verificação de compilação real (`g++ -fsyntax-only -std=gnu++17`)
já usada para partes do porte sem superfície C (vários `pd_*` de
`os-javaos-memory-pd`) — aceitável pela mesma razão já documentada lá: prova que o
código compila e type-checa contra a interface real do HotSpot, mesmo sem executá-lo
de verdade.

## Risks / Trade-offs

- **[Risco] Sem verificação em boot real, um erro de lógica em `PlatformEvent`
  (ex.: uma condição de corrida entre `park()`/`unpark()`) só apareceria quando o
  HotSpot de verdade rodar sobre este porte** → Mitigação: a implementação segue
  literalmente o protocolo já *battle-tested* do upstream (`os_posix.cpp`), sem
  nenhuma lógica nova inventada por este projeto — risco residual aceito, mesma
  categoria de risco já aceita pelos `pd_*` de memória sem superfície C.
- **[Risco] `AtomicAccess`/`OrderAccess` podem precisar de alguma definição adicional
  específica de CPU/SO não coberta pelo esqueleto `os_cpu/javaos_zero` atual** →
  Mitigação: a tarefa de implementação cobre explicitamente a compilação real como
  critério de conclusão; se precisar de algo adicional, isso vira um achado real
  documentado em vez de uma suposição não verificada.

## Migration Plan

Puramente aditivo — um novo arquivo (`park_javaos.cpp`), nenhuma mudança em arquivo
já existente. Nenhum passo de migração necessário.

## Resultado real da implementação

`park_javaos.cpp` foi criado com `PlatformEvent::PlatformEvent()`/`park()`/
`park(jlong millis)`/`park_nanos(jlong nanos)`/`unpark()` e
`PlatformParker::PlatformParker()`/`~PlatformParker()` reais, seguindo literalmente o
protocolo já vendorizado em `os/posix/os_posix.cpp` (decremento/troca atômica de
`_event` via `AtomicAccess::cmpxchg`/`xchg`, `OrderAccess::fence()` nos mesmos pontos,
`pthread_cond_timedwait` real para a variante com prazo) — nenhum desvio de design em
relação ao planejado acima.

**Achado real durante a implementação**: o mesmo bug de comentário C já visto em
`glue.c`/`mutex_javaos.hpp` (um `*/` dentro da sequência `pthread_mutex_*/pthread_cond_*`
fechando o comentário de bloco cedo demais) se repetiu no comentário de topo deste
arquivo — corrigido reescrevendo a frase para evitar a sequência `_*/` literal.

**Verificado**: `g++ -fsyntax-only -std=gnu++17` (mesma invocação de
`os-javaos-skeleton`/`os-javaos-memory-pd`, rodada de dentro de `src/hotspot`, via
WSL — o g++ do Git Bash/Windows não tem `alloca.h`/glibc real, não serve para este
tipo de verificação) contra a árvore real vendorizada, sem erro — único warning é o
`-Winvalid-offsetof` pré-existente em `share/utilities/exceptions.hpp`
(`ThreadShadow::pending_exception_offset`), já documentado em `os-javaos-memory-pd`,
alheio a esta change.
