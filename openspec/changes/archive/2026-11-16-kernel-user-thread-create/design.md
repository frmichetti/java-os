# Design

## Context

Ver `proposal.md` — "Why" para a motivação (bloqueio estrutural para o porte
do HotSpot).

Estado atual confirmado em código (não suposição):

- `kernel::process::Process { id, address_space, threads: Vec<ThreadId> }`
  (`process.rs:226`) já tem o campo `threads`, mas hoje todo processo nasce
  com exatamente uma entrada nele e nunca ganha uma segunda.
- `Process::spawn_user_thread` (`process.rs:350-362`) já existe, já faz
  exatamente o que esta change precisa (mapeia uma nova stack de usuário no
  `root` existente do processo via `create_user_thread`/`Mapper::for_root`,
  aloca uma nova stack de kernel via `stack::alloc_thread_stack`, e dá
  `push` no `ThreadId` em `self.threads`) — mas é código morto hoje, nunca
  chamado de `scheduler.rs` nem de nenhuma syscall.
- `kernel::scheduler::PROCESSES: Spinlock<Vec<Process>>` (`scheduler.rs:185`)
  só recebe `push` (6 call sites, uma por tipo de spawn existente); nada
  nele hoje faz busca por `ProcessId` — é indexado implicitamente por ordem
  de criação.
- `pthread_handles`/`file_handles`/`vmm` já são todos indexados por
  `ProcessId` (não por `ThreadId`), desde a change `kernel-handle-ownership`
  — duas threads com o mesmo `ProcessId` automaticamente compartilham
  mutexes/condvars/arquivos/reservas, sem nenhuma mudança de código.
- O maior número de syscall hoje é `SYSCALL_MUTEX_TRYLOCK = 22`
  (`syscall.rs:100`), despachado em `dispatch` (`syscall.rs:593-...`).
- A limpeza de término de thread em `schedule_next` (`scheduler.rs:795-835`)
  roda incondicionalmente dentro de `if is_terminated(was_running)`:
  libera TODOS os frames do espaço de endereçamento
  (`AddressSpace::free_user_mappings`), fecha todo arquivo
  (`file_handles::close_all_owned_by`) e libera toda reserva de VMM
  (`vmm::release_all_owned_by`) do processo dono dessa thread. Isso só é
  seguro hoje porque processo e thread são 1:1 — não existe, em nenhum
  ponto do código atual, uma segunda thread viva no mesmo `ProcessId`.

## Goals / Non-Goals

**Goals:**
- Uma nova chamada de sistema que adiciona uma thread de usuário a um
  processo já existente, reaproveitando `Process::spawn_user_thread`.
- Um alocador de stack de usuário para essa thread adicional (hoje só
  existe constante de compilação fixa por programa de teste).
- Corrigir a limpeza de término em `schedule_next` para só rodar quando a
  thread terminada é a última viva do seu processo.
- Um programa de teste em `userland_c/` provando concorrência real entre
  duas threads do mesmo processo, compartilhamento de handles, e que o
  término de uma não afeta a outra.

**Non-Goals:**
- Wirear `os_javaos::pd_start_thread` ou `pthread_create` da libc sobre
  esta syscall (change seguinte, mesmo padrão de `os-javaos-platform-sync`
  sobre `kernel-thread-sync`).
- Qualquer destruição real de `pthread_handles` (mutex/condvar) — já é
  Non-Goal herdado de `kernel-pthread-sync`/`kernel-handle-ownership`, sem
  mudança aqui.
- Reciclagem de slot de stack de usuário ou de kernel ao terminar uma
  thread — `kernel::stack` já nunca recicla slots de stack de kernel; o
  novo alocador de stack de usuário segue a mesma política.
- Migrar uma thread entre processos, ou qualquer forma de "thread órfã"
  sobrevivendo à troca do processo dono.

## Decisions

### Reaproveitar `Process::spawn_user_thread` como está
Já implementa corretamente mapear a stack de usuário no `root` do processo
via `Mapper::for_root` (não `Mapper::new` — distinção importante: a stack
de usuário vive na metade inferior do endereçamento, privada por processo,
ao contrário da stack de kernel, que vive na metade superior
compartilhada). Não precisa de nenhuma mudança; só precisa passar a ser
chamada.

### Nova syscall `SYSCALL_THREAD_CREATE = 23`
Segue a convenção já existente: próximo número livre após
`SYSCALL_MUTEX_TRYLOCK = 22`, despachado em `dispatch` (`syscall.rs:593`).
Argumentos: `arg0` = ponto de entrada (endereço virtual), `arg1` = tamanho
de stack pedido. O handler:
1. Localiza o `Process` do chamador em `PROCESSES` por `CURRENT_PROCESS_ID`
   (nova função de busca — ver abaixo).
2. Valida `arg0` com `Mapper::validate_user_range` (mesmo mecanismo de
   `kernel-user-pointer-validation`), exigindo página executável e
   acessível por usuário → `EFAULT` se falhar.
3. Valida `arg1 != 0` e dentro do espaço livre da janela de stacks de
   usuário → `EINVAL` se falhar.
4. Aloca o próximo slot da nova janela de stack de usuário (ver abaixo),
   chama `Process::spawn_user_thread`, registra a nova `Thread` em
   `CONTEXTS`/`READY_QUEUE` do escalonador (mesmo padrão já usado por
   `spawn_from_elf`/`spawn_sharing_current_address_space` para registrar
   uma `Thread` recém-criada), devolve o `ThreadId` como `u64`.

### Nova função de busca de `Process` por `ProcessId` em `PROCESSES`
`PROCESSES` hoje só recebe `push`. Adiciona uma função
`fn find_process_mut(id: ProcessId) -> ... ` (closure sobre o `MutexGuard`
já tomado, já que `PROCESSES` é `pub(crate)` dentro de `scheduler.rs`) que
itera linearmente procurando `p.id == id`. Linear é aceitável: o número de
processos simultâneos neste kernel é pequeno (programas de teste/HotSpot
embarcado, não um sistema multiusuário), e já é o padrão usado em buscas
análogas (`file_handles`/`vmm` por `ProcessId`). Esta mesma função também
resolve a busca necessária para o gate de "última thread" da correção de
limpeza abaixo.

### Janela de stack de usuário dedicada a threads adicionais, com contador global monotônico
Nova constante (análoga a `THREAD_STACK_POOL_BASE` em `kernel::stack`, mas
na metade inferior do endereçamento, privada por processo):
`USER_THREAD_STACK_POOL_BASE = 0x0000_0f00_0000_0000` — ver "Resultado real
da implementação" abaixo para o porquê deste valor exato, distinto do
`0x0000_1000_0000_0000` originalmente cogitado aqui.

Um único contador `AtomicU64` **global** (não por processo) numera os
slots. Isso é seguro mesmo sendo global: dois processos diferentes têm
`CR3`/PML4 diferentes, então o mesmo endereço virtual de slot nunca colide
entre processos — só colidiria entre duas threads adicionais *do mesmo
processo* pedindo stack ao mesmo tempo, e o contador é atômico. Mesma
política de "nunca recicla" que `NEXT_THREAD_STACK_SLOT` já usa para
stacks de kernel — sem reclamação por thread individual (Non-Goal).

**Alternativa considerada e rejeitada:** um contador por processo (campo
novo em `Process`). Rejeitada porque exigiria mudar `Process`/`PROCESSES`
e a lógica de `AddressSpace::new()` (que hoje copia por valor as entradas
de PML4 256-511 de um processo "molde" — ver achado documentado em
`kernel::stack::ensure_pool_mapped`); o contador global evita tocar nesse
mecanismo e é suficiente dado que os slots nunca são reciclados de qualquer
forma.

### Gate de "última thread do processo" na limpeza de término
Em `schedule_next` (`scheduler.rs:795`), antes de rodar as três chamadas de
limpeza existentes (`free_user_mappings`/`close_all_owned_by`/
`release_all_owned_by`), remove o `ThreadId` de `was_running` de
`process.threads` (usando a busca por `ProcessId` acima) e só executa as
três chamadas se `process.threads` ficar vazio depois dessa remoção. Caso
contrário, nenhuma das três roda — a thread-irmã continua com memória e
handles intactos.

Processos de uma única thread (todo caso hoje existente) continuam se
comportando exatamente como antes: a remoção sempre esvazia
`process.threads` nesse caso, preservando 100% do comportamento observável
anterior — nenhuma das specs já existentes muda de resultado para esse
caso, só o caso novo (múltiplas threads) passa a ser coberto.

### Programa de teste de thread-irmã
Novo `userland_c/sibling_thread_test_program.c`: a thread principal cria
um mutex nomeado e um arquivo aberto (handles de processo), invoca
`SYSCALL_THREAD_CREATE` passando um ponto de entrada que opera sobre esse
mesmo mutex/arquivo, espera a thread-irmã terminar (via uma variável
compartilhada em memória, já que não há `pthread_join` nesta change —
Non-Goal), e verifica que (a) ambas realmente executaram (contador
compartilhado incrementado por ambas), (b) o mutex/arquivo continuam
utilizáveis pela thread principal depois que a thread-irmã termina via
`SYSCALL_EXIT`, e (c) a thread principal, ao terminar por último, ainda
consegue acionar a limpeza completa sem travar.

## Risks / Trade-offs

- **[Risco]** Sem o gate de "última thread", a própria introdução desta
  syscall tornaria `schedule_next` destrutivo para qualquer processo
  multithread (liberaria a memória/handles de uma thread-irmã ainda viva).
  → **Mitigação:** gate implementado e coberto por cenário dedicado no
  delta spec de `kernel-processes` e `kernel-handle-ownership`
  ("Thread que termina não é a última do seu processo não libera nada").
- **[Trade-off]** `switch_address_space` continua trocando `CR3`
  incondicionalmente a cada troca de contexto, mesmo entre duas threads do
  mesmo processo (mesmo `root`) — recarga de TLB desnecessária nesse caso.
  Aceito como já era antes desta change (trade-off de desempenho
  pré-existente, não uma regressão introduzida aqui; há precedente
  documentado em `scheduler.rs` de que trocar isso para evitar recargas
  desnecessárias já causou uma IRQ de teclado perdida em teste).
- **[Risco]** Busca linear em `PROCESSES` por `ProcessId` a cada término de
  thread adiciona uma travessia O(n) no caminho crítico do escalonador. →
  **Mitigação:** aceitável dado o número pequeno de processos simultâneos
  neste kernel (não um SO multiusuário de propósito geral); mesma política
  já aceita para buscas por `ProcessId` em `file_handles`/`vmm`.
- **[Risco]** Sem `pthread_join` nesta change, o programa de teste precisa
  de uma forma ad-hoc (variável compartilhada) de esperar a thread-irmã
  terminar antes de verificar o estado final — não é uma primitiva de
  sincronização geral. → **Mitigação:** aceitável para um teste de kernel;
  `pthread_join` real fica para a change que wirear `os_javaos`/libc sobre
  esta syscall.

## Migration Plan

Não aplicável — nova capacidade aditiva, sem migração de dados nem
compatibilidade retroativa a preservar além da já garantida pelo gate de
"última thread" (processos de uma thread continuam idênticos).

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.

## Resultado real da implementação

- **`stack::next_user_thread_stack_base` não mapeia nada, só reserva o
  endereço:** a primeira versão desta função mapeava a stack diretamente
  (como `kernel::stack::alloc_thread_stack` faz para stacks de kernel), mas
  isso duplicava o mapeamento já feito por `Process::spawn_user_thread`/
  `create_user_thread` (já existente, ver "Context" acima) — a segunda
  tentativa de mapear a mesma página falharia (`map_new` com
  `allow_replace: false`). Corrigido para `next_user_thread_stack_base`
  devolver só o endereço virtual base do próximo slot (sem tocar
  `FrameAllocator`/`Mapper`), que `sys_thread_create` repassa para
  `Process::spawn_user_thread` mapear como já fazia antes desta change.
- **`0x0000_1000_0000_0000` não estava livre:** a suposição original desta
  seção ("Janela de stack de usuário...") de que esse endereço era
  "claramente distinto de toda constante já existente" estava errada — é
  exatamente `USER_DEMO_CODE_VIRT` em `kernel::scheduler`. Inspecionar
  todas as constantes `*_CODE_VIRT`/`*_STACK_VIRT` de `scheduler.rs` mostrou
  que a faixa inteira de incrementos de `0x0000_0100_0000_0000` entre
  `0x0000_0100_...` e `0x0000_7000_...` já está ocupada por programas de
  teste existentes. Corrigido para `USER_THREAD_STACK_POOL_BASE =
  0x0000_0f00_0000_0000` — o primeiro incremento livre dessa mesma faixa
  (entre `EFAULT_TEST_PROGRAM_STACK_VIRT = 0x0000_0e00_...` e
  `USER_DEMO_CODE_VIRT = 0x0000_1000_...`), com `USER_THREAD_STACK_SLOT_STRIDE`
  (~8 MiB) de sobra antes do próximo endereço já em uso.
- **Verificação em boot real (QEMU/OVMF) confirmou o cenário completo**:
  `/bin/siblingt.elf` reportou `sibling_thread_test_program: resultado
  PPPPPPPP (ordem: mutex_create,file_open,thread_create,join_poll,
  irma_leu_primeiro_byte,mutex_apos_irma,arquivo_cursor_compartilhado,
  file_close)` — a thread-irmã criada por `SYSCALL_THREAD_CREATE` realmente
  executa (compartilhando o espaço de endereçamento e os handles de
  mutex/arquivo do processo), e depois que ela termina via `SYSCALL_EXIT`,
  a thread principal continua usando normalmente o mesmo mutex e o mesmo
  arquivo (cursor realmente avançado pela leitura da thread-irmã), sem
  nenhum travamento ao encerrar por último. Todos os demais programas de
  teste já existentes (`file_syscalls_demo`, `libc_test_program`,
  `cxx_test_program`, `javaos_vm_test_program`,
  `javaos_pthread_test_program`, `libm_test_program`,
  `timedwait_test_program`, `efault_test_program`, `user_posix_demo`,
  `fpu_demo_a`/`fpu_demo_b`, `safepoint`, `wait_queue`, `pthread`)
  reportaram exatamente as mesmas strings P/F de antes desta change, e o
  único `#PF` real do boot continua sendo o deliberado de
  `fault_test_program.c` — confirmando que o gate de "última thread" não
  alterou nenhum comportamento de processo de uma única thread.
