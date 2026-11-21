# Design

## Context

Ver `proposal.md` — "Why" para a motivação. Esta change depende de
`pthread-create-thread` (arquivada): `os::create_thread` real já existe em
`os_javaos.cpp`, com o mesmo `OSThread`/padrão de verificação (`-fsyntax-only`,
nunca linkado) que esta change reaproveita.

Estado atual confirmado em código (não suposição):

- `os::create_main_thread`/`os::create_attached_thread` **não existem em
  nenhum arquivo** de `os/javaos` (confirmado via `grep`), apesar de
  declaradas em `share/runtime/os.hpp:631,646`.
- `Thread::set_as_starting_thread(JavaThread* jt)` (`share/runtime/thread.cpp:562`)
  chama `os::create_main_thread(jt)` — único chamador real de
  `create_main_thread`, por sua vez chamado de `Threads::create_vm()`
  (`share/runtime/threads.cpp:576`) logo após alocar o `JavaThread` primordial
  (`new JavaThread()`), **antes** de `vm_init_globals()`/`init_globals()`/
  `init_globals2()` (`share/runtime/init.cpp` — GC, code cache, interpretador,
  compilador, JVMTI) rodarem. Ou seja: estes dois hooks não dependem de
  nenhuma dessas ~40 chamadas de `init_globals`/`init_globals2` — só do
  `JavaThread` recém-alocado em si.
- `os::create_attached_thread` tem um segundo chamador real,
  `share/prims/jni.cpp:3830` (`AttachCurrentThread`) — fora de escopo desta
  change (ver proposal.md, "Fora do escopo").
- `os_bsd.cpp:754-798` (referência real já lida) mostra a forma exata:
  `create_main_thread` só delega para `create_attached_thread`, que aloca
  `OSThread`, grava nele o id nativo **da própria thread chamadora**
  (`os::Bsd::gettid()` — não um valor devolvido por `pthread_create`, já que
  esta thread nunca foi criada por ele), marca `RUNNABLE`, e associa ao
  `Thread` via `set_osthread`. Também faz `PosixSignals::hotspot_sigmask`
  (sem equivalente neste porte, Non-Goal) e `os::Bsd::init_thread_fpu_state()`
  (sem equivalente — FPU já é gerenciada pelo kernel por thread,
  `kernel-fpu-context-switch`, Non-Goal).
- **Achado real da exploração**: nenhuma função deste porte expõe "qual é o
  meu próprio identificador nativo" a partir de C — `javaos_get_thread_id`
  (`userland_c/glue.c`, de `libc-retargetable-locking`) já embrulha a
  `SYSCALL_GET_THREAD_ID` necessária, mas é `static` (uso interno das
  variantes recursivas de `__retarget_lock_*`, nunca pensada para ser
  chamada de fora). `pthread_self()` (declarada em `pthread.h`, nunca
  implementada neste porte) é exatamente a função POSIX padrão para isso —
  `os::create_attached_thread` a usa no lugar do equivalente a
  `os::Bsd::gettid()`.
- `osThread_javaos.hpp`/`OSThread` (de `os-javaos-skeleton`/
  `pthread-create-thread`) já tem tudo que os dois hooks precisam:
  construtor, `set_thread_id`/`set_state`, herdados de `OSThreadBase`
  (`ThreadState`/`RUNNABLE` já definidos ali) — nenhuma mudança adicional
  nesse arquivo necessária (ao contrário de `pthread-create-thread`, que
  precisou adicionar `set_startThread_lock`).
- `JavaThread::verify_not_published()` (`share/runtime/javaThread.hpp:483`)
  existe na árvore vendorizada — mesmo assert de depuração que
  `os_bsd.cpp::create_attached_thread` usa.

## Goals / Non-Goals

**Goals:**
- `os::create_main_thread`/`os::create_attached_thread` reais em
  `os_javaos.cpp`, verificados por `-fsyntax-only` contra os headers reais
  do HotSpot vendorizado — mesma disciplina de `os::create_thread`
  (`pthread-create-thread`).
- `pthread_self()` real em `userland_c/glue.c`, sobre
  `SYSCALL_GET_THREAD_ID` já existente.

**Non-Goals:**
- Qualquer uso de `os::create_attached_thread` via JNI
  (`AttachCurrentThread`, `share/prims/jni.cpp`) — `prims`/JNI é item 6 do
  roadmap, ainda não explorado. Esta change implementa o suficiente para
  `create_main_thread` funcionar; uma superfície JNI completa (validar
  `JNIEnv*`/`JavaVMAttachArgs`, por exemplo) fica para quando `prims` for
  escopada.
- `vm_init_globals()`/`init_globals()`/`init_globals2()`
  (`share/runtime/init.cpp`) e tudo que dependem (GC, interpretador,
  compilador, `classfile`/`oops`) — dependência muito mais distante,
  roadmap itens 2-4/6, nenhuma exploração ainda.
- Linkar ou executar `os_javaos.cpp`/qualquer arquivo do HotSpot — mesma
  disciplina de toda change `os-javaos-*` anterior: só `-fsyntax-only`. A
  verificação real de `pthread_self()` é via um programa C que o chama
  diretamente, nunca passando por código C++ do HotSpot.
- `PosixSignals::hotspot_sigmask`/estado de FPU específico de BSD/macOS —
  mesmos Non-Goals já aceitos por `os::create_thread`
  (`pthread-create-thread`): este porte não tem sinais POSIX implementados,
  e o estado de FPU por thread já é preservado pelo próprio kernel
  (`kernel-fpu-context-switch`).
- Qualquer mudança em `javaos_get_thread_id` (continua `static`, uso
  interno de `libc-retargetable-locking`) — `pthread_self()` é uma função
  nova e independente, não uma mudança de visibilidade daquela.

## Decisions

### `pthread_self()` chama a syscall diretamente, não reaproveita `javaos_get_thread_id`
```c
pthread_t pthread_self(void) {
    return (pthread_t)javaos_syscall(SYS_GET_THREAD_ID, 0, 0, 0, 0);
}
```
Mesma primitiva de kernel (`SYSCALL_GET_THREAD_ID`), mas uma função pública
independente — `javaos_get_thread_id` (`static`) continua servindo só às
variantes recursivas de `__retarget_lock_*`, sem precisar mudar sua
assinatura/visibilidade para esta change (que teria um raio de efeito maior
que o necessário, tocando código já verificado por `libc-retargetable-locking`
sem motivo). `SYS_GET_THREAD_ID`/`javaos_syscall` já existem em `glue.c`,
reaproveitados sem mudança.

### `os::create_attached_thread` usa `pthread_self()` no lugar de `os::Bsd::gettid()`
```cpp
bool os::create_attached_thread(JavaThread* thread) {
#ifdef ASSERT
  thread->verify_not_published();
#endif

  OSThread* osthread = new (std::nothrow) OSThread();
  if (osthread == nullptr) {
    return false;
  }

  osthread->set_thread_id(pthread_self());
  osthread->set_state(RUNNABLE);
  thread->set_osthread(osthread);

  return true;
}

bool os::create_main_thread(JavaThread* thread) {
  return create_attached_thread(thread);
}
```
Idêntico em estrutura a `os_bsd.cpp:754-798`, com `pthread_self()` no lugar
de `os::Bsd::gettid()` (mesmo papel: identificador nativo da thread
chamadora, descoberto nela mesma — nunca um valor emprestado de outra
thread ou da criação). Sem `assert(os::Bsd::_main_thread == pthread_self(), ...)`
em `create_main_thread` (não existe `_main_thread`/conceito de "thread
principal do processo" registrado em nenhum lugar deste porte — não é uma
garantia que este porte hoje consiga verificar, e nenhuma change anterior
introduziu esse conceito).

### Novo include necessário: `runtime/javaThread.hpp`
`os::create_main_thread`/`create_attached_thread` recebem `JavaThread*`
(não `Thread*`, ao contrário de `os::create_thread`) e chamam
`thread->verify_not_published()` (`JavaThread`, não `Thread`) — exige
`#include "runtime/javaThread.hpp"` novo em `os_javaos.cpp`.
`runtime/thread.hpp`/`runtime/mutexLocker.hpp`/`<pthread.h>` (já incluídos
por `pthread-create-thread`) cobrem o resto.

## Risks / Trade-offs

- **[Risco]** `os_javaos.cpp` nunca é linkado/executado — um erro de lógica
  em `create_main_thread`/`create_attached_thread` (não só de sintaxe) nunca
  seria pego por esta change. → **Mitigação aceita, não resolvida**: mesma
  limitação já aceita por toda a linha de changes `os-javaos-*` desde
  `os-javaos-skeleton`.
- **[Trade-off]** `create_attached_thread` sem assert de "sou mesmo a thread
  primordial" em `create_main_thread` — aceitável: o único chamador real
  (`Threads::create_vm()`) já garante isso pela própria ordem de chamada,
  e nenhum outro código deste porte chama `create_main_thread` diretamente.

## Migration Plan

Não aplicável — nenhuma mudança de comportamento para código já existente
(`pthread_self()` é uma função nova; os dois hooks `os::` eram `fatal()`/
inexistentes antes).

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.

## Resultado real da implementação

Implementado exatamente conforme planejado, sem nenhuma revisão de design
necessária. Achados reais durante a implementação e a verificação:

- **`pthread_self()` compilou e funcionou de primeira**: `glue.c` já tinha
  toda a infraestrutura necessária (`javaos_syscall`, `SYS_GET_THREAD_ID`)
  — a nova função é só uma chamada direta, sem nenhuma surpresa.
- **`#include "runtime/javaThread.hpp"` foi o único include novo
  necessário** em `os_javaos.cpp` — `runtime/thread.hpp`/
  `runtime/mutexLocker.hpp`/`<pthread.h>` (já incluídos por
  `pthread-create-thread`) cobriram o resto. Como efeito colateral
  esperado (não um problema), o novo include trouxe mais ocorrências do
  aviso pré-existente `-Winvalid-offsetof` (agora também em
  `share/runtime/javaThread.hpp`/`os_cpu/javaos_zero/javaThread_javaos_zero.hpp`)
  — mesmo aviso já documentado em `os-javaos-memory-pd`, nenhum erro novo.
- **Confirmado por leitura de código, não suposição**: `os::create_main_thread`
  é chamado por `Thread::set_as_starting_thread` (`share/runtime/thread.cpp:562`),
  por sua vez chamado por `Threads::create_vm()` (`share/runtime/threads.cpp:576`)
  logo após `new JavaThread()`, **antes** de `vm_init_globals()` sequer ser
  chamada. Isso confirma a decisão de escopo do proposal.md/design.md: os
  dois hooks genuinamente não dependem de nenhuma parte de
  `init_globals()`/`init_globals2()` (GC, code cache, interpretador,
  compilador) — a formulação anterior do roadmap ("mapear `init.cpp`")
  tratava dois problemas de tamanhos muito diferentes como um só.
- **Verificação de compilação real**: `os_javaos.cpp`, `mutex_javaos.cpp`
  e `park_javaos.cpp` (regressão, já que `osThread_javaos.hpp` não foi
  tocado por esta change — ao contrário de `pthread-create-thread` — mas
  revalidados por precaução) continuam compilando via
  `g++ -fsyntax-only -std=gnu++17` (mesma invocação de changes anteriores).
- **Verificação em boot real sem regressão**: todo programa de teste já
  existente reporta exatamente a mesma string P/F de antes. Novo
  `/bin/pthreads.elf` reporta `resultado PPPP` (duas threads criadas via
  `pthread_create`, cada uma confirmando via `pthread_self()` que seu
  próprio identificador bate com o `pthread_t` que a criadora recebeu, e
  que os dois nunca se confundem entre si).
- `cargo test -p kernel --lib -- --test-threads=1`: 338 testes, 0 falhas
  (serializado para evitar a *flake* pré-existente e não relacionada de
  `kernel::acpi::tests`, já registrada em `KNOW_ISSUES.md` desde
  `pthread-create-thread`).
