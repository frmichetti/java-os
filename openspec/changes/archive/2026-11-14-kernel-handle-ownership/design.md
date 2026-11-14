# Design

## Context

`kernel::pthread_handles`/`kernel::file_handles`/`kernel::vmm` são tabelas globais
sem nenhum rastreamento de dono — `kernel::pthread_handles::create_mutex()`/
`create_cond()` não recebem nenhum parâmetro; `kernel::file_handles::register
(contents)` só recebe o conteúdo; `kernel::vmm::Vmm::reserve(size)` só recebe o
tamanho. Nenhuma das três sabe, nem tem como saber hoje, qual processo fez a
chamada. `kernel::scheduler::register_thread` já recebe `thread.process:
ProcessId` (dentro do `Thread` passado por valor), mas descarta esse campo —
`contexts[index] = thread.context;` só copia o contexto de registradores, nunca o
`ProcessId`. Ver proposal.md para a motivação completa (vazamento entre
processos + acesso entre processos hoje possível) e o porquê de
`kernel::pthread_handles` ficar de fora da liberação real nesta change.

`kernel::scheduler::schedule_next` já tem o ponto de limpeza de uma thread
terminada (`kernel-process-cleanup`, que chama `AddressSpace::
free_user_mappings` ali) — esta change só adiciona mais chamadas de limpeza no
mesmo ponto, para os dois recursos que já têm uma operação de fechamento/
liberação segura e existente (`file_handles::close`/`Vmm::release`).

## Goals / Non-Goals

**Goals:**
- `kernel::process::Context` ganha `process: ProcessId`, populado em
  `register_thread` a partir de `thread.process` (hoje descartado) — mesmo
  padrão incremental já usado para `fpu_state`/`syscall_rsp_slot`/`timed_out`.
- `kernel::scheduler::current_process_id() -> ProcessId`, mesmo papel de
  `current_thread_id()` já existente.
- `pthread_handles`/`file_handles`/`vmm` passam a exigir o `ProcessId` dono na
  criação e na consulta — rejeitando qualquer consulta cujo chamador não seja o
  dono, com o mesmo sentinela de erro de "handle desconhecido" já usado hoje
  (nenhum código de erro novo, nenhuma assinatura de syscall muda).
- Ao terminar, um processo tem seus arquivos fechados (`file_handles::close`) e
  reservas de memória virtual liberadas (`Vmm::release`) — reaproveitando essas
  operações já existentes e já testadas, chamadas a partir do mesmo ponto de
  `schedule_next` que já libera o espaço de endereçamento.

**Non-Goals:**
- Liberar de verdade os `Mutex`/`CondVar` de `kernel::pthread_handles` — ver
  Decisions, "Por que `pthread_handles` continua sem destruição real".
- Qualquer mudança de assinatura de syscall ou código de retorno novo — a
  rejeição por posse reaproveita exatamente o sentinela de erro que já existe
  para "handle desconhecido" em cada chamada.
- Reaproveitar slots de handle liberados (`file_handles`/`pthread_handles`
  continuam só crescendo, mesma política já documentada em cada um) — mudar
  isso é ortogonal a rastrear posse, e mexeria num Non-Goal já decidido em
  changes anteriores sem necessidade.
- Qualquer coordenação entre múltiplas threads do mesmo processo compartilhando
  os mesmos handles — nenhum processo de modo usuário tem mais de uma thread
  hoje (mesma limitação já documentada em changes anteriores).

## Decisions

### `current_process_id()` lê uma global leve (`CURRENT_PROCESS_ID`), nunca `with_interrupts_disabled`

**Achado real, correção de design feita durante a implementação**: a primeira
versão de `current_process_id()` chamava `current_thread_id()` (que usa
`scheduler::with_interrupts_disabled`, ou seja, executa `cli`) e então
consultava `CONTEXTS.lock()[id.0 as usize].process`. Isso **derrubou a suíte de
testes de host inteira com `SIGSEGV`** — todo teste de `kernel::syscall` que
chama `dispatch(SYSCALL_MUTEX_CREATE, ...)`/`dispatch(SYSCALL_COND_CREATE,
...)` diretamente (vários já existentes, incluindo os nomeados
"..._succeeds_without_touching_the_scheduler") passou a executar `cli` — uma
instrução privilegiada, que causa `#GP`/`SIGSEGV` em anel 3 (o processo de
teste em host). Como toda `sys_*` que cria ou consulta um handle agora precisa
de `current_process_id()` incondicionalmente (mesmo no caminho de rejeição,
que antes nunca tocava o escalonador), isso quebraria dezenas de testes já
existentes, não só um caso de borda.

**Correção**: `current_process_id()` passa a ler diretamente
`CURRENT_PROCESS_ID` — uma nova `static mut u64` em `kernel::scheduler`,
atualizada por `schedule_next`/`register_boot_thread` a cada troca de
contexto, exatamente o mesmo padrão já usado por `CURRENT_FPU_AREA`/
`CURRENT_SYSCALL_RSP_SLOT`. Uma leitura direta de `static mut` não usa
nenhuma instrução privilegiada — segura em qualquer anel, inclusive no
processo de teste em host. **Por que uma única global global basta aqui** (ao
contrário de `CURRENT_SYSCALL_RSP_SLOT`, que precisou virar por-thread — ver
`KNOW_ISSUES.md`): `current_process_id()` é sempre lida uma única vez, de
forma síncrona, bem no início de cada `sys_*`, antes de qualquer bloqueio
possível — o valor usado para a checagem de posse nunca precisa sobreviver a
uma troca de contexto dentro da mesma chamada (diferente do `RSP` de usuário,
que precisa ser "emprestado" durante potencialmente vários ticks enquanto uma
syscall bloqueante está em andamento).

`kernel::process::Context` continua ganhando o campo `process: ProcessId`
(ver decisão abaixo) — não para `current_process_id()` (que usa a global
leve), mas para a limpeza ao terminar (tarefa 5.2: `contexts[was_running.0 as
usize].process`, precisa do processo da thread que *acabou de terminar*, não
da que está *prestes a rodar* — a global `CURRENT_PROCESS_ID` só reflete esta
última).

### `ProcessId` guardado no `Context`, não numa tabela de consulta separada

Em vez de criar uma tabela `ThreadId → ProcessId` à parte, o `ProcessId` vira só
mais um campo do `Context` já indexado por `ThreadId` — exatamente o mesmo lugar
de onde `fpu_state`/`syscall_rsp_slot`/`timed_out` já são lidos a cada troca de
contexto/*syscall*. **Alternativa rejeitada**: uma tabela separada
`Spinlock<Vec<ProcessId>>` indexada por `ThreadId` — rejeitada por duplicar uma
estrutura que já existe (`CONTEXTS`), sem nenhum benefício; o próprio `Process`
em si (`PROCESSES`) nunca precisa ser consultado para descobrir o dono de uma
thread, só o `Context` dela.

### Cada tabela ganha um parâmetro `owner`/campo de posse, não uma tabela por processo

`pthread_handles`/`file_handles` continuam sendo um único `Vec` global cada,
agora guardando `(ProcessId, T)` em vez de só `T` — `vmm::ReservationState` ganha
um campo `owner: ProcessId`. **Alternativa rejeitada**: dar a cada `Process` sua
própria tabela de handles (`Process.mutexes: Vec<...>`, etc.) — mais fiel ao
nome "isolar... por processo" do `ROADMAP.md`, mas exigiria mudar a forma do
`handle` em si (hoje um índice plano, só comparável dentro da tabela global) ou
inventar uma codificação `(ProcessId, índice-local)` dentro do `u64` de 64 bits
que as *syscalls* já usam — uma mudança de contrato observável desnecessária,
quando o mesmo resultado (um processo nunca opera sobre o handle de outro) é
alcançável só verificando o dono a cada consulta, sem mudar o que um `handle`
*é*.

### Rejeição reaproveita o sentinela de "handle desconhecido", nunca um código novo

`mutex(handle, caller)`/`cond(handle, caller)`/`read(handle, len, caller)`/
`close(handle, caller)` passam a devolver `None`/`false` tanto para um `handle`
que nunca existiu quanto para um que existe mas pertence a outro processo —
exatamente o que cada `sys_*` já traduz para `SYSCALL_ERROR` hoje. Do ponto de
vista de um programa de modo usuário bem-comportado (só usa seus próprios
handles), nenhum comportamento muda. **Alternativa rejeitada**: um sentinela de
erro novo e distinto para "handle de outro processo" (ex.: `SYSCALL_FORBIDDEN`) —
rejeitada por ser uma mudança de contrato desnecessária; a spec (`kernel-syscalls`)
já trata "handle desconhecido" como a categoria correta de erro, e um processo
nunca tem como saber se um handle é "desconhecido" ou "de outro processo" de
qualquer forma útil (ambos "não é meu").

### Por que `pthread_handles` continua sem destruição real

Reclamar a memória de um `&'static Mutex`/`&'static CondVar` alocado via
`Box::leak` exigiria `unsafe { Box::from_raw(ptr) }` — seguro **somente** se
nenhuma outra referência `&'static` a esse objeto específico ainda circula em
algum lugar do kernel no momento da liberação. Hoje, toda referência a um desses
objetos vem de uma consulta momentânea à tabela (`mutex(handle)`/`cond(handle)`,
usada e descartada dentro da mesma chamada de sistema) — nenhum código guarda
uma cópia de longo prazo. Isso **sugere** que seria seguro, mas provar isso de
verdade (sem nenhum caminho que guarde uma referência além do escopo de uma
única *syscall*) é uma auditoria própria, maior que "adicionar rastreamento de
dono" — e já era um Non-Goal explícito desde `kernel-pthread-sync` ("destroy
devolve sucesso sem liberar o objeto no kernel"). Esta change não revisita essa
decisão: `pthread_handles` ganha posse e rejeição de acesso entre processos
(a parte de isolamento), mas continua vazando os próprios objetos (a parte de
liberação), documentado como vazamento residual conhecido.

## Risks / Trade-offs

- **[Risco] Um erro na checagem de posse poderia rejeitar um processo operando
  sobre seu próprio handle legítimo** → Mitigação: a checagem é sempre "dono
  salvo na criação == chamador agora", nunca uma lógica mais complexa — testável
  em host sem hardware (comparação pura de `ProcessId`), mesma categoria de teste
  já usada para `handle` desconhecido em cada tabela.
- **[Risco] `pthread_handles` continua vazando os próprios objetos — o maior
  vazamento residual depois desta change** → Aceito, documentado como Non-Goal;
  revisitar exigiria uma auditoria de todo caminho que possa reter uma
  referência `&'static Mutex`/`&'static CondVar` além do escopo de uma única
  *syscall*, fora do escopo aqui.
- **[Risco] Qualquer `sys_*` nova no futuro que crie ou consulte um handle
  precisa lembrar de passar `current_process_id()`** → Mitigação parcial: a
  assinatura de cada função de tabela (`create_mutex(owner: ProcessId)`, etc.)
  torna isso um erro de compilação (parâmetro obrigatório), não um detalhe fácil
  de esquecer silenciosamente.

## Migration Plan

Puramente aditivo para todo programa que só usa seus próprios handles (nenhuma
mudança observável) — o único comportamento novo é a rejeição de um acesso entre
processos, que hoje nunca é exercido por nenhum programa de teste existente (não
existe hoje nenhum jeito de dois processos distintos sequer descobrirem o handle
um do outro, exceto adivinhação deliberada, usada só pelo novo programa de teste
desta change). Nenhum passo de migração necessário.

## Resultado real da implementação

**Testes de host**: `cargo test -p kernel --lib` passou com 279 testes (276 de
antes desta change + 3 novos de `vmm`, somados aos já contabilizados por
`pthread_handles`/`file_handles` nas tarefas 2.1/3.1/3.2), 0 falhas, sem
nenhum `SIGSEGV` — confirmando a correção de `current_process_id()` descrita
acima (Decisions, "`current_process_id()` lê uma global leve").

**Boot real** (`cargo xtask test`, harness padrão de 15s): `smoke test de
boot: PASSOU`. Todo programa de teste já existente reportou exatamente o
mesmo padrão de resultado de antes desta change (`file_syscalls_demo:
resultado PPPPPPPP`, `javaos_pthread_test_program: resultado PPPPPPPPPP`,
`javaos_vm_test_program: resultado PPPPPPPP`, `user_posix_demo: resultado
PPPPPPPPPPPP`, `timedwait_test_program: resultado PP`, `safepoint: 51607
leituras completadas sem halt... PASSOU`, `pthread: contador final=8000...
PASSOU`, etc.) — nenhuma regressão para uso legítimo. A liberação ao terminar
apareceu no log exatamente como esperado: `kernel_handle_ownership: thread 9
fechou 0 arquivo(s) e liberou 1 reserva(s) de memoria virtual` (thread
coordenadora de `kernel::safepoint_demo`, liberando a página de poll
reservada) e `kernel_handle_ownership: thread 8 fechou 0 arquivo(s) e liberou
1 reserva(s) de memoria virtual` (thread de `javaos_vm_test_program`,
liberando sua última reserva viva), ambas logo após o diagnóstico já
existente de `kernel_process_cleanup` (frames físicos) para a mesma thread.

**Achado real, fora do escopo desta change**: ao investigar com uma janela de
boot manualmente estendida para 30s (bem além do harness padrão de 15s usado
pela tarefa 6.1), apareceu um panic reproduzível e não relacionado ao código
desta change: `kernel::clock::cycles_to_nanos` estoura `u64` (`attempt to
multiply with overflow`) porque `kernel::clock::calibrate_tsc()` — pré-existente,
da change `kernel-wall-clock` — mediu uma frequência de TSC de ~23,4 GHz
(fisicamente implausível) sob contenção do host durante esta sessão de
desenvolvimento. Causa raiz completa, instrumentação usada para confirmá-la e
as duas correções pendentes (calibração robusta + aritmética de 128 bits em
`cycles_to_nanos`) estão registradas em `KNOW_ISSUES.md`. Não bloqueia esta
change: o critério real da tarefa 6.1 (harness padrão `cargo xtask test`, 15s)
nunca expôs o problema, e a causa raiz está inteiramente em `kernel::clock`,
sem nenhuma relação com `pthread_handles`/`file_handles`/`vmm`/o ponto de
limpeza do escalonador adicionados aqui.
