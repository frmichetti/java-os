# Design

## Context

Ver `proposal.md` — "Why" para a motivação (locking interno do newlib hoje
colapsa para no-op, inofensivo até `kernel-user-thread-create` existir).

Estado atual confirmado em código/configuração:

- `newlib/libc/include/sys/lock.h`: sem `_RETARGETABLE_LOCKING`, `_LOCK_T`
  é só `int` e toda macro (`__lock_acquire_recursive`, etc.) expande para
  `((void) 0)`. Com `_RETARGETABLE_LOCKING`, `_LOCK_T` vira `struct __lock
  *` (opaco, definido pelo retarget) e cada macro chama uma função real
  `__retarget_lock_*` que o retarget deve fornecer: `init`/
  `init_recursive`, `acquire`/`acquire_recursive`, `release`/
  `release_recursive`, `close`/`close_recursive`, `try_acquire`/
  `try_acquire_recursive` — 10 no total.
- A flag de `configure` exata é `--enable-newlib-retargetable-locking`
  (`newlib/configure.ac:285-292`), que define `_RETARGETABLE_LOCKING` via
  `AC_DEFINE` (linha 538-540) no `newlib.h` gerado.
- `newlib/libc/stdlib/_mallocr.c` já define `MALLOC_LOCK`/`MALLOC_UNLOCK`
  incondicionalmente como `__malloc_lock(reent_ptr)`/
  `__malloc_unlock(reent_ptr)` (linhas 313-314) — já chama de verdade, sem
  depender de nenhuma flag adicional além de `_RETARGETABLE_LOCKING`.
  `newlib/libc/stdlib/mlock.c` já fornece `__malloc_lock`/
  `__malloc_unlock` padrão, chamando `__lock_acquire_recursive`/
  `__lock_release_recursive` sobre `__malloc_recursive_mutex` — nenhuma
  mudança nesses dois arquivos é necessária, só a flag de build.
- As 8 instâncias `extern struct __lock` que precisam de definição real
  (via `__LOCK_INIT_RECURSIVE`/`__LOCK_INIT`, que expandem para `extern
  struct __lock __lock_<nome>; static _LOCK_T <nome> = &__lock_<nome>`),
  achadas por `grep -r __LOCK_INIT_RECURSIVE\|__LOCK_INIT\( newlib/libc/`:
  `__malloc_recursive_mutex` (`mlock.c`), `__sfp_recursive_mutex`
  (`stdio/findfp.c`), `__env_recursive_mutex` (`stdlib/envlock.c`),
  `__atexit_recursive_mutex` (`stdlib/__call_atexit.c`) — recursivas;
  `__dd_hash_mutex` (`posix/telldir.c`), `__at_quick_exit_mutex`
  (`stdlib/quick_exit.c`), `__tz_mutex` (`time/tzlock.c`),
  `__arc4random_mutex` (`stdlib/arc4random.h`) — não recursivas.
- `userland_c/glue.c` já implementa `pthread_mutex_*` real sobre
  `SYSCALL_MUTEX_CREATE`/`LOCK`/`UNLOCK`/`TRYLOCK` (change
  `kernel-pthread-sync`) — reaproveitado diretamente aqui, sem precisar de
  nenhuma primitiva de kernel nova além de `SYSCALL_GET_THREAD_ID`.
- `kernel::scheduler::current_thread_id()` já existe (`pub(crate) fn
  current_thread_id() -> ThreadId`, usado internamente pelo escalonador) —
  só falta expô-lo como *syscall*, mesmo padrão trivial de
  `SYSCALL_GET_TICKS`.

## Goals / Non-Goals

**Goals:**
- `malloc`/`free` e os demais recursos internos do newlib listados acima
  ficam seguros sob concorrência real entre threads do mesmo processo.
- Locks recursivos (`malloc`/`stdio`/`env`/`atexit`) suportam reentrância
  real pela mesma thread, sem auto-deadlock.
- Programas de uma única thread continuam idênticos a antes.

**Non-Goals:**
- `pthread_create`/`os::create_thread`/`os_javaos::pd_start_thread` reais —
  change seguinte.
- Expor `__retarget_lock_*`/`struct __lock` como uma API pública nova para
  programas C deste projeto usarem diretamente — é infraestrutura interna
  do retarget do newlib, não uma capability de usuário final.
- `__lock_init`/`__lock_acquire` (variantes **não**-recursivas da API
  pública `<sys/lock.h>`, usadas por código de aplicação que declare seus
  próprios locks via `__LOCK_INIT`) — nenhum código deste projeto usa isso
  hoje; só as 8 instâncias internas do próprio newlib precisam de
  definição.
- Destruição real de qualquer um dos 8 mutexes internos ao processo
  terminar (`__retarget_lock_close*`) além de liberar o handle do kernel —
  mesma disciplina de "leak aceitável por enquanto" já aceita para
  `kernel::pthread_handles` (change `kernel-pthread-sync`).

## Decisions

### Um único `struct __lock` para os 8 casos, recursivos e não-recursivos
```c
struct __lock {
    long mutex_handle;   /* handle de SYSCALL_MUTEX_CREATE */
    long owner_tid;      /* -1 se livre; só usado pelas variantes recursivas */
    int depth;           /* só usado pelas variantes recursivas */
};
```
Mais simples que dois tipos distintos — os campos `owner_tid`/`depth`
simplesmente nunca são tocados pelas funções não-recursivas
(`__retarget_lock_acquire`/`release`/`try_acquire`, sem sufixo `_recursive`,
usadas por `__dd_hash_mutex`/`__at_quick_exit_mutex`/`__tz_mutex`/
`__arc4random_mutex`), que só fazem `pthread_mutex_lock`/`unlock`/`trylock`
direto sobre `mutex_handle`.

### Reentrância via `SYSCALL_GET_THREAD_ID`, não um contador por-lock ingênuo
`__retarget_lock_acquire_recursive(lock)`:
```c
long me = javaos_get_thread_id();  /* nova syscall */
if (lock->owner_tid == me) {
    lock->depth++;
    return;
}
javaos_syscall(SYS_MUTEX_LOCK, lock->mutex_handle, 0, 0, 0);
lock->owner_tid = me;
lock->depth = 1;
```
`__retarget_lock_release_recursive(lock)`:
```c
lock->depth--;
if (lock->depth == 0) {
    lock->owner_tid = -1;
    javaos_syscall(SYS_MUTEX_UNLOCK, lock->mutex_handle, 0, 0, 0);
}
```
**Por que precisa mesmo da nova syscall**: o mutex do kernel
(`SYSCALL_MUTEX_LOCK`) não é recursivo — a mesma thread chamando `lock`
duas vezes bloquearia esperando a si mesma para sempre. A verificação
`lock->owner_tid == me` tem que acontecer **antes** de qualquer chamada ao
mutex do kernel, e só uma forma real de "quem sou eu" resolve isso sem
depender de nenhum estado adicional por-thread (TLS incluída — os campos
`owner_tid`/`depth` vivem no PRÓPRIO lock, compartilhado entre threads,
não em armazenamento local de alguma thread específica).

### Inicialização dos 8 `struct __lock` no boot do processo (`crt0.S`), não sob demanda
Cada um dos 8 `struct __lock __lock_<nome>` é uma `static` global em
`glue.c`, com `mutex_handle` preenchido por uma nova função
`__javaos_init_internal_locks(void)` chamada por `crt0.S` **antes** de
`call main` — ponto em que o processo garantidamente só tem uma thread
(nenhuma `SYSCALL_THREAD_CREATE` pode ter acontecido ainda). Evita
completamente o problema clássico de "quem cria o lock que protege a
criação do lock" (double-checked locking sem átomo real disponível em C
puro neste projeto) — inicialização de mutex sempre single-threaded por
construção, sem precisar de nenhuma primitiva de inicialização atômica
nova.

**Alternativa considerada e rejeitada**: inicialização sob demanda (lazy,
na primeira chamada de `__retarget_lock_acquire*`) — exigiria uma seção
crítica para proteger a própria criação do mutex (ex.: um
`SYSCALL_MUTEX_CREATE` chamado duas vezes concorrentemente pela primeira
aquisição de duas threads diferentes), sem nenhum átomo de CAS disponível
neste ambiente para arbitrar com segurança. Inicializar tudo antes de
`main` elimina o problema por completo.

### `SYSCALL_GET_THREAD_ID = 24`
Próximo número livre após `SYSCALL_THREAD_CREATE = 23`
(`kernel-user-thread-create`). Handler trivial:
```rust
fn sys_get_thread_id() -> u64 {
    crate::scheduler::current_thread_id().0
}
```
Já existe internamente (`pub(crate) fn current_thread_id() -> ThreadId`)
— só precisa de um wrapper `pub(crate)` → chamável do dispatch, e virar
`pub` o suficiente para `kernel::syscall` chamar (hoje `pub(crate)`, já
acessível dentro do crate).

## Risks / Trade-offs

- **[Risco]** Esquecer de inicializar um dos 8 locks antes de `main`
  deixaria `mutex_handle == 0` (valor padrão), que
  `SYSCALL_MUTEX_LOCK`/`UNLOCK` rejeitariam como handle desconhecido
  (`EINVAL`) — nunca como corrupção silenciosa, já que o kernel já valida
  handles desconhecidos (change `kernel-handle-ownership`). Mitigado por
  verificar explicitamente, no novo programa de teste, que todas as
  operações que dependem desses locks (`malloc` concorrente) funcionam
  normalmente desde a primeira chamada.
- **[Risco]** Esta mudança reconstrói o newlib inteiro — qualquer
  diferença de comportamento introduzida pela própria flag
  `_RETARGETABLE_LOCKING` (além do *locking* em si) afetaria todo programa
  já existente. → **Mitigação**: verificar em boot real que todos os
  programas de teste já existentes continuam reportando exatamente as
  mesmas strings P/F de antes, mesma disciplina já usada em toda change
  anterior que tocou `build-newlib.sh`/vendorização.
- **[Trade-off]** Locks não-recursivos nunca liberados ao terminar o
  processo (mesma disciplina de `pthread_handles`) — aceitável, mesmo
  racional já aceito.
- **[Risco]** `SYSCALL_GET_THREAD_ID` devolve o `ThreadId.0` bruto (um
  `u64` monotônico, nunca reciclado) — poderia, em teoria, colidir com a
  faixa reservada de erro de `syscall_err` se algum dia esse contador
  ultrapassar `u64::MAX - 300`. → **Mitigação**: aceitável na prática (um
  contador de 64 bits nunca chegaria perto disso neste kernel), mesma
  categoria de não-risco já aceita para outros identificadores crus
  devolvidos por syscall (ex.: handles de mutex/arquivo).

## Migration Plan

Reconstrução do newlib necessária: `scripts/cross/build-newlib.sh` precisa
rodar de novo depois desta change (apaga/reconfigura `vendor/newlib-build`
com a nova flag). Documentar isso explicitamente no `README`/mensagem de
erro já existente do script, se ainda não cobrir esse caso.

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.
