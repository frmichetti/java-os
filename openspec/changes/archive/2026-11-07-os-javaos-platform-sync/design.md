# Design

## Context

Ver `proposal.md` para a motivação. Estado atual relevante:

- `kernel::pthread::Mutex`/`CondVar` (kernel-side, `kernel-thread-sync`) já têm
  bloqueio real sem busy-wait, expostos a modo usuário via `SYSCALL_MUTEX_CREATE`/
  `LOCK`/`UNLOCK` e `SYSCALL_COND_CREATE`/`WAIT`/`NOTIFY_ONE`/`NOTIFY_ALL`
  (`kernel-jvm-posix-syscalls`). As tabelas de handle (`kernel::pthread_handles`) só
  crescem — nenhuma das duas suporta remoção, decisão deliberada já tomada antes
  desta change.
- `kernel::pthread::Mutex::try_acquire` (não-bloqueante, CAS pura) já existe, mas é
  privado — usado só internamente por `Mutex::lock()`.
- `userland_c/glue.c` não tem nenhuma função `pthread_mutex_*`/`pthread_cond_*` hoje
  — só os tipos (`pthread_mutex_t`/`pthread_cond_t = uint32_t` opaco,
  `PTHREAD_MUTEX_INITIALIZER`/`PTHREAD_COND_INITIALIZER = 0xFFFFFFFF`) vêm do newlib
  vendorizado.
- `os/javaos/mutex_javaos.hpp` declara `PlatformMutex`/`PlatformMonitor`, mas nenhum
  `.cpp` do porte os define — nem como stub `fatal()` (diferente dos `pd_*` de
  memória antes desta change, que ao menos existiam como stub).
- Nenhuma primitiva de espera com prazo existe em lugar nenhum da pilha:
  `kernel::wait_queue::WaitQueue::block_current()` bloqueia até ser despertada
  explicitamente, sem parâmetro de tempo; não há relógio associado ao bloqueio.
- Nenhuma syscall de criação de thread em modo usuário existe (22 syscalls hoje,
  0–21) — um processo de teste não pode criar uma segunda thread para provar
  contenção real de mutex ou um despertar real de `pthread_cond_wait`.

## Goals / Non-Goals

**Goals:**
- `pthread_mutex_init`/`lock`/`unlock`/`trylock`/`destroy` e `pthread_cond_init`/
  `wait`/`signal`/`broadcast`/`destroy` com corpo real em `userland_c/glue.c`.
- Uma nova syscall mínima (`SYSCALL_MUTEX_TRYLOCK`) para dar a `trylock` uma
  primitiva não-bloqueante real, em vez de aproximar com a bloqueante existente.
- `PlatformMutex`/`PlatformMonitor` com corpo real em um novo
  `os/javaos/mutex_javaos.cpp`, sobre as funções acima.
- `PlatformMonitor::wait(0)` (sem prazo) real; `wait(millis != 0)` como stub
  explícito honesto, não uma aproximação enganosa.
- Verificação em boot real do que de fato dá para provar sem threads concorrentes:
  round-trip real de mutex (init/lock/trylock/unlock/destroy) e de condvar sem
  esperador (init/signal/broadcast/destroy).

**Non-Goals:**
- Qualquer primitiva de espera com prazo (`pthread_cond_timedwait`-like) — exigiria
  um temporizador novo em `kernel::wait_queue`, fora de escopo.
- `PlatformEvent`/`PlatformParker` (`park_javaos.hpp`) — dependem da mesma espera com
  prazo ausente, mais a classe compartilhada `Parker` (`share/runtime/park.hpp`),
  ainda não examinada.
- Qualquer syscall de criação de thread em modo usuário — maior mudança estrutural
  (multithreading real dentro de um processo), rastreada separadamente, não parte
  desta change.
- Provar em boot real o bloqueio + despertar de `pthread_cond_wait` entre execuções
  distintas — bloqueado pelo Non-Goal anterior.
- Liberar de verdade um mutex/condvar destruído — as tabelas de handle do kernel já
  não suportam remoção (decisão anterior a esta change, não revista aqui).

## Decisions

### `pthread_mutex_t`/`pthread_cond_t` guardam o `handle` do kernel diretamente, sem indireção

Como `pthread_mutex_t`/`pthread_cond_t` já são só `uint32_t` opacos no newlib
vendorizado, e os `handle`s de `SYSCALL_MUTEX_CREATE`/`SYSCALL_COND_CREATE` são
pequenos inteiros sequenciais (`Vec::len() - 1`), a implementação guarda o `handle`
diretamente no valor do tipo — sem alocar nenhuma estrutura adicional em `glue.c`.
`pthread_mutex_init` simplesmente chama `SYSCALL_MUTEX_CREATE` e escreve o resultado
em `*mutex`.

### `_POSIX_THREADS` definido manualmente em `glue.c` antes de `#include <pthread.h>`

**Achado real durante a implementação**: `pthread.h` do newlib vendorizado envolve
todo o seu conteúdo (protótipos de função e macros como `PTHREAD_MUTEX_INITIALIZER`)
num único `#if defined(_POSIX_THREADS)` (`sys/features.h`), e esse macro só é
definido automaticamente dentro de blocos específicos de SO (`__rtems__`/`__XMK__`/
`__CYGWIN__`/etc.) — nenhum deles se aplica ao alvo genérico `x86_64-elf` deste
projeto (mesma categoria de achado de `libc-minimal-port` sobre flags de build do
newlib: a build genérica do newlib não presume nenhuma configuração de SO real).
Sem isso, nada em `pthread.h` **do newlib vendorizado** fica visível. Corrigido
definindo `_POSIX_THREADS 1` diretamente em `glue.c`, antes do `#include` — escolhido
em vez de uma flag `-D` de build porque só os arquivos que de fato compilam contra o
`pthread.h` do newlib precisam dela (`glue.c` e, pela mesma razão,
`javaos_pthread_test_program.c` receberia o mesmo `#define` local).
**Correção em relação ao plano original desta change** (achado só confirmado na
implementação, task 3.1): `mutex_javaos.cpp`/`mutex_javaos.hpp` **não** compila
contra o newlib vendorizado — a verificação por compilação de todo o esqueleto
`os_javaos` usa o `g++`/glibc do *host* (ver design.md de `os-javaos-skeleton`,
"Invocação de compilação real usada"), cujo `<pthread.h>` já é completo
independentemente de `_POSIX_THREADS`. Nenhuma flag `-D_POSIX_THREADS` extra é
necessária nem usada nessa invocação — só `glue.c` (e o programa de teste, que
compila de verdade contra o newlib vendorizado pelo toolchain cruzado) precisam do
`#define` local.
**Seguro**: `vendor/newlib-build/.../libc.a` não foi buildado com
`--enable-newlib-multithread` (confirmado via `nm` — nenhum símbolo
`pthread_mutex_*`/`pthread_cond_*` já existe no arquivo), então não há risco de
colisão de símbolo com as implementações novas desta change.

### Inicialização preguiçosa via comparação com o sentinela, sem proteção contra corrida

`PTHREAD_MUTEX_INITIALIZER`/`PTHREAD_COND_INITIALIZER` valem `0xFFFFFFFF`. Toda
função que recebe um `pthread_mutex_t*`/`pthread_cond_t*` (`lock`/`unlock`/`trylock`/
`wait`/`signal`/`broadcast`) verifica esse valor primeiro e, se for o sentinela, cria
o objeto real (mesma lógica de `pthread_mutex_init`) antes de prosseguir.
**Deliberadamente sem proteção contra corrida** (ex.: CAS na própria variável): hoje
nenhum processo de usuário pode ter duas threads (nenhuma syscall de criação de
thread existe — ver Non-Goals), então duas execuções não podem corrida para
inicializar o mesmo `pthread_mutex_t` estático simultaneamente. **Risco aceito e
documentado**: se uma syscall de criação de thread for adicionada no futuro sem
revisar este código, a inicialização preguiçosa passa a ter uma corrida real
(duas threads criando dois mutexes reais distintos para o mesmo `pthread_mutex_t`
estático, uma delas vazando) — ver Risks/Trade-offs.

### Nova syscall `SYSCALL_MUTEX_TRYLOCK` em vez de aproximar com a bloqueante existente

`PlatformMutex::try_lock()` exige uma tentativa que nunca bloqueia. `kernel::pthread::
Mutex` já tem exatamente essa primitiva (`try_acquire`, CAS pura sem tocar
`WaitQueue`/escalonador), só não exposta a modo usuário. Duas alternativas:

- **(Escolhida) Nova syscall** (`SYSCALL_MUTEX_TRYLOCK`, número 22) expondo
  `try_acquire` diretamente — `kernel::pthread::Mutex::try_acquire` passa de privado
  para `pub(crate)`, chamado por uma nova `sys_mutex_trylock` em
  `kernel::syscall`, mesma estrutura das demais chamadas de mutex/condvar.
  Extensão pura da ABI (nenhuma syscall existente muda), consistente com o
  crescimento incremental já praticado por este projeto (22 syscalls hoje, cada uma
  adicionada por uma change que precisou dela).
- (Rejeitada) Aproximar com `SYSCALL_MUTEX_LOCK` de algum jeito não-bloqueante do
  lado da libc (ex.: um loop de tentativas com timeout curto) — reintroduziria
  exatamente o tipo de busy-wait com interrupções que já causou o impasse
  documentado em `KNOW_ISSUES.md` ("duas threads de kernel imprimindo
  concorrentemente travam o timer de hardware para sempre"), e ainda não seria uma
  tentativa de verdade não-bloqueante (só rápida).

### `pthread_*_destroy` como sucesso real, sem liberar o objeto no kernel

Já documentado em `proposal.md`/Impact — POSIX permite que `destroy` não libere
memória imediatamente (é erro de uso destruir um mutex/condvar em uso, não uma
garantia de que a implementação precise reciclar recursos). Como as tabelas de
handle do kernel já não suportam remoção (decisão anterior, não revista aqui),
`pthread_mutex_destroy`/`pthread_cond_destroy` devolvem sucesso (`0`) sem emitir
nenhuma syscall nova — o handle já criado continua existindo no kernel (vazado),
mesmo comportamento que o próprio kernel já aceita para si.

### `PlatformMonitor::wait(millis)`: real só para `millis == 0`, stub explícito para `millis != 0`

Já decidido com o usuário (ver proposal.md) — a convenção do HotSpot (`millis == 0`
= esperar indefinidamente) é exatamente o único caso que `pthread_cond_wait` real
consegue honrar sem uma primitiva de prazo no kernel. Para `millis != 0`, a função
aborta com mensagem de diagnóstico explícita (`fatal()`), mesma filosofia dos `pd_*`
sem primitiva equivalente de `os-javaos-memory-pd` — nunca fingir um timeout que não
existe.

### Verificação em boot real: mesmo programa C real, escopo deliberadamente reduzido

Mesmo mecanismo de `os-javaos-memory-pd` (programa C real,
`userland_c/javaos_pthread_test_program.c`, compilado pelo toolchain cruzado,
carregado via `kernel::elf`/`spawn_from_elf`). Diferença importante: como nenhuma
syscall de criação de thread existe, o programa só pode provar operações que fazem
sentido numa única thread — init/lock/trylock/unlock/destroy de um mutex (lock e
trylock sobre um mutex livre, trylock sobre um mutex já tomado pela própria thread
chamadora, tudo observável sem segunda thread), e init/signal/broadcast/destroy de
uma condvar sem nenhum esperador (mesmo padrão seguro já usado por
`kernel::user_posix_demo`). **`pthread_cond_wait` não é exercitado pelo programa de
verificação** — chamá-lo sem ninguém para notificar de volta bloquearia o processo
de teste para sempre, travando a verificação em boot real inteira. Essa lacuna é
aceita e documentada (ver proposal.md, "Verificação em boot real limitada") em vez de
inventar um jeito artificial de notificar a si mesmo que não provaria nada real sobre
bloqueio entre execuções distintas.

## Risks / Trade-offs

- **[Risco] Inicialização preguiçosa sem proteção contra corrida vira um bug real
  assim que uma syscall de criação de thread existir** → Mitigação: documentado
  explicitamente aqui e no comentário do código (`glue.c`); uma change futura que
  adicione criação de thread SHALL revisar este código antes de ser considerada
  completa — não é um risco esquecido, é uma dependência futura explícita.
- **[Risco] `pthread_cond_wait` real nunca é exercitado em boot real nesta change** →
  Mitigação: aceito e documentado (mesma honestidade de `os-javaos-memory-pd` sobre
  o que cada change prova e como); o comportamento em si já é garantido pela syscall
  subjacente (`SYSCALL_COND_WAIT`, já verificada por `kernel-jvm-posix-syscalls`) — o
  que fica sem prova nesta change é só a fiação da função `pthread_cond_wait` em cima
  dela, um risco pequeno e localizado.
- **[Risco] `PlatformMonitor::wait(millis != 0)` abortando o processo pode travar
  qualquer caminho futuro do HotSpot que dependa de espera com prazo** (ex.:
  safepoint polling, GC) → Mitigação: aceito deliberadamente (ver Non-Goals) — o
  caminho correto é adicionar uma primitiva de prazo ao `kernel::wait_queue` antes de
  qualquer código real do HotSpot precisar dela, trabalho de uma change futura.
- **[Risco] Handles de mutex/condvar nunca liberados continuam crescendo a tabela
  indefinidamente** — já um risco aceito do projeto desde `kernel-jvm-posix-syscalls`,
  não introduzido nem agravado por esta change.

## Migration Plan

Puramente aditivo — nenhuma syscall existente muda de número, assinatura ou
comportamento; nenhum código existente (kernel, libc, runtime C++, programas de
teste já existentes) muda de comportamento. Nenhum passo de migração necessário.

## Resultado real da implementação (relato honesto — task 5.1)

Tudo planejado nesta change foi implementado e verificado. Três achados reais
exigiram desviar do plano original, todos documentados em `tasks.md` nas tarefas
correspondentes — resumidos aqui:

1. **`pthread.h` do newlib vendorizado só expõe seu conteúdo sob
   `_POSIX_THREADS`**, que não é definido para o alvo genérico `x86_64-elf` deste
   projeto — corrigido com `#define _POSIX_THREADS 1` local em `glue.c` e em
   `javaos_pthread_test_program.c`, antes de cada `#include <pthread.h>` (ver
   Decisions). **Não se aplica** a `mutex_javaos.cpp` — esse arquivo é verificado
   por compilação contra o glibc do *host*, que já expõe `pthread.h` por completo
   independentemente desse macro (correção em relação ao plano original da task
   3.1, que presumia erradamente que a mesma lacuna se aplicaria lá).
2. **O mesmo bug de comentário C (uma sequência `*/` escondida dentro de
   `pthread_mutex_*/pthread_cond_*`, fechando o comentário cedo demais) apareceu
   três vezes independentes** nesta change — em `glue.c`, em `mutex_javaos.cpp` e
   em `javaos_pthread_test_program.c`, cada uma causando uma cascata de dezenas de
   erros de compilação não relacionados ao ponto real do problema. Corrigido nas
   três ocorrências evitando a sequência `_*/` em qualquer comentário C novo.
3. **O nome de arquivo originalmente planejado para o executável de verificação
   (`javaospthread.elf`, 13 caracteres de base) excede o limite de nomes curtos 8.3
   que o driver FAT32 do kernel suporta** (`kernel-filesystem`) — `mcopy` aceita o
   nome na imagem de disco (gera um alias VFAT diferente internamente), mas o
   carregador ELF do kernel, que só reconhece 8.3 literal, falhava com `NotFound`
   ao procurar pelo nome original. Corrigido renomeando para `jpthread.elf` (8+3),
   mesmo padrão de `javaosvm.elf`/`libctest.elf`/`cxxtest.elf`.

### Verificação em boot real

`userland_c/javaos_pthread_test_program.c`, carregado como `/bin/jpthread.elf` via
`kernel::elf`/`spawn_from_elf`, reportou no console serial (QEMU, `cargo xtask
build-image` + `qemu-system-x86_64 ... -display none -serial stdio`):

```
javaos_pthread: /bin/jpthread.elf carregado e em execucao
javaos_pthread_test_program: resultado PPPPPPPPPP (ordem: mutex_init,lock,trylock_busy,unlock,trylock_free,unlock2,mutex_destroy,cond_init,signal_broadcast,cond_destroy)
```

Todos os 10 cenários passaram. Nenhum `EXCECAO`/pânico no log completo de boot; o
timer e as demonstrações já existentes (`wait_queue`, FPU, etc.) continuaram
avançando normalmente depois, confirmando que a nova syscall e as novas funções não
perturbaram nada já existente. `cargo test -p kernel --lib` (261 testes, incluindo
os 3 novos de `SYSCALL_MUTEX_TRYLOCK`) e `cargo kernel-build` também confirmados sem
regressão.

Como decidido desde o `proposal.md`, `pthread_cond_wait` **não** foi exercitado em
boot real — permanece verificado só pela leitura de código (chama `SYS_COND_WAIT`
exatamente como `kernel::user_posix_demo` já prova para a syscall bruta) e pela
compilação real de `mutex_javaos.cpp` contra os headers do HotSpot.
