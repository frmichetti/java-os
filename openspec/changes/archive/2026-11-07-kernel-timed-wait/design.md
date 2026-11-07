# Design

## Context

Ver `proposal.md` para a motivação. Estado atual relevante:

- `kernel::wait_queue::WaitQueue` bloqueia uma thread registrando uma "intenção de
  bloqueio" (`BLOCK_INTENT`, um único `Option<(ThreadId, &'static WaitQueue)>`) e
  disparando `int {BLOCK_VECTOR}` (`commit_block`), que reentra no mesmo mecanismo de
  troca de contexto do timer (`schedule_next`). A intenção só é consumida no próximo
  tick/interrupção de bloqueio (`take_block_intent`, chamada por `schedule_next`), que
  move a thread da `ReadyQueue` para a `WaitQueue` indicada. `commit_block` é só uma
  instrução `int` — não tem nenhum canal de retorno de valor para a thread que acabou
  de ser despertada.
- `kernel::scheduler::on_timer_tick` já roda a cada tick real do timer (100 Hz,
  `crate::timer::on_tick()`), antes de `schedule_next`.
- `kernel::clock::monotonic_nanos()` já existe — função livre, sem lock, baseada em
  TSC calibrado.
- `kernel::process::Context` já é indexada por `ThreadId.0` (mesmo padrão de
  `CONTEXTS.lock()[id.0 as usize]`), com campos como `fpu_state` já seguindo essa
  convenção.
- `kernel::syscall::sys_cond_wait(cond_handle, mutex_handle)` (`SYSCALL_COND_WAIT`,
  número 11) já é despachada com `arg0`/`arg1` apenas — `arg2` chega sempre `0` de
  todo chamador existente (`kernel::user_posix_demo`, e o futuro
  `userland_c/glue.c::pthread_cond_wait` desta própria libc), mas nunca é lido.
- **Achado real desta pesquisa**: `userland_c/glue.c` nunca implementou
  `clock_gettime`/`gettimeofday` — as duas syscalls de relógio
  (`SYSCALL_CLOCK_WALL`/`SYSCALL_CLOCK_MONOTONIC`) só foram exercitadas até hoje por
  assembly bruto (`kernel::user_posix_demo`), nunca por uma função POSIX real da
  libc. `PlatformMonitor::wait(millis)` precisa construir um prazo absoluto
  (`struct timespec`) para passar a `pthread_cond_timedwait` — sem `clock_gettime`
  real, não tem como. `<time.h>` do newlib vendorizado declara `clock_gettime`/
  `CLOCK_REALTIME`/`CLOCK_MONOTONIC` sob `#if defined(_POSIX_TIMERS)`, mesma família
  de macro de feature-test de `_POSIX_THREADS` (`libc-pthread-sync`) — não definida
  automaticamente para o alvo genérico `x86_64-elf` deste projeto.

Ver proposal.md para por que `PlatformEvent`/`PlatformParker` ficam fora de escopo.

## Goals / Non-Goals

**Goals:**
- Bloqueio com prazo em `WaitQueue`, consumido por `CondVar::wait_timeout`.
- `SYSCALL_COND_WAIT` interpretando `arg2` como prazo em nanossegundos (`0` =
  indefinido, comportamento idêntico ao de hoje).
- `pthread_cond_timedwait` real na libc, e `clock_gettime` real (`CLOCK_REALTIME`/
  `CLOCK_MONOTONIC`) como pré-requisito dela e de `PlatformMonitor::wait`.
- `PlatformMonitor::wait(millis != 0)` real em `mutex_javaos.cpp`.
- Verificação em boot real do caminho de **expiração** do prazo (provável sem
  nenhuma segunda thread, ao contrário de toda espera não-temporizada já entregue).

**Non-Goals:**
- `PlatformEvent`/`PlatformParker` (`park_javaos.hpp`) — ver proposal.md.
- Qualquer seleção de relógio por `pthread_condattr_setclock` — `pthread_cond_timedwait`
  assume `CLOCK_REALTIME` (comportamento padrão POSIX), única opção que
  `PlatformMonitor::wait` precisa.
- `nanosleep`/`clock_nanosleep` ou qualquer outra API de "dormir" — fora do pedido
  do roadmap, embora a primitiva de base sirva de alicerce para isso no futuro.
- Provar em boot real o caminho "notificado antes do prazo expirar" — mesma
  limitação de sempre (nenhuma syscall de criação de thread em modo usuário existe).

## Decisions

### Reaproveitar o terceiro argumento de `SYSCALL_COND_WAIT`, sem syscall nova

`arg2` já existe na ABI desta chamada (número + 4 argumentos é a convenção de toda
syscall deste projeto) e já chega `0` de todo chamador existente, sem nenhum uso.
Interpretá-lo como `timeout_nanos` (`0` = espera indefinida) é estritamente aditivo:
nenhum chamador existente muda de comportamento, e nenhum número de syscall novo
precisa ser reservado. Alternativa rejeitada: uma `SYSCALL_COND_TIMEDWAIT` dedicada —
cresceria a superfície de syscalls sem necessidade, já que a ABI atual já tinha
espaço para o parâmetro.

### Novo sentinela `SYSCALL_TIMEOUT`, distinto de `SYSCALL_ERROR`

A chamada agora tem três resultados possíveis: sucesso por notificação (`0`), erro de
handle (`SYSCALL_ERROR`, já existente), e expiração de prazo — precisa de um
terceiro valor distinto, já que o chamador precisa diferenciar "notificado" de
"expirou" (ao contrário de "handle inválido", que nunca é ambíguo com sucesso).
Escolhido `SYSCALL_TIMEOUT = u64::MAX - 1`.

### `Context.timed_out: bool` como canal de retorno entre o tick e a thread despertada

`commit_block` é só `int {vector}` — a rotina de entrada (`block_entry`/`timer_entry`)
não tem como devolver um valor "por que fui despertada" à função Rust que estava no
meio de `WaitQueue::block_current_with_deadline` quando bloqueou. Como `Context` já é
indexada por `ThreadId` e cada thread só pode estar bloqueada numa única espera por
vez, um campo booleano ali (`false` por padrão, setado para `true` pelo código que
desperta por expiração de prazo, e lido-e-limpo pela própria thread logo depois de
`commit_block()` retornar) é a extensão mais direta — mesmo padrão já usado por
`fpu_state`. Alternativa rejeitada: uma tabela separada (`Spinlock<BTreeMap<ThreadId,
bool>>`) — funcionaria, mas duplicaria uma indexação por `ThreadId` que `Context` já
tem, sem necessidade.

### Lista de prazos pendentes: `Vec` sem ordenação, busca linear a cada tick

`TIMED_WAITERS: Spinlock<Vec<(ThreadId, u64, &'static WaitQueue)>>`, varrida
inteiramente a cada tick (100 Hz). O número de esperas com prazo concorrentes é
pequeno (mesma escala de `ReadyQueue`, um `VecDeque` já varrido/manipulado
linearmente em vários pontos deste kernel) — uma fila de prioridade ordenada por
prazo evitaria a varredura completa a cada tick, mas adicionaria complexidade sem
nenhuma necessidade medida; mesma filosofia de "não otimizar sem medir" já seguida
pelo resto do escalonador.

### Invariante contra prazos presos (*stale*): purgar ao registrar qualquer nova intenção de bloqueio

**Achado real de design**: se uma thread for notificada normalmente antes do seu
prazo expirar, e iniciar uma **nova** espera (com ou sem prazo) na mesma fila antes
do prazo antigo chegar a expirar, a entrada antiga em `TIMED_WAITERS` ainda aponta
para essa `WaitQueue` — quando o tick eventualmente processá-la, encontraria a thread
bloqueada de novo (na nova espera) e a acordaria **prematuramente**, tratando por
engano a nova espera como se tivesse expirado. Corrigido fazendo com que toda
função que registra uma intenção de bloqueio (`set_block_intent`, chamada por
`block_current`, `block_current_if` e `register_intent_for` — o único ponto comum a
toda forma de bloqueio deste kernel) primeiro remova qualquer entrada pendente de
`TIMED_WAITERS` para essa mesma thread. Isso garante o invariante: uma entrada em
`TIMED_WAITERS` só existe enquanto a thread que a criou ainda não começou **nenhuma**
espera nova desde então (nem precisa saber se é a "mesma" espera — qualquer nova
intenção de bloqueio já invalida a anterior, correto por construção).

### `clock_gettime` real na libc (achado real, pré-requisito não previsto originalmente)

Ver Context — sem isso, `mutex_javaos.cpp` não tem como construir o prazo absoluto
que `pthread_cond_timedwait` exige. Implementado em `glue.c` sobre
`SYSCALL_CLOCK_WALL` (`CLOCK_REALTIME`) e `SYSCALL_CLOCK_MONOTONIC`
(`CLOCK_MONOTONIC`) já existentes, com o mesmo `#define _POSIX_TIMERS 1` local antes
de `#include <time.h>` que `libc-pthread-sync` já praticou para `_POSIX_THREADS` —
mesma causa raiz (macro de feature-test do newlib não definido automaticamente para
o alvo genérico `x86_64-elf`).

### `pthread_cond_timedwait`: prazo absoluto convertido para relativo antes da syscall

POSIX exige `abstime` absoluto (medido contra `CLOCK_REALTIME`, já que esta libc não
implementa `pthread_condattr_setclock`); a syscall (ver Decisions acima) espera um
prazo **relativo** em nanossegundos a partir de agora — mais simples de checar a
cada tick (só comparar contra `monotonic_nanos()` local, sem depender de qualquer
relógio de parede dentro do kernel). `pthread_cond_timedwait` calcula
`delta = abstime_nanos - wall_clock_nanos()` (via `SYS_CLOCK_WALL` diretamente,
sem depender do novo `clock_gettime` — mais direto, já que `glue.c` já tem acesso a
`javaos_syscall`). **Caso especial**: se `delta <= 0` (prazo já no passado), devolve
`ETIMEDOUT` imediatamente, sem emitir a syscall — evita bloquear a thread chamadora
por um tick inteiro só para descobrir que o prazo já tinha expirado.

### `PlatformMonitor::wait(millis)`: `clock_gettime(CLOCK_REALTIME)` + `millis` vira o `abstime`

Mesmo padrão de qualquer porte POSIX real do HotSpot (`os_posix.cpp`): soma `millis`
(convertido para nanossegundos) ao resultado de `clock_gettime(CLOCK_REALTIME, ...)`
para construir o `struct timespec` absoluto que `pthread_cond_timedwait` exige,
tratando *overflow* de nanossegundos (`tv_nsec >= 1_000_000_000`) incrementando
`tv_sec` — aritmética padrão de manipulação de `timespec`.

### Verificação em boot real do caminho de expiração, sem segunda thread

Ao contrário de toda espera sem prazo já entregue (nunca provada em boot real por
falta de syscall de criação de thread — `os-javaos-platform-sync`), o caminho de
**expiração** desta primitiva não precisa de nenhuma segunda execução: um programa
de teste chama `pthread_cond_timedwait` com um prazo curto (ex.: alguns
milissegundos) sobre uma variável de condição que ninguém mais notifica, mede o
tempo decorrido via `clock_gettime(CLOCK_MONOTONIC)` antes/depois, e confirma (a)
que a chamada retornou indicando expiração, e (b) que o tempo decorrido é
compatível com o prazo pedido (maior ou igual a ele, dentro da granularidade de 100
Hz do timer). O caminho "notificado antes de expirar" continua fora de alcance pela
mesma limitação de sempre.

## Risks / Trade-offs

- **[Risco] Granularidade de 100 Hz (10 ms) limita a precisão de qualquer prazo
  pedido** — um prazo de 1 ms, por exemplo, só é detectado no próximo tick (até 10 ms
  depois). Aceito: a spec já declara a expiração "dentro da granularidade do
  temporizador do kernel", não um prazo exato; suficiente para o caso de uso real
  (`PlatformMonitor::wait`, tipicamente usado com prazos de dezenas/centenas de ms ou
  mais).
- **[Risco] O invariante de purga de `TIMED_WAITERS` (ver Decisions) é a parte mais
  sutil desta change — um erro ali reintroduziria o próprio bug que ele resolve** →
  Mitigação: testável em host sem hardware (mesma categoria de teste puro já usada
  por `BLOCK_INTENT`/`take_block_intent`) — a tarefa de implementação cobre
  explicitamente o cenário "nova espera antes do prazo antigo expirar" com um teste
  dedicado.
- **[Risco] `clock_gettime`/`pthread_cond_timedwait` novos na libc podem ter efeitos
  colaterais não previstos sobre programas de usuário já existentes** (nenhum usa
  essas funções hoje) → Mitigação: aditivo puro — nenhuma função já existente muda de
  assinatura ou comportamento; a tarefa de verificação em boot real confirma que os
  programas de teste já existentes continuam passando sem regressão (mesma prática
  de `libc-float-math`).

## Migration Plan

Puramente aditivo — nenhuma syscall existente muda de número; `SYSCALL_COND_WAIT`
muda de assinatura semântica (`arg2` passa a ter efeito), mas todo chamador existente
já passava `0` ali, preservando exatamente o comportamento anterior. Nenhum passo de
migração necessário.

## Resultado real da implementação

Toda a primitiva descrita acima (grupo 1-4 das tarefas) foi implementada exatamente
como projetado, sem desvio de design além da correção de escopo já registrada em
`tasks.md` (tarefa 1.3 — nenhum `WaitQueue::block_current_with_deadline` separado,
já que `CondVar::wait_timeout` cobre o comportamento observável de ponta a ponta).

**Achado real grave durante a verificação em boot real (tarefa 5.3), fora do escopo
original desta change:** a primeira tentativa de rodar `/bin/timedw.elf` travava o
kernel inteiro com `EXCECAO #PF (page fault)` pouco depois do prazo de 50 ms
expirar — não por nenhum bug no código novo descrito acima, mas por um defeito
**pré-existente** em `kernel::syscall`: `SYSCALL_USER_RSP_SCRATCH` era uma única
`static` global (não por-thread) usada por `syscall_entry` para guardar/restaurar o
`RSP` de modo usuário ao redor de `syscall`/`sysret`. Isso era seguro enquanto
nenhuma syscall cedia a CPU para uma thread diferente no meio do caminho — deixou de
valer desde que `Mutex::lock`/`CondVar::wait` passaram a bloquear de verdade
(`kernel-pthread-sync`), mas nunca tinha sido exposto: nenhum cenário de boot real
anterior com espera bloqueante via syscall teve uma janela de bloqueio longa o
bastante, com outra thread fazendo syscalls concorrentes nesse intervalo exato, para
corromper o valor antes do despertar. `timedw.elf` é o primeiro cenário com uma
janela de bloqueio garantidamente longa (50 ms ≈ 5 ticks a 100 Hz) e sem nenhum
notificador — tempo de sobra para `test.elf` (laço apertado de `SYSCALL_WRITE`) e
outras threads corromperem a variável antes do despertar, devolvendo a `timedw.elf`
o `RSP` de usuário de `javaosvm.elf` em vez do seu próprio.

Consultado o usuário diretamente (corrigir agora, fora do escopo formal da proposta,
vs. adiar e registrar em `KNOW_ISSUES.md`), a decisão foi corrigir nesta mesma
sessão, já que sem a correção a tarefa 5.3/5.4 desta própria change não podia ser
verdadeiramente concluída. A correção: `SYSCALL_USER_RSP_SCRATCH` foi removida;
`kernel::process::Context` ganhou um novo campo `syscall_rsp_slot: u64` (endereço de
uma célula por-thread, alocada via `Box::leak`, mesma disciplina de `fpu_state`), e
um novo global `kernel::scheduler::CURRENT_SYSCALL_RSP_SLOT` (mesmo padrão de
`CURRENT_FPU_AREA`) é atualizado pelo escalonador a cada troca de contexto para
sempre apontar ao slot da thread que está de fato retomando — `syscall_entry` passa a
indireccionar por esse slot em vez de usar uma única `static` global. Detalhe
completo (incluindo o achado sobre qual registrador usar como temporário na
indireção, sem corromper o número da syscall em `RAX`) em `KNOW_ISSUES.md`.

**Verificado em boot real, depois da correção:**
- `timedwait_test_program: resultado PP (ordem: etimedout,elapsed_ge_requested)` —
  confirma que `pthread_cond_timedwait` devolveu `ETIMEDOUT` e que o tempo decorrido
  foi maior ou igual ao prazo pedido, sem nenhum `EXCECAO`/pânico (tarefa 5.3).
- Nenhuma regressão: `javaos_pthread_test_program` (10 `P`s), `libm_test_program` (5
  `P`s), `javaos_vm_test_program` (8 `P`s), `pthread`/`wait_queue`/
  `file_syscalls_demo`/`user_posix_demo`/DLOPEN-DLSYM/`libc_test_program` — todos com
  exatamente os mesmos resultados de antes desta change (tarefa 5.4).
- `cargo test -p kernel --lib`: 269 testes passando (mesmo total de antes desta
  change — os testes novos desta change substituem/complementam testes já
  existentes, sem adicionar regressão de contagem; ver tarefas 1.6/2.3).

O caminho "notificado antes de expirar" de `CondVar::wait_timeout` continua fora de
alcance em boot real pela mesma limitação de sempre (sem syscall de criação de
thread) — não é uma lacuna desta change, é a mesma já documentada para
`CondVar::wait` em `os-javaos-platform-sync`.
