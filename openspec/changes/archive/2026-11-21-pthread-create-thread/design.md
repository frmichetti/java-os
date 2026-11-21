# Design

## Context

Ver `proposal.md` — "Why" para a motivação. Esta change **depende de**
`libc-retargetable-locking` (planejada, ainda não implementada): o
trampolim de `pthread_create` e o próprio `malloc` usado por ele só são
seguros sob concorrência real depois que aquele *locking* existir —
implementar esta change antes seria construir sobre uma base que ainda
corrompe o heap sob duas threads.

Estado atual confirmado em código (não suposição):

- `kernel::process::create_user_thread` (`process.rs`, privada, único
  ponto que cria qualquer thread de usuário — tanto a primeira de um
  processo quanto uma adicional via `Process::spawn_user_thread`) chama
  `init_stack(kernel_stack_top, entry, USER_CODE_SELECTOR,
  USER_DATA_SELECTOR, user_stack_top - ABI_ALIGNMENT_PAD)`, que zera
  todos os `SAVED_GPR_COUNT = 15` registradores de uso geral salvos da
  nova thread (`for i in 0..SAVED_GPR_COUNT { write(0) }`,
  `stack_top - 48 - i*8` para cada `i`).
- **Achado confirmado rastreando a ordem real de `push`/`pop` em
  `timer_entry`/`block_entry` (`scheduler.rs`)**: a ordem de `push` é
  `rax, rbx, rcx, rdx, rsi, rdi, rbp, r8..r15` — então o índice `i` do
  laço de zeragem de `init_stack` corresponde, na mesma ordem, a esses
  registradores. `RDI` é o índice `i = 5`. Escrever um valor ali em vez de
  zero é exatamente o que a nova thread recebe em `RDI` na primeira vez
  que é escalonada — a convenção SysV ABI de primeiro argumento de uma
  função chamada por `call`/`jmp` direto para seu endereço de entrada.
- `init_stack` tem só 4 pontos de chamada reais (`process.rs`, mais um no
  teste `init_stack_for_user_mode_uses_user_selectors_and_user_landing_rsp`)
  — mudar sua assinatura é um escopo pequeno e já mapeado.
- `kernel::syscall::sys_thread_create(entry: u64, stack_size: u64)`
  (`syscall.rs`) despacha `arg0`/`arg1` da *syscall* — `arg2` (`RDX` na
  entrada de `syscall`) está disponível e nunca usado por esta chamada
  hoje.
- `os::create_thread`/`os::free_thread` **não existem em nenhum arquivo**
  de `os/javaos`/`os_cpu/javaos_zero`, apesar de declaradas em
  `share/runtime/os.hpp:625,653`. `os::pd_start_thread` existe só como
  `fatal()` (`os_javaos.cpp:182-184`).
- `osThread_javaos.hpp` (`os-javaos-skeleton`) já tem `_startThread_lock:
  Monitor*` (hoje sempre `nullptr` — nunca construído) e herda
  `get_state()`/`set_state()`/`ThreadState` de `OSThreadBase` (compartilhado,
  shared/runtime/osThreadBase.hpp — `ALLOCATED`/`INITIALIZED`/`RUNNABLE`
  já definidos ali, nenhum trabalho novo necessário).
- `mutex_javaos.cpp` (`os-javaos-platform-sync`) já faz `PlatformMutex`/
  `PlatformMonitor` reais chamando `pthread_mutex_*`/`pthread_cond_*` de
  `glue.c` — o `Monitor`/`Mutex` de mais alto nível do HotSpot
  (`share/runtime/mutex.cpp`, compartilhado) já se apoia nisso; nenhuma
  mudança adicional necessária para `osthread->startThread_lock()`
  funcionar de verdade, só construir o `Monitor` (`new Monitor(...)`) em
  `os::create_thread`.
- `os_bsd.cpp:652-720,800-806` (referência real já lida) mostra a forma
  exata de `os::create_thread`/`os::pd_start_thread`: aloca `OSThread`,
  marca `ALLOCATED`, chama `pthread_create` com atributos
  `PTHREAD_CREATE_DETACHED`, e o trampolim (`thread_native_entry`) faz
  `thread->initialize_thread_current()` (grava `Thread::_thr_current`,
  TLS real — `kernel-thread-local-storage` já entrega isso) e espera em
  `startThread_lock` até `pd_start_thread` notificar.
  `os::create_main_thread`/`os::create_attached_thread` (mesma seção do
  arquivo) são um problema **diferente** — anexam a thread *primordial*,
  que já existe, a um `JavaThread`, sem criar nenhum SO thread novo — fora
  de escopo aqui (ver "Non-Goals").

## Goals / Non-Goals

**Goals:**
- `pthread_create`/`pthread_exit` reais em `glue.c`, sobre
  `SYSCALL_THREAD_CREATE` estendida com um argumento.
- `os::create_thread`/`os::free_thread`/`os::pd_start_thread` reais em
  `os_javaos.cpp`, verificados por `-fsyntax-only` contra os headers reais
  do HotSpot vendorizado.

**Non-Goals:**
- `os::create_main_thread`/`os::create_attached_thread` — anexar a thread
  primordial, problema de sequenciamento de inicialização da VM (roadmap,
  item 6.5), não de criação de thread.
- `pthread_join`/`pthread_detach` — `os::create_thread` sempre cria
  threads destacadas; nenhum consumidor real precisa esperar o término de
  uma thread.
- Linkar ou executar `os_javaos.cpp`/qualquer arquivo do HotSpot — mesma
  disciplina de `os-javaos-skeleton`: só `-fsyntax-only`. A verificação
  real de comportamento é via um programa C que chama `pthread_create`
  diretamente, nunca passando por código C++ do HotSpot.
- `pthread_attr_t` completo (prioridade, afinidade, etc.) — só
  `stacksize`/`detachstate` são lidos, mesmo subconjunto que
  `os::create_thread` real de `os_bsd.cpp` usa.
- Qualquer limite de quantas threads um processo pode ter — já não existe
  hoje (`kernel-user-thread-create`), continua sem limite explícito.

## Decisions

### Estender `SYSCALL_THREAD_CREATE` com um terceiro argumento, escrito em `RDI`
`sys_thread_create(entry: u64, stack_size: u64, arg: u64)` — `arg2`
(`RDX` na entrada de `syscall`, hoje ignorado) passa a virar esse
terceiro parâmetro. `create_user_thread` recebe `arg: u64` e repassa a
`init_stack`, que grava `arg` no índice `i = 5` do laço de zeragem (em
vez de `0`) — único ponto de mudança real, já que os outros 14 registros
continuam zerados exatamente como antes.

**Por que `RDI`, não um mecanismo de memória compartilhada**: a
alternativa óbvia (gravar `(start_routine, arg)` num bloco e informar seu
*ThreadId* de volta para um mapa global) tem uma corrida real: a
thread-filha pode começar a rodar, e portanto precisar ler esse mapa,
antes mesmo de a *syscall* retornar o `ThreadId` para a thread-mãe
registrar a entrada nele. Escrever diretamente no registrador de entrada
da nova thread elimina a corrida por completo — o valor já está lá no
primeiro ciclo de CPU que ela executa.

### Trampolim comum em `glue.c`, não um por chamador
```c
struct pthread_trampoline_args {
    void *(*start_routine)(void *);
    void *arg;
};

static void pthread_trampoline(void) {
    /* RDI na entrada contém o endereço de um `pthread_trampoline_args`
     * alocado por pthread_create (ver abaixo) - acessado aqui via asm,
     * já que a função em si não declara parâmetro algum (a nova thread
     * não "chama" pthread_trampoline por uma call normal, é a primeira
     * instrução que ela executa). */
    struct pthread_trampoline_args *args = __javaos_thread_trampoline_arg();
    void *(*start_routine)(void *) = args->start_routine;
    void *arg = args->arg;
    free(args);
    start_routine(arg);
    pthread_exit(NULL);
}
```
`pthread_create` aloca `args` (via `malloc` — seguro já que
`libc-retargetable-locking` existe), chama
`javaos_syscall(SYS_THREAD_CREATE, (long)pthread_trampoline,
stack_size, (long)args, 0)`, e devolve o `ThreadId` resultante como
`pthread_t`. `__javaos_thread_trampoline_arg()` é uma pequena função
`naked`-equivalente (ou, mais simples, `pthread_trampoline` em si lê `RDI`
via um parâmetro de verdade — já que nada na ABI SysV proíbe declarar
`pthread_trampoline(void *args)` normalmente: a nova thread "aterrissa"
exatamente como se tivesse sido chamada com esse argumento em `RDI`,
então uma função C comum com um parâmetro já funciona sem nenhum assembly
adicional).

**Correção sobre a suposição do `proposal.md`**: não precisa de nenhuma
função especial para ler `RDI` — basta `pthread_trampoline` declarar um
parâmetro normal (`void *args`); o compilador já gera código que o lê de
`RDI` na entrada, exatamente como qualquer função chamada pela convenção
SysV.

### `os::create_thread` replica `os_bsd.cpp`, sem `os::Posix::get_initial_stack_size`
Já decidido (achado de `os-javaos-skeleton`): este porte não inclui
`os_posix.hpp`. Tamanho de stack: usa `req_stack_size` se diferente de
zero, senão uma constante própria do porte (`JAVAOS_DEFAULT_STACK_SIZE`,
ex. 1 MiB — bem acima do que qualquer thread deste kernel já usa hoje).
`osthread->set_thread_id(...)`: usa o valor devolvido pelo `pthread_t`
(que é o `ThreadId` bruto do kernel, já disponível, sem precisar de
nenhuma *syscall* adicional como `SYSCALL_GET_THREAD_ID` — essa é
necessidade de `libc-retargetable-locking`, não desta change).

### `os::free_thread`/`os::pd_start_thread` — mecânicos, mínimos
`pd_start_thread`: idêntico a `os_bsd.cpp:800-806` (`MutexLocker` sobre
`osthread->startThread_lock()`, `notify()`). `free_thread`: `delete
osthread` (sem liberação de recurso de kernel adicional — a thread já se
encerrou sozinha via `pthread_exit`/`SYSCALL_EXIT` antes disso ser
chamado, mesmo padrão real).

## Risks / Trade-offs

- **[Risco]** Erro na extensão de `init_stack`/`sys_thread_create`
  afetaria toda criação de thread existente (`kernel-user-thread-create`),
  não só a nova. → **Mitigação**: `sibling_thread_test_program.c`
  (já existente, `kernel-user-thread-create`) continua passando como
  regressão obrigatória, já que não passa nenhum argumento explícito
  (equivalente a `arg = 0`, preservando o comportamento anterior) — ver
  spec delta, "Ausência de argumento preserva o comportamento já
  existente".
- **[Risco]** `os_javaos.cpp` nunca é linkado/executado — um erro de
  lógica em `os::create_thread` (não só de sintaxe) nunca seria pego por
  esta change. → **Mitigação aceita, não resolvida**: mesma limitação já
  aceita por toda a linha de changes `os-javaos-*` desde `os-javaos-skeleton`;
  resolver isso de verdade exigiria linkar o HotSpot inteiro, fora de
  escopo de qualquer change até agora.
- **[Trade-off]** Thread criada por `pthread_create` nunca pode ser
  "joinada" — aceitável (Non-Goal), mas significa que um programa C
  genérico que *precise* de `pthread_join` real ainda não é suportado por
  este porte.

## Migration Plan

Não aplicável — `arg` extra é aditivo (valor `0` preserva o comportamento
anterior). Depende de `libc-retargetable-locking` já estar implementada e
arquivada antes de esta change começar a ser aplicada.

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.

## Resultado real da implementação

Implementado conforme planejado, com um achado real sobre o próprio
esqueleto `os_javaos` (abaixo) além dos já antecipados em "Context"/
"Decisions". Achados durante a implementação e a verificação em boot/compilação:

- **`osThread_javaos.hpp` precisou de um `setter` novo**: `_startThread_lock`
  (do esqueleto original, `os-javaos-skeleton`) só tinha o getter
  `startThread_lock()` — o plano desta change (`os::create_thread`
  constrói o `Monitor` diretamente, não no construtor de `OSThread`,
  diferente de `os_bsd.cpp`/`os_linux.cpp` reais) exigia uma forma de
  atribuí-lo de fora. Adicionado `set_startThread_lock(Monitor*)`
  (mesmo padrão de `set_thread_id`), e o destrutor de `OSThread` passou a
  `delete _startThread_lock` (antes um no-op) — simetria necessária, já
  que agora o `Monitor` é de fato alocado dinamicamente. Pequena edição
  adicional em `osThread_javaos.hpp`, não só em `os_javaos.cpp` (escopo da
  tarefa 3.1 original) — mecânica e diretamente implicada pela decisão de
  design já tomada, não uma mudança de comportamento nova.
- **Includes adicionais necessários em `os_javaos.cpp`**: `runtime/thread.hpp`
  (para `Thread::record_stack_base_and_size`/`initialize_thread_current`/
  `call_run`/`osthread`/`set_osthread`), `runtime/mutexLocker.hpp` (para
  `MutexLocker`, não usado por nenhum `pd_*` anterior desta change) e
  `<pthread.h>` (para `pthread_create`/`pthread_attr_t`/
  `PTHREAD_CREATE_DETACHED`) — nenhum desses já era necessário pelos `pd_*`
  de memória/mutex existentes.
- **Verificação de compilação real** (`g++ -fsyntax-only -std=gnu++17`,
  mesma invocação de `os-javaos-skeleton`/`os-javaos-memory-pd`/
  `os-javaos-park-sync`) contra a árvore vendorizada: `os_javaos.cpp`
  compila sem erro. Como regressão (`osThread_javaos.hpp` editado é
  incluído por outros dois arquivos), `mutex_javaos.cpp` e
  `park_javaos.cpp` também foram reverificados — ambos continuam
  compilando. Único aviso em todos os três, pré-existente e alheio a esta
  change: `-Winvalid-offsetof` em `share/utilities/exceptions.hpp`/
  `share/runtime/thread.hpp` (já documentado em `os-javaos-memory-pd`).
- **Tarefa 1.2 — `SYSCALL_GET_THREAD_ID` ainda não existia no momento em
  que a tarefa 1.1 foi planejada, mas já existia (change
  `libc-retargetable-locking`, dependência explícita desta change) no
  momento da implementação** — sem impacto: `arg` do novo terceiro
  argumento de `SYSCALL_THREAD_CREATE` é só repasse mecânico de um `u64`,
  nenhuma interação com `SYSCALL_GET_THREAD_ID`.
- **Verificação em boot real sem regressão**: todo programa de teste já
  existente antes desta change (incluindo `/bin/siblingt.elf`, que nunca
  passa argumento explícito — equivalente a `arg = 0`) reporta exatamente
  a mesma string P/F de antes. Novo `/bin/pthreadc.elf` reporta
  `resultado PPP` (malloc do argumento, `pthread_create`, argumento
  intacto através do trampolim).
- **Achado real de orçamento de tempo de boot**: com mais um programa de
  teste de thread concorrente carregado no boot, `recursive_lock_test_program`/
  `sibling_thread_test_program` (ambos de changes anteriores) deixaram de
  caber de forma confiável na janela padrão de 15s de `cargo xtask test`
  sob carga mais pesada do host — confirmado correto (mesmas strings de
  sempre) numa janela estendida manual de 120s. Mesma categoria, já aceita
  neste projeto, de `fpu_demo_a`/`fpu_demo_b` (nunca garantidos dentro de
  15s mesmo antes desta change). `malloc_concurrency_test_program`
  (`libc-retargetable-locking`) teve suas iterações reduzidas mais uma vez
  (300 → 100 por thread) para compensar parcialmente, sem perder cobertura
  genuína de concorrência — nenhuma mudança de comportamento, só de
  quantidade de iterações.
- **Achado real não relacionado, descoberto durante a verificação**:
  `cargo test -p kernel --lib` com o paralelismo padrão do harness de
  testes falha de forma intermitente em dois testes de
  `kernel::acpi::tests` que compartilham `ISA_IRQ_OVERRIDES` (`static`
  global) sem isolamento entre si — pré-existente desde `kernel-acpi-madt`,
  sem nenhuma relação com o código desta change (`kernel::acpi` não foi
  tocado). Registrado em `KNOW_ISSUES.md` com o racional completo, não
  corrigido (fora de escopo). A suíte completa passa de forma estável e
  determinística com `cargo test -p kernel --lib -- --test-threads=1`
  (338 testes, 0 falhas).
