//! Escalonador preemptivo round-robin do kernel — ver `design.md` da change
//! `kernel-process-scheduler`.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::phys::{FrameAllocator, FrameStore};
use crate::process::{self, Context, Process, ProcessError, ProcessId, Thread, ThreadId};
use crate::sync::Spinlock;

/// Fila circular de todas as threads escalonáveis, em ordem round-robin — a
/// "frente" é a que está rodando (ou a próxima a rodar). Lógica pura (sem
/// hardware), testável diretamente em host.
///
/// Dividida em passos menores (`pop_front`/`push_back`/`front`) — em vez do
/// `advance()` monolítico original, que sempre recolocava a thread
/// interrompida no fim — para que `on_timer_tick` tenha um ponto de decisão
/// explícito sobre o destino dela (recolocar no fim, comportamento padrão de
/// qualquer tick normal; ou desviá-la para uma `WaitQueue`, quando há uma
/// intenção de bloqueio pendente — ver `kernel::wait_queue` e design.md da
/// change `kernel-wait-queue`).
struct ReadyQueue {
    order: VecDeque<ThreadId>,
}

impl ReadyQueue {
    const fn new() -> Self {
        Self { order: VecDeque::new() }
    }

    fn push_back(&mut self, id: ThreadId) {
        self.order.push_back(id);
    }

    /// Remove e devolve a thread atualmente na frente (a que estava
    /// rodando) — `None` se não houver nenhuma thread registrada (fila
    /// vazia).
    fn pop_front(&mut self) -> Option<ThreadId> {
        self.order.pop_front()
    }

    /// A próxima thread a rodar (a nova frente da fila), sem removê-la —
    /// `None` se a fila estiver vazia (nenhuma thread pronta).
    fn front(&self) -> Option<ThreadId> {
        self.order.front().copied()
    }
}

static READY_QUEUE: Spinlock<ReadyQueue> = Spinlock::new(ReadyQueue::new());

/// Topo da stack de kernel da thread corrente — mesma informação que
/// `TSS.rsp0`, mas acessível diretamente pela rotina de entrada de
/// `syscall` (`kernel::syscall`), que não passa pela troca automática de
/// stack via TSS (só interrupções com troca de privilégio usam `TSS.rsp0`
/// automaticamente — `syscall` não). Atualizada no mesmo ponto que
/// `TSS.rsp0`, nunca separadamente — ver `design.md` da change
/// `kernel-syscalls`, "Stack de kernel confiável".
#[unsafe(no_mangle)]
static mut CURRENT_KERNEL_STACK_TOP: u64 = 0;

/// Endereço da área de `FXSAVE` (`kernel::fpu::FxSaveArea`) da thread
/// corrente — consultada diretamente por nome pelas rotinas assembly
/// `timer_entry`/`block_entry` (`FXSAVE`/`FXRSTOR [CURRENT_FPU_AREA]`),
/// atualizada por `schedule_next` no mesmo trecho que já atualiza
/// `TSS.rsp0`/`CR3`. Ver design.md da change `kernel-fpu-context-switch`.
#[unsafe(no_mangle)]
static mut CURRENT_FPU_AREA: u64 = 0;

/// Endereço do "slot" (um `u64` alocado via `Box::leak`, mesma disciplina de
/// `kernel::fpu::new_area`) onde a rotina assembly `syscall_entry`
/// (`kernel::syscall`) guarda/restaura o `RSP` de modo usuário da thread
/// corrente ao redor de `syscall`/`sysret` — consultada diretamente por nome
/// por aquela rotina, atualizada por `schedule_next` no mesmo trecho que já
/// atualiza `CURRENT_FPU_AREA`. **Por-thread, não uma única variável global**
/// — ver `KNOW_ISSUES.md`, "`SYSCALL_USER_RSP_SCRATCH` é uma variável global
/// única...": uma syscall bloqueante (`WaitQueue::commit_block`) cede a CPU
/// para uma thread totalmente diferente, que também pode fazer suas próprias
/// syscalls antes da primeira ser despertada — uma única `static` global
/// usada por ambas seria sobrescrita, devolvendo à primeira o `RSP` de
/// usuário de outra thread ao final de sua chamada.
#[unsafe(no_mangle)]
static mut CURRENT_SYSCALL_RSP_SLOT: u64 = 0;

/// `ProcessId.0` (valor cru) do processo dono da thread corrente — mesmo
/// padrão de `CURRENT_FPU_AREA`/`CURRENT_SYSCALL_RSP_SLOT` (atualizada por
/// `schedule_next`/`register_boot_thread` a cada troca de contexto), mas
/// nunca consultada por nenhuma rotina assembly — só por
/// [`current_process_id`]. **Por que uma leitura direta de global, não
/// `CONTEXTS.lock()[current_thread_id().0 as usize].process` sob
/// `with_interrupts_disabled`**: `current_process_id()` é chamada por toda
/// `sys_*` que cria/consulta um handle (change `kernel-handle-ownership`),
/// inclusive nos testes de host já existentes de `kernel::syscall` que
/// chamam `dispatch` diretamente — `with_interrupts_disabled` executa `cli`,
/// instrução privilegiada que sempre causa `SIGSEGV` em anel 3 (confirmado
/// ao rodar a suíte: todo teste que chama `SYSCALL_MUTEX_CREATE`/
/// `SYSCALL_COND_CREATE` via `dispatch` travava o binário de teste inteiro).
/// Uma leitura direta de `static mut`, sem lock nem instrução privilegiada
/// nenhuma, é segura em qualquer anel — mesma categoria seguro-em-host de
/// `CURRENT_FPU_AREA` (nunca lida por nenhum teste hoje, mas por natureza
/// inofensiva se fosse). **Por que uma única global basta aqui, ao contrário
/// de `CURRENT_SYSCALL_RSP_SLOT` (que precisou virar por-thread — ver
/// `KNOW_ISSUES.md`)**: `current_process_id()` sempre é lida uma única vez,
/// de forma síncrona, bem no início de cada `sys_*`, antes de qualquer
/// bloqueio possível — nunca precisa sobreviver a uma troca de contexto
/// dentro da mesma chamada (o valor usado é só a referência já obtida da
/// tabela de handles, não o `ProcessId` em si).
static mut CURRENT_PROCESS_ID: u64 = 0;

/// `ProcessId` do processo dono da thread corrente — ver
/// [`CURRENT_PROCESS_ID`] para o racional completo. Usada por toda `sys_*`
/// que cria ou consulta um handle de mutex/condvar/arquivo/reserva de
/// memória virtual (change `kernel-handle-ownership`).
pub(crate) fn current_process_id() -> ProcessId {
    ProcessId(unsafe { CURRENT_PROCESS_ID })
}

/// Aloca o "slot" por-thread descrito em [`CURRENT_SYSCALL_RSP_SLOT`] — uma
/// única célula `u64`, nunca liberada (mesma disciplina de "leak aceitável
/// por enquanto" de `kernel::fpu::new_area`/`kernel::pthread_handles`).
fn new_syscall_rsp_slot() -> u64 {
    let leaked: &'static mut u64 = alloc::boxed::Box::leak(alloc::boxed::Box::new(0u64));
    leaked as *mut u64 as u64
}

/// Contextos de todas as threads registradas, indexados por `ThreadId`. Sem
/// remoção — esta change não implementa encerramento de thread (ver
/// design.md — Non-Goals).
static CONTEXTS: Spinlock<Vec<Context>> = Spinlock::new(Vec::new());

/// Marca, por `ThreadId`, se uma thread terminou via `SYSCALL_EXIT` — ver
/// design.md da change `libc-minimal-port`, "`SYSCALL_EXIT` e remoção
/// permanente de `ReadyQueue`". Crescida em paralelo com `CONTEXTS` (mesmos
/// índices, nunca encolhida) por [`register_thread`]. Uma thread terminada
/// nunca é removida desta tabela nem de `CONTEXTS` — mesma disciplina de
/// "leak aceitável por enquanto" já aceita para `kernel::pthread_handles`/
/// `kernel::file_handles`.
static TERMINATED: Spinlock<Vec<bool>> = Spinlock::new(Vec::new());

/// Marca a thread `id` como terminada: [`schedule_next`] nunca mais a
/// devolve à `ReadyQueue` depois disso.
///
/// Deliberadamente **sem** `with_interrupts_disabled` (só um `Spinlock.lock()`
/// puro, igual ao caminho não-contestado de `kernel::pthread::Mutex::lock`) —
/// os dois pontos de chamada (`sys_exit`, com `IF` já mascarado por `SFMASK`
/// durante toda a *syscall*, e dentro do fecho de `schedule_next`, já
/// executando com interrupções desabilitadas) nunca precisam da proteção
/// extra, e isso mantém esta função chamável diretamente em teste de host
/// (`cli`/`sti` são instruções privilegiadas — crashariam em anel 3).
pub(crate) fn mark_terminated(id: ThreadId) {
    TERMINATED.lock()[id.0 as usize] = true;
}

/// `true` se a thread `id` já terminou via [`mark_terminated`]. Mesma
/// justificativa de ausência de `with_interrupts_disabled` que lá.
pub(crate) fn is_terminated(id: ThreadId) -> bool {
    TERMINATED.lock()[id.0 as usize]
}

/// Encerra a thread corrente permanentemente, sem nunca retomar: marca-a
/// como terminada ([`mark_terminated`]) e entra num laço `sti`/`hlt` que
/// nunca mais executa nenhuma instrução desta thread — o próprio tick
/// seguinte do timer, no seu curso normal, trata esta thread como
/// `was_running` em [`schedule_next`], vê `is_terminated() == true`, e
/// simplesmente não a devolve à `ReadyQueue`, escolhendo a próxima thread
/// pronta via o `iretq` que o caminho do timer já usa normalmente — nenhuma
/// troca de contexto manual é necessária aqui. Chamada por
/// `kernel::syscall::sys_exit` (encerramento voluntário) e pelos handlers de
/// exceção de `kernel::idt` (encerramento involuntário por `#PF`/`#GP` em
/// modo usuário — ver design.md da change `kernel-fault-isolation`): `IF` já
/// está mascarado em ambos os casos (`SFMASK` durante uma *syscall*, gate de
/// interrupção durante uma exceção), então [`mark_terminated`] não precisa
/// de proteção extra, mesmo racional já documentado ali.
pub(crate) fn terminate_current_thread() -> ! {
    mark_terminated(current_thread_id());
    loop {
        unsafe { core::arch::asm!("sti", "hlt", options(preserves_flags)) };
    }
}

/// Processos criados via [`spawn`], retidos pelo escalonador pela mesma
/// razão (sem encerramento/limpeza nesta change). Indexados por posição de
/// criação, não por `ProcessId` diretamente.
pub(crate) static PROCESSES: Spinlock<Vec<Process>> = Spinlock::new(Vec::new());

/// Procura, em [`PROCESSES`], o `Process` cujo `id` é `process_id`, e
/// aplica `f` a ele sob o mesmo lock — `None` se nenhum processo com esse
/// id estiver registrado. Busca linear, aceitável dado o número pequeno de
/// processos simultâneos deste kernel (não um SO multiusuário de propósito
/// geral) — mesma política já aceita para buscas por `ProcessId` em
/// `kernel::file_handles`/`kernel::vmm`. Ver `design.md` da change
/// `kernel-user-thread-create`.
///
/// Deliberadamente sem `with_interrupts_disabled` próprio: todo chamador
/// já está dentro de uma seção crítica existente que também toca
/// `PROCESSES` (`schedule_next`, ou `sys_thread_create` por cima de
/// `current_process_id()`), então chamar de novo aqui seria redundante —
/// `with_interrupts_disabled` já tolera chamadas aninhadas (cada uma só
/// restaura `IF` ao seu próprio estado anterior), mas evitá-las mantém o
/// ponto de `cli`/`sti` explícito no chamador.
pub(crate) fn with_process_mut<R>(process_id: ProcessId, f: impl FnOnce(&mut Process) -> R) -> Option<R> {
    let mut processes = PROCESSES.lock();
    let process = processes.iter_mut().find(|p| p.id == process_id)?;
    Some(f(process))
}

/// Remove `thread` de `threads` e devolve se isso o esvaziou — lógica pura
/// do gate de "última thread do processo" em [`schedule_next`] (change
/// `kernel-user-thread-create`), separada de [`with_process_mut`] para ser
/// testável em host sem precisar construir um `Process`/`AddressSpace` real
/// (que exigiria `read_cr3`, instrução privilegiada — crasharia em anel 3,
/// mesmo motivo já documentado para `with_interrupts_disabled` em
/// [`CURRENT_PROCESS_ID`]).
fn remove_thread_and_check_if_last(threads: &mut Vec<ThreadId>, thread: ThreadId) -> bool {
    threads.retain(|&t| t != thread);
    threads.is_empty()
}

/// Único ponto que atualiza a stack de kernel confiável da thread corrente
/// — `TSS.rsp0` (consultado automaticamente pela CPU em interrupções com
/// troca de privilégio) e `CURRENT_KERNEL_STACK_TOP` (consultado
/// manualmente pela rotina de entrada de `syscall`) sempre juntos, nunca
/// separadamente, para nunca divergirem — ver design.md, "Risco: esquecer
/// de atualizar `TSS.rsp0`".
fn set_current_kernel_stack(top: u64) {
    unsafe {
        crate::gdt::set_kernel_stack(top);
        CURRENT_KERNEL_STACK_TOP = top;
    }
}

fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe { core::arch::asm!("pushfq", "pop {0}", out(reg) flags, options(preserves_flags)) };
    flags & (1 << 9) != 0
}

/// Executa `f` com interrupções desabilitadas durante toda a sua duração,
/// restaurando o estado anterior de `IF` ao final. Necessário para toda
/// seção crítica sobre `READY_QUEUE`/`CONTEXTS`/`PROCESSES`: o próprio
/// handler do timer precisa de `READY_QUEUE`/`CONTEXTS` a cada tick, e uma
/// thread comum preemptada no meio dessas estruturas (lock já adquirido)
/// causaria um deadlock real de single-core — ver `design.md`, "Fila de
/// prontos do escalonador".
pub(crate) fn with_interrupts_disabled<R>(f: impl FnOnce() -> R) -> R {
    let was_enabled = interrupts_enabled();
    unsafe { core::arch::asm!("cli", options(preserves_flags)) };
    let result = f();
    if was_enabled {
        unsafe { core::arch::asm!("sti", options(preserves_flags)) };
    }
    result
}

fn register_thread(thread: Thread) -> ThreadId {
    with_interrupts_disabled(|| {
        let id = thread.id;
        {
            let mut contexts = CONTEXTS.lock();
            let index = id.0 as usize;
            if contexts.len() <= index {
                contexts.resize_with(index + 1, || Context { rsp: 0, kernel_stack_top: 0, address_space_root: 0, fpu_state: 0, syscall_rsp_slot: 0, timed_out: false, process: ProcessId(0) });
            }
            contexts[index] = thread.context;
            // Área de FXSAVE própria desta thread, alocada aqui (único ponto
            // de registro de todas as threads) em vez de em cada local que
            // constrói um `Context` — ver design.md da change
            // `kernel-fpu-context-switch`, "Área de FXSAVE por thread".
            contexts[index].fpu_state = crate::fpu::new_area();
            // Mesma disciplina acima, para o slot de `CURRENT_SYSCALL_RSP_SLOT`
            // (change `kernel-timed-wait`, correção registrada em
            // `KNOW_ISSUES.md`) — por-thread, nunca uma única `static` global.
            contexts[index].syscall_rsp_slot = new_syscall_rsp_slot();
        }
        {
            let mut terminated = TERMINATED.lock();
            let index = id.0 as usize;
            if terminated.len() <= index {
                terminated.resize(index + 1, false);
            }
        }
        READY_QUEUE.lock().push_back(id);
        id
    })
}

/// A thread atualmente em execução: por construção, sempre a que está na
/// frente da `ReadyQueue` entre dois ticks (ver `on_timer_tick` — a thread
/// `next` escolhida num tick permanece na frente até o próximo). Usada por
/// `kernel::wait_queue::WaitQueue::block_current` para saber qual thread
/// registrar a intenção de bloqueio.
///
/// Entra em pânico se chamada antes de qualquer thread ter sido registrada
/// (nenhum consumidor legítimo chama isso antes de `register_boot_thread`).
pub(crate) fn current_thread_id() -> ThreadId {
    with_interrupts_disabled(|| READY_QUEUE.lock().front().expect("escalonador sem nenhuma thread corrente"))
}

/// Devolve `id` à rotação normal do escalonador (fim da `ReadyQueue`) —
/// seguro para chamar a partir de uma ISR, já que usa
/// `with_interrupts_disabled` internamente (nunca mantém o lock além da
/// seção crítica mínima). Usada por `kernel::wait_queue::WaitQueue::wake_one`/
/// `wake_all` para devolver uma thread despertada à rotação.
pub(crate) fn make_ready(id: ThreadId) {
    with_interrupts_disabled(|| READY_QUEUE.lock().push_back(id));
}

/// Marca `CONTEXTS[id].timed_out = true` — usada por
/// `kernel::wait_queue::wake_expired_deadlines` para registrar que `id` está
/// sendo despertada por expiração de prazo, não por notificação normal (ver
/// design.md da change `kernel-timed-wait`). Chamador deve já ter chamado
/// `make_ready(id)` nesta mesma operação (ou chamar logo em seguida) —
/// `CONTEXTS` por si só não afeta o escalonamento.
pub(crate) fn mark_timed_out(id: ThreadId) {
    with_interrupts_disabled(|| CONTEXTS.lock()[id.0 as usize].timed_out = true);
}

/// Lê e limpa (volta a `false`) `CONTEXTS[id].timed_out` — chamada pela
/// própria thread logo após `WaitQueue::commit_block()` retornar, para saber
/// se foi despertada por notificação ou por expiração de prazo. Sempre
/// `false` para uma thread que nunca teve `mark_timed_out` chamada para ela.
pub(crate) fn take_timed_out(id: ThreadId) -> bool {
    with_interrupts_disabled(|| {
        let mut contexts = CONTEXTS.lock();
        let flag = &mut contexts[id.0 as usize].timed_out;
        core::mem::replace(flag, false)
    })
}

/// Registra a thread que já está em execução (`continue_boot`, rodando
/// sobre a stack dedicada de `kernel-stack-guard-page`) como a primeira
/// thread do escalonador. Deve ser chamada uma única vez, antes de `sti`.
/// O `rsp` inicial (`0`, nunca de fato usado como alvo de retomada) é
/// sobrescrito pelo valor real assim que esta thread for interrompida pela
/// primeira vez — ver `on_timer_tick`. `kernel_stack_top` já é o real: o
/// topo da stack dedicada de `kernel-stack-guard-page` (não vem do pool de
/// stacks de thread, já que esta é a stack original do boot).
/// `address_space_root` também já é o real: a PML4 atualmente ativa (montada
/// pelo Limine/`kernel-memory-management`), já que a thread de boot nunca
/// passa por `AddressSpace::new` — ver design.md da change
/// `kernel-syscalls`, achado sobre `CR3` nunca trocado entre processos.
pub fn register_boot_thread() -> ThreadId {
    let id = process::next_thread_id_for_scheduler();
    let boot_process = process::next_process_id_for_scheduler();
    let kernel_stack_top = crate::stack::KERNEL_STACK_BASE + crate::stack::KERNEL_STACK_SIZE;
    let address_space_root = crate::paging::current_root();
    let thread = Thread { id, process: boot_process, context: Context { rsp: 0, kernel_stack_top, address_space_root, fpu_state: 0, syscall_rsp_slot: 0, timed_out: false, process: boot_process } };
    // A stack de kernel confiável precisa estar correta desde já — não dá
    // para esperar o primeiro tick do timer, já que uma troca de privilégio
    // pode, em tese, acontecer antes dele (ver design.md da change
    // `kernel-syscalls`).
    set_current_kernel_stack(kernel_stack_top);
    let thread_id = register_thread(thread);
    // Mesmo racional de `set_current_kernel_stack` acima, para
    // `CURRENT_FPU_AREA` (ver design.md da change `kernel-fpu-context-switch`):
    // `timer_entry`/`block_entry` fazem `FXSAVE [CURRENT_FPU_AREA]` *antes*
    // de `schedule_next` rodar pela primeira vez — sem isto, o primeiro tick
    // do timer executaria `FXSAVE [0]` (valor inicial da global) e geraria
    // um page fault. Achado real em boot: confirmado via
    // `EXCECAO #PF (page fault): endereco=0x0` antes desta correção.
    unsafe { CURRENT_FPU_AREA = CONTEXTS.lock()[thread_id.0 as usize].fpu_state };
    // Mesmo racional acima, agora para `CURRENT_SYSCALL_RSP_SLOT` (change
    // `kernel-timed-wait`) — sem isto, a primeira `syscall` desta thread
    // guardaria/restauraria seu `RSP` de usuário no slot nulo (endereço 0).
    unsafe { CURRENT_SYSCALL_RSP_SLOT = CONTEXTS.lock()[thread_id.0 as usize].syscall_rsp_slot };
    // Mesmo racional acima, agora para `CURRENT_PROCESS_ID` (change
    // `kernel-handle-ownership`) — sem isto, a primeira syscall desta thread
    // que crie/consulte um handle usaria `ProcessId(0)` em vez do processo
    // real dela.
    unsafe { CURRENT_PROCESS_ID = CONTEXTS.lock()[thread_id.0 as usize].process.0 };
    thread_id
}

/// Cria um novo processo com uma thread executando `entry` e a adiciona à
/// fila de prontos do escalonador.
pub fn spawn<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    entry: extern "C" fn() -> !,
) -> Result<ThreadId, ProcessError> {
    let (process, thread) = Process::new(frame_allocator, entry)?;
    let thread_id = register_thread(thread);
    with_interrupts_disabled(|| PROCESSES.lock().push(process));
    Ok(thread_id)
}

/// Cria uma thread de kernel adicional que compartilha o espaço de
/// endereçamento atualmente ativo (`paging::current_root()`), em vez de um
/// processo com `AddressSpace` própria — ver design.md da change
/// `kernel-keyboard`, "Troca de CR3 a cada tick": como o escalonador troca
/// `CR3` incondicionalmente a cada tick (`on_timer_tick`), cada
/// `AddressSpace` distinta em rotação é mais uma troca de `CR3`/flush de
/// TLB por tick — achado real em boot via QEMU, onde isso se mostrou
/// suficiente para impedir a entrega confiável de uma IRQ externa de baixa
/// frequência (o teclado) competindo com o timer a 100 Hz. Threads só de
/// diagnóstico/demonstração que não precisam de isolamento de endereço
/// (ex.: `keyboard_demo`) usam esta função em vez de [`spawn`].
pub fn spawn_sharing_current_address_space<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    entry: extern "C" fn() -> !,
) -> Result<ThreadId, ProcessError> {
    let process = process::next_process_id_for_scheduler();
    let root = crate::paging::current_root();
    let thread = process::create_kernel_thread(frame_allocator, process, root, entry)?;
    Ok(register_thread(thread))
}

/// Cria uma nova thread de usuário dentro do processo `process_id`
/// (`kernel::syscall::sys_thread_create`, `SYSCALL_THREAD_CREATE`),
/// compartilhando o mesmo espaço de endereçamento desse processo —
/// reaproveita `Process::spawn_user_thread` (já existente, até esta change
/// nunca chamado) via [`with_process_mut`]. `None` se `process_id` não
/// corresponder a nenhum processo registrado em [`PROCESSES`] — nunca
/// deveria acontecer para um chamador real (todo processo é registrado
/// antes de sua primeira thread poder fazer qualquer *syscall*). Ver
/// `design.md` da change `kernel-user-thread-create`.
pub(crate) fn spawn_user_thread_in_process<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    process_id: ProcessId,
    entry: u64,
    user_stack_virt_base: u64,
    user_stack_size: u64,
) -> Option<Result<ThreadId, ProcessError>> {
    with_process_mut(process_id, |process| {
        process.spawn_user_thread(frame_allocator, entry, user_stack_virt_base, user_stack_size)
    })
    .map(|result| result.map(register_thread))
}

/// Endereço virtual, na metade inferior, onde o código do programa de teste
/// de modo usuário (`kernel::user_demo`) é copiado.
const USER_DEMO_CODE_VIRT: u64 = 0x0000_1000_0000_0000;
/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// programa de teste é mapeada.
const USER_DEMO_STACK_VIRT: u64 = 0x0000_2000_0000_0000;
const USER_DEMO_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Cria o processo de demonstração de modo usuário (`kernel::user_demo`):
/// monta seu espaço de endereçamento, mapeia e copia o programa de teste
/// para uma página de código acessível por usuário, cria sua thread inicial
/// (em anel 3), e a adiciona à fila de prontos do escalonador. Ver
/// `design.md` da change `kernel-syscalls`, "Primeiro programa de usuário".
pub fn spawn_user_demo<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<ThreadId, ProcessError> {
    let address_space = Process::begin_new_user(frame_allocator)?;
    let root = address_space.root();

    // Mapeia a página de código na PML4 do *novo* processo — que ainda não
    // está ativa (`CR3` só troca quando o escalonador escolher esta thread
    // pela primeira vez). Por isso os bytes do programa não podem ser
    // copiados diretamente para `USER_DEMO_CODE_VIRT` (inacessível com a
    // paginação atualmente ativa): copiamos via o alias físico-direto
    // (HHDM) do frame recém-alocado, que é válido com qualquer PML4 — ver
    // design.md, achado sobre `Mapper::for_root` vs. escrita direta
    // (mesma classe de bug já corrigida uma vez para o pool de stacks de
    // thread em `kernel-process-scheduler`).
    let code_frame = crate::paging::Mapper::for_root(frame_allocator, root)
        .map_new(
            USER_DEMO_CODE_VIRT,
            crate::paging::PageFlags { writable: true, user_accessible: true, executable: true },
            false,
        )
        .map_err(ProcessError::AddressSpace)?;
    let code_hhdm_addr = crate::hhdm::phys_to_virt(crate::hhdm::offset(), code_frame);
    unsafe { crate::user_demo::copy_into(code_hhdm_addr) };

    let (process, thread) = Process::finish_new_user(
        frame_allocator,
        address_space,
        USER_DEMO_CODE_VIRT,
        USER_DEMO_STACK_VIRT,
        USER_DEMO_STACK_SIZE,
    )?;
    let thread_id = register_thread(thread);
    with_interrupts_disabled(|| PROCESSES.lock().push(process));
    Ok(thread_id)
}

/// Endereço virtual, na metade inferior, onde o código do programa de teste
/// de modo usuário `kernel::user_posix_demo` é copiado — distinto de
/// `USER_DEMO_CODE_VIRT` só por clareza ao ler despejos de depuração (cada
/// um vive no seu próprio `AddressSpace`, então não há colisão real possível
/// mesmo que os valores fossem iguais).
const USER_POSIX_DEMO_CODE_VIRT: u64 = 0x0000_3000_0000_0000;
/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// programa de teste é mapeada.
const USER_POSIX_DEMO_STACK_VIRT: u64 = 0x0000_4000_0000_0000;
const USER_POSIX_DEMO_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Cria o processo de demonstração de modo usuário
/// `kernel::user_posix_demo`: mesma estrutura de [`spawn_user_demo`], com seu
/// próprio `AddressSpace`, página de código e stack dedicadas — ver
/// `design.md` da change `kernel-jvm-posix-syscalls`, "Escopo da verificação
/// em boot real".
pub fn spawn_user_posix_demo<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<ThreadId, ProcessError> {
    let address_space = Process::begin_new_user(frame_allocator)?;
    let root = address_space.root();

    let code_frame = crate::paging::Mapper::for_root(frame_allocator, root)
        .map_new(
            USER_POSIX_DEMO_CODE_VIRT,
            crate::paging::PageFlags { writable: true, user_accessible: true, executable: true },
            false,
        )
        .map_err(ProcessError::AddressSpace)?;
    let code_hhdm_addr = crate::hhdm::phys_to_virt(crate::hhdm::offset(), code_frame);
    unsafe { crate::user_posix_demo::copy_into(code_hhdm_addr) };

    let (process, thread) = Process::finish_new_user(
        frame_allocator,
        address_space,
        USER_POSIX_DEMO_CODE_VIRT,
        USER_POSIX_DEMO_STACK_VIRT,
        USER_POSIX_DEMO_STACK_SIZE,
    )?;
    let thread_id = register_thread(thread);
    with_interrupts_disabled(|| PROCESSES.lock().push(process));
    Ok(thread_id)
}

/// Endereço virtual, na metade inferior, onde o código do programa de teste
/// de modo usuário `kernel::file_syscalls_demo` é copiado — distinto das
/// demais constantes `*_CODE_VIRT` só por clareza em despejos de depuração
/// (cada um vive no seu próprio `AddressSpace`).
const FILE_SYSCALLS_DEMO_CODE_VIRT: u64 = 0x0000_6000_0000_0000;
/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// programa de teste é mapeada.
const FILE_SYSCALLS_DEMO_STACK_VIRT: u64 = 0x0000_7000_0000_0000;
const FILE_SYSCALLS_DEMO_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Cria o processo de demonstração de modo usuário
/// `kernel::file_syscalls_demo`: mesma estrutura de [`spawn_user_posix_demo`],
/// com seu próprio `AddressSpace`, página de código e stack dedicadas — ver
/// `design.md` da change `kernel-file-syscalls`, tarefa 3.2.
pub fn spawn_file_syscalls_demo<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<ThreadId, ProcessError> {
    let address_space = Process::begin_new_user(frame_allocator)?;
    let root = address_space.root();

    let code_frame = crate::paging::Mapper::for_root(frame_allocator, root)
        .map_new(
            FILE_SYSCALLS_DEMO_CODE_VIRT,
            crate::paging::PageFlags { writable: true, user_accessible: true, executable: true },
            false,
        )
        .map_err(ProcessError::AddressSpace)?;
    let code_hhdm_addr = crate::hhdm::phys_to_virt(crate::hhdm::offset(), code_frame);
    unsafe { crate::file_syscalls_demo::copy_into(code_hhdm_addr) };

    let (process, thread) = Process::finish_new_user(
        frame_allocator,
        address_space,
        FILE_SYSCALLS_DEMO_CODE_VIRT,
        FILE_SYSCALLS_DEMO_STACK_VIRT,
        FILE_SYSCALLS_DEMO_STACK_SIZE,
    )?;
    let thread_id = register_thread(thread);
    with_interrupts_disabled(|| PROCESSES.lock().push(process));
    Ok(thread_id)
}

/// Endereço virtual, na metade inferior, onde o código do programa de teste
/// de modo usuário `kernel::exit_demo` é copiado — distinto das demais
/// constantes `*_CODE_VIRT` só por clareza em despejos de depuração (cada um
/// vive no seu próprio `AddressSpace`). Ver design.md da change
/// `libc-minimal-port`, tarefa 1.6.
///
/// Deliberadamente bem abaixo de `0x0000_8000_0000_0000` (índice 256 da
/// PML4): esse valor cairia exatamente no início da metade superior
/// compartilhada (HHDM — ver `docs/architecture/overview.md`, seção 2),
/// já mapeada em todo `AddressSpace`, causando `AlreadyMapped` — achado
/// real ao implementar esta tarefa (o incremento de `0x1000_0000_0000`
/// usado pelas demais constantes `*_CODE_VIRT`/`*_STACK_VIRT` deste arquivo
/// não pode ultrapassar 7 passos a partir de zero sem invadir esse
/// território).
const EXIT_DEMO_CODE_VIRT: u64 = 0x0000_0100_0000_0000;
/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// programa de teste é mapeada.
const EXIT_DEMO_STACK_VIRT: u64 = 0x0000_0200_0000_0000;
const EXIT_DEMO_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Cria o processo de demonstração de modo usuário `kernel::exit_demo`:
/// mesma estrutura de [`spawn_user_posix_demo`], com seu próprio
/// `AddressSpace`, página de código e stack dedicadas — verifica
/// `SYSCALL_EXIT` em boot real (ver design.md da change `libc-minimal-port`).
pub fn spawn_exit_demo<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<ThreadId, ProcessError> {
    let address_space = Process::begin_new_user(frame_allocator)?;
    let root = address_space.root();

    let code_frame = crate::paging::Mapper::for_root(frame_allocator, root)
        .map_new(
            EXIT_DEMO_CODE_VIRT,
            crate::paging::PageFlags { writable: true, user_accessible: true, executable: true },
            false,
        )
        .map_err(ProcessError::AddressSpace)?;
    let code_hhdm_addr = crate::hhdm::phys_to_virt(crate::hhdm::offset(), code_frame);
    unsafe { crate::exit_demo::copy_into(code_hhdm_addr) };

    let (process, thread) = Process::finish_new_user(
        frame_allocator,
        address_space,
        EXIT_DEMO_CODE_VIRT,
        EXIT_DEMO_STACK_VIRT,
        EXIT_DEMO_STACK_SIZE,
    )?;
    let thread_id = register_thread(thread);
    with_interrupts_disabled(|| PROCESSES.lock().push(process));
    Ok(thread_id)
}

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste do carregador ELF (`/bin/test.elf`, ver design.md da
/// change `kernel-elf-loader`) é mapeada — o código em si vem dos endereços
/// que o próprio executável declara (`p_vaddr`), não de uma constante como
/// `USER_DEMO_CODE_VIRT`.
pub const ELF_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_5000_0000_0000;
pub const ELF_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste do port mínimo de libc (`/bin/libctest.elf`, change
/// `libc-minimal-port`) é mapeada — mesmo papel de
/// `ELF_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por clareza em
/// despejos de depuração. Deliberadamente bem abaixo de `0x0000_8000_0000_0000`
/// (índice 256 da PML4, início da HHDM compartilhada) — ver comentário em
/// `EXIT_DEMO_CODE_VIRT` para o achado real que motiva essa faixa.
pub const LIBC_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0300_0000_0000;
pub const LIBC_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste do runtime C++ mínimo (`/bin/cxxtest.elf`, change
/// `cxx-minimal-runtime`) é mapeada — mesmo papel de
/// `LIBC_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por clareza em
/// despejos de depuração.
pub const CXX_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0800_0000_0000;
pub const CXX_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste da API `javaos_vm.h`/`glue.c` (`/bin/javaosvm.elf`,
/// change `os-javaos-memory-pd`) é mapeada — mesmo papel de
/// `CXX_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por clareza em
/// despejos de depuração.
pub const JAVAOS_VM_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0900_0000_0000;
pub const JAVAOS_VM_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste das funções `pthread_mutex_*`/`pthread_cond_*` reais
/// (`/bin/javaospthread.elf`, change `os-javaos-platform-sync`) é mapeada —
/// mesmo papel de `JAVAOS_VM_TEST_PROGRAM_STACK_VIRT`, endereço distinto só
/// por clareza em despejos de depuração.
pub const JAVAOS_PTHREAD_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0a00_0000_0000;
pub const JAVAOS_PTHREAD_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste de `libm`/conversão float (`/bin/libm.elf`, change
/// `libc-float-math`) é mapeada — mesmo papel de
/// `JAVAOS_PTHREAD_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por
/// clareza em despejos de depuração.
pub const LIBM_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0b00_0000_0000;
pub const LIBM_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste de espera com prazo real (`/bin/timedw.elf`, change
/// `kernel-timed-wait`) é mapeada — mesmo papel de
/// `LIBM_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por clareza em
/// despejos de depuração.
pub const TIMEDWAIT_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0c00_0000_0000;
pub const TIMEDWAIT_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste de isolamento de falha por processo
/// (`/bin/fault.elf`, change `kernel-fault-isolation`) é mapeada — mesmo
/// papel de `TIMEDWAIT_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por
/// clareza em despejos de depuração.
pub const FAULT_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0d00_0000_0000;
pub const FAULT_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário do
/// executável de teste de validação de ponteiro de usuário
/// (`/bin/efault.elf`, change `kernel-user-pointer-validation`) é mapeada —
/// mesmo papel de `FAULT_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por
/// clareza em despejos de depuração.
pub const EFAULT_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_0e00_0000_0000;
pub const EFAULT_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereço virtual, na metade inferior, onde a stack de usuário (da
/// thread **principal**, não da thread-irmã — essa é alocada
/// dinamicamente pela própria `SYSCALL_THREAD_CREATE`, na janela de
/// `kernel::stack::USER_THREAD_STACK_POOL_BASE`) do executável de teste de
/// criação de thread de usuário (`/bin/siblingt.elf`, change
/// `kernel-user-thread-create`) é mapeada — mesmo papel de
/// `EFAULT_TEST_PROGRAM_STACK_VIRT`, endereço distinto só por clareza em
/// despejos de depuração. Deliberadamente fora da faixa de incrementos de
/// `0x0000_0100_0000_0000` já totalmente ocupada por este arquivo (ver
/// achado em `kernel::stack::USER_THREAD_STACK_POOL_BASE`).
pub const SIBLING_THREAD_TEST_PROGRAM_STACK_VIRT: u64 = 0x0000_1100_0000_0000;
pub const SIBLING_THREAD_TEST_PROGRAM_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Endereços virtuais das duas variantes de `kernel::fpu_demo` (change
/// `kernel-fpu-context-switch`) — mesma faixa baixa e mesmo racional de
/// `EXIT_DEMO_CODE_VIRT`/`LIBC_TEST_PROGRAM_STACK_VIRT` (bem abaixo do
/// início da HHDM compartilhada).
const FPU_DEMO_A_CODE_VIRT: u64 = 0x0000_0400_0000_0000;
const FPU_DEMO_A_STACK_VIRT: u64 = 0x0000_0500_0000_0000;
const FPU_DEMO_B_CODE_VIRT: u64 = 0x0000_0600_0000_0000;
const FPU_DEMO_B_STACK_VIRT: u64 = 0x0000_0700_0000_0000;
const FPU_DEMO_STACK_SIZE: u64 = crate::paging::PAGE_SIZE;

/// Cria os processos de demonstração/verificação de
/// `kernel::fpu_demo` (duas variantes, A e B, cada uma num `AddressSpace`
/// próprio) — mesma estrutura de [`spawn_exit_demo`]. Ver design.md da
/// change `kernel-fpu-context-switch`, tarefas 4.1-4.3: prova que o estado
/// de FPU/SSE de cada thread sobrevive à preempção sem se misturar com o de
/// outra.
pub fn spawn_fpu_demo<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<(ThreadId, ThreadId), ProcessError> {
    let spawn_variant = |frame_allocator: &mut FrameAllocator<S>,
                          code_virt: u64,
                          stack_virt: u64,
                          copy_fn: unsafe fn(u64)|
     -> Result<ThreadId, ProcessError> {
        let address_space = Process::begin_new_user(frame_allocator)?;
        let root = address_space.root();

        let code_frame = crate::paging::Mapper::for_root(frame_allocator, root)
            .map_new(code_virt, crate::paging::PageFlags { writable: true, user_accessible: true, executable: true }, false)
            .map_err(ProcessError::AddressSpace)?;
        let code_hhdm_addr = crate::hhdm::phys_to_virt(crate::hhdm::offset(), code_frame);
        unsafe { copy_fn(code_hhdm_addr) };

        let (process, thread) =
            Process::finish_new_user(frame_allocator, address_space, code_virt, stack_virt, FPU_DEMO_STACK_SIZE)?;
        let thread_id = register_thread(thread);
        with_interrupts_disabled(|| PROCESSES.lock().push(process));
        Ok(thread_id)
    };

    let a = spawn_variant(frame_allocator, FPU_DEMO_A_CODE_VIRT, FPU_DEMO_A_STACK_VIRT, crate::fpu_demo::copy_into_a)?;
    let b = spawn_variant(frame_allocator, FPU_DEMO_B_CODE_VIRT, FPU_DEMO_B_STACK_VIRT, crate::fpu_demo::copy_into_b)?;
    Ok((a, b))
}

/// Carrega um executável ELF64 estático do sistema de arquivos `fs` em
/// `path` e o coloca para rodar em um novo processo de modo usuário — ver
/// spec `kernel-elf-loader` e `design.md` da change `kernel-elf-loader`,
/// "`load_and_spawn` vive em `kernel::scheduler`" (`register_thread`/
/// `PROCESSES` são privados/`pub(crate)` deste módulo, então a função de
/// alto nível vive aqui, não em `kernel::elf`). Mesma estrutura de
/// [`spawn_user_demo`], mas o código do processo vem de um arquivo real em
/// vez de um programa embutido no binário do kernel.
///
/// Nenhum processo é registrado em [`PROCESSES`] nem tem thread criada
/// (`register_thread`) antes de todos os segmentos terem sido mapeados com
/// sucesso — qualquer falha (leitura do arquivo, validação do ELF,
/// sobreposição de segmentos, falta de memória) retorna `Err` antes desse
/// ponto, deixando nenhum processo "fantasma" visível ao escalonador (ver
/// design.md, "Riscos — processo parcial").
pub fn spawn_from_elf<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    fs: &crate::fat32::Fat32Filesystem,
    path: &str,
    user_stack_virt_base: u64,
    user_stack_size: u64,
) -> Result<ThreadId, crate::elf::ElfError> {
    let file = crate::storage::with_disk(|disk| fs.read_file(disk, path))
        .ok_or(crate::elf::ElfError::NoDisk)?
        .map_err(crate::elf::ElfError::FileRead)?;

    let header = crate::elf::ElfHeader::parse(&file)?;
    // `ElfHeader::parse` sozinho aceita `ET_EXEC` e `ET_DYN` (desde a change
    // `kernel-dynamic-linker`, que reaproveita o parser para bibliotecas
    // `.so`) — esta capability só carrega executáveis estáticos, então
    // rejeita `ET_DYN` aqui explicitamente, preservando o comportamento já
    // coberto pela spec `kernel-elf-loader`.
    if header.e_type != crate::elf::ET_EXEC {
        return Err(crate::elf::ElfError::UnsupportedExecutable);
    }
    let segments = crate::elf::load_program_headers(&file, &header)?;

    let address_space = Process::begin_new_user(frame_allocator).map_err(crate::elf::ElfError::Process)?;
    let root = address_space.root();
    crate::elf::load_segments(frame_allocator, root, &file, &segments)?;

    let (process, thread) =
        Process::finish_new_user(frame_allocator, address_space, header.entry, user_stack_virt_base, user_stack_size)
            .map_err(crate::elf::ElfError::Process)?;
    let thread_id = register_thread(thread);
    with_interrupts_disabled(|| PROCESSES.lock().push(process));
    Ok(thread_id)
}

/// Chamada pela rotina de entrada do timer ([`timer_entry`]) com o `rsp` da
/// thread que acabou de ser interrompida. Mantém o incremento do contador
/// Lógica de decisão de escalonamento em si — compartilhada entre
/// `on_timer_tick` (tick real de hardware do timer) e `on_block_request`
/// (bloqueio voluntário via `WaitQueue::block_current`, ver
/// `kernel::wait_queue`): salva `current_rsp` como o contexto da thread
/// interrompida, decide seu destino (recolocar no fim da `ReadyQueue`,
/// comportamento padrão; ou desviá-la para uma `WaitQueue`, se ela pediu
/// bloqueio antes desta chamada), escolhe a próxima via round-robin, atualiza
/// `TSS.rsp0`/`CR3` para a nova thread corrente (ver `design.md` da change
/// `kernel-syscalls`), e devolve o `rsp` salvo dela, para a rotina de entrada
/// restaurar.
///
/// Deliberadamente **não** toca nada específico de hardware do timer
/// (contagem de ticks, EOI do Local APIC) — isso é responsabilidade exclusiva
/// de `on_timer_tick`. Ver design.md da change `kernel-wait-queue`, "Achados
/// durante a implementação": um `int` de software não corresponde a nenhuma
/// interrupção real "em serviço" no Local APIC, e enviar um EOI nessa
/// condição (via `on_block_request`, antes desta correção) deixava a entrega
/// do timer real travada para sempre.
fn schedule_next(current_rsp: u64) -> u64 {
    with_interrupts_disabled(|| {
        let was_running = READY_QUEUE.lock().pop_front().expect("escalonador sem nenhuma thread registrada");
        {
            CONTEXTS.lock()[was_running.0 as usize].rsp = current_rsp;
        }

        // Ponto de decisão (ver design.md, "`ReadyQueue` ganha um ponto de
        // decisão explícito"): se há uma intenção de bloqueio pendente para
        // `was_running`, ela é desviada para a `WaitQueue` indicada; se ela
        // terminou via `SYSCALL_EXIT` (change `libc-minimal-port`), ela
        // simplesmente não volta à rotação; caso contrário (qualquer chamada
        // normal), comportamento padrão inalterado.
        if !crate::wait_queue::take_block_intent(was_running) && !is_terminated(was_running) {
            READY_QUEUE.lock().push_back(was_running);
        }

        let next = READY_QUEUE
            .lock()
            .front()
            .expect("escalonador sem nenhuma thread pronta (todas bloqueadas) - fora do escopo desta change, ver design.md");
        let contexts = CONTEXTS.lock();
        set_current_kernel_stack(contexts[next.0 as usize].kernel_stack_top);
        // Achado durante a implementação (ver design.md): threads de
        // processos diferentes podem ter mapeamentos próprios na metade
        // inferior (ex.: código/stack de uma thread de usuário) — sem isso,
        // a CPU continuaria com a PML4 de outro processo ativa.
        unsafe { crate::paging::switch_address_space(contexts[next.0 as usize].address_space_root) };
        // Mesmo trecho que já atualiza TSS.rsp0/CR3 acima — ver design.md da
        // change `kernel-fpu-context-switch`: `timer_entry`/`block_entry`
        // fazem `FXRSTOR [CURRENT_FPU_AREA]` logo depois de trocar `RSP`,
        // então esta global precisa já estar correta antes de `schedule_next`
        // retornar.
        unsafe { CURRENT_FPU_AREA = contexts[next.0 as usize].fpu_state };
        // Mesmo racional acima, para `CURRENT_SYSCALL_RSP_SLOT` (change
        // `kernel-timed-wait`, correção registrada em `KNOW_ISSUES.md`):
        // precisa já apontar para o slot de `next` antes de `schedule_next`
        // retornar, já que `next` pode retomar em qualquer ponto dentro de
        // `syscall_entry` (ex.: logo após ser despertada de um bloqueio).
        unsafe { CURRENT_SYSCALL_RSP_SLOT = contexts[next.0 as usize].syscall_rsp_slot };
        // Mesmo racional acima, para `CURRENT_PROCESS_ID` (change
        // `kernel-handle-ownership`) — precisa já refletir o processo de
        // `next` antes de `schedule_next` retornar, já que a primeira
        // syscall dela (ex.: logo após ser despertada) pode precisar
        // identificar o processo chamador.
        unsafe { CURRENT_PROCESS_ID = contexts[next.0 as usize].process.0 };
        let next_rsp = contexts[next.0 as usize].rsp;
        // Liberação real de recursos (change `kernel-process-cleanup`): agora
        // que `CR3` já trocou para `next` (acima), nunca mais para a PML4 de
        // `was_running`, é seguro devolver ao alocador todo frame físico que
        // só o espaço de endereçamento dela possuía — ver design.md, "Chamada
        // depois de `switch_address_space`...". `is_terminated` garante que
        // isto só roda exatamente uma vez por thread (ela nunca mais é
        // escolhida como `was_running` depois de não voltar à `ReadyQueue`).
        if is_terminated(was_running) {
            let owner = contexts[was_running.0 as usize].process;
            // Gate "última thread do processo" (change
            // `kernel-user-thread-create`, ver design.md): sem isto, uma
            // thread terminando liberaria incondicionalmente a memória e os
            // handles do processo inteiro, mesmo com uma thread-irmã ainda
            // viva no mesmo processo (compartilhando o mesmo espaço de
            // endereçamento desde que `SYSCALL_THREAD_CREATE` existe) — o que
            // destruiria a memória/handles dela por baixo dos pés. Remove
            // `was_running` de `process.threads` e só libera quando isso
            // esvazia a lista. `with_process_mut` só devolve `None` se
            // `owner` já não estiver mais em `PROCESSES`, o que nunca deveria
            // acontecer (nenhum processo é removido dela — ver comentário de
            // `PROCESSES` acima); trata-se como "última thread" nesse caso
            // defensivo, preservando a liberação que já acontecia antes desta
            // change.
            let is_last_thread_of_process =
                with_process_mut(owner, |process| remove_thread_and_check_if_last(&mut process.threads, was_running))
                    .unwrap_or(true);

            if is_last_thread_of_process {
                let mut allocator = crate::phys::FRAME_ALLOCATOR.lock();
                let free_before = allocator.free_frames();
                crate::paging::AddressSpace::free_user_mappings(
                    contexts[was_running.0 as usize].address_space_root,
                    &mut allocator,
                );
                // Achado real ao verificar em boot real (change
                // `kernel-process-cleanup`): medir "antes"/"depois" em pontos
                // distantes no tempo (ex.: antes de carregar este processo vs.
                // logo antes de `idle_loop()`) não funciona — nada é de fato
                // escalonado antes de `idle_loop()` chamar `sti` pela primeira
                // vez (nenhuma troca de contexto acontece com interrupções
                // desabilitadas), então qualquer falta/limpeza real só acontece
                // bem depois, em plena rotação já com `sti`. A medição confiável
                // é esta: imediatamente antes/depois desta própria liberação.
                crate::serial_println!(
                    "kernel_process_cleanup: thread {} liberou {} frames (livres: {} -> {})",
                    was_running.0,
                    allocator.free_frames() - free_before,
                    free_before,
                    allocator.free_frames()
                );
                // Posse de handles/reservas (change `kernel-handle-ownership`):
                // mesmo ponto, mesmo racional — fecha todo arquivo e libera toda
                // reserva de memória virtual que só este processo possuía.
                // `pthread_handles` fica deliberadamente de fora (ver
                // design.md, "Por que `pthread_handles` continua sem destruição
                // real").
                let files_closed = crate::file_handles::close_all_owned_by(owner);
                let regions_released = crate::vmm::VMM.lock().release_all_owned_by(&mut allocator, owner);
                if files_closed > 0 || regions_released > 0 {
                    crate::serial_println!(
                        "kernel_handle_ownership: thread {} fechou {} arquivo(s) e liberou {} reserva(s) de memoria virtual",
                        was_running.0,
                        files_closed,
                        regions_released
                    );
                }
            }
        }
        next_rsp
    })
}

/// Chamada pela rotina de entrada do timer ([`timer_entry`]) com o `rsp` da
/// thread que acabou de ser interrompida. Mantém o incremento do contador de
/// ticks e o EOI já existentes em `kernel::timer` (só fazem sentido para uma
/// interrupção real de hardware), delegando a decisão de escalonamento em si
/// a [`schedule_next`].
#[unsafe(no_mangle)]
extern "C" fn on_timer_tick(current_rsp: u64) -> u64 {
    crate::timer::on_tick();
    crate::wait_queue::wake_expired_deadlines(crate::clock::monotonic_nanos());
    schedule_next(current_rsp)
}

/// Vetor de interrupção de software dedicado a pedidos voluntários de
/// bloqueio (`WaitQueue::block_current`) — deliberadamente **distinto** do
/// vetor do timer (ver design.md, "Achados durante a implementação", e a
/// correção da decisão original "Reaproveitar `int <vetor do timer>`") e do
/// vetor do teclado (`kernel::keyboard::KEYBOARD_VECTOR`, 33 — achado real
/// durante a implementação: 33 colidia diretamente com ele, ver design.md).
pub const BLOCK_VECTOR: usize = 34;

/// Chamada pela rotina de entrada de bloqueio ([`block_entry`]), disparada
/// por `WaitQueue::block_current` via `int {BLOCK_VECTOR}`. Mesma decisão de
/// escalonamento de `on_timer_tick` (via [`schedule_next`]), mas sem nenhuma
/// contagem de ticks nem EOI — um `int` de software não é uma interrupção
/// real de hardware "em serviço" no Local APIC.
#[unsafe(no_mangle)]
extern "C" fn on_block_request(current_rsp: u64) -> u64 {
    schedule_next(current_rsp)
}

core::arch::global_asm!(
    ".global timer_entry",
    "timer_entry:",
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // FXSAVE do estado de FPU/SSE da thread que está saindo, na área
    // apontada por CURRENT_FPU_AREA (ainda a da thread corrente neste
    // ponto) — ver design.md da change `kernel-fpu-context-switch`. `rax`
    // já foi salvo pelo `push rax` acima, livre como escrátio de endereço.
    "mov rax, [CURRENT_FPU_AREA]",
    "fxsave [rax]",
    // `current_rsp` (1o argumento, SysV) antes de qualquer outra coisa
    // tocar RSP.
    "mov rdi, rsp",
    // A ABI exige RSP % 16 == 0 imediatamente antes de um `call`, mas uma
    // interrupção pode chegar com RSP em qualquer alinhamento — forçamos o
    // alinhamento aqui; o valor original já foi salvo em RDI e nunca é
    // necessário de volta, já que trocamos de stack logo em seguida.
    "and rsp, -16",
    "call on_timer_tick",
    // `on_timer_tick` devolve, em RAX (SysV), o `rsp` salvo da próxima
    // thread.
    "mov rsp, rax",
    // FXRSTOR do estado de FPU/SSE da nova thread corrente — `schedule_next`
    // já atualizou CURRENT_FPU_AREA antes de retornar. `rax` já foi
    // consumido pelo `mov rsp, rax` acima; o `pop rax` mais abaixo restaura
    // o valor correto a partir da stack da nova thread, não deste `mov`.
    "mov rax, [CURRENT_FPU_AREA]",
    "fxrstor [rax]",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",
    "iretq",
);

// Cópia de `timer_entry` acima, só trocando a função chamada
// (`on_block_request` em vez de `on_timer_tick`) — mesmo formato de frame e
// mesma disciplina de push/pop de registradores, já comprovados. Ver
// design.md, "Achados durante a implementação", sobre por que este vetor
// precisa ser distinto do timer.
core::arch::global_asm!(
    ".global block_entry",
    "block_entry:",
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Mesma disciplina de FXSAVE/FXRSTOR de `timer_entry` acima — ver
    // design.md da change `kernel-fpu-context-switch`.
    "mov rax, [CURRENT_FPU_AREA]",
    "fxsave [rax]",
    "mov rdi, rsp",
    "and rsp, -16",
    "call on_block_request",
    "mov rsp, rax",
    "mov rax, [CURRENT_FPU_AREA]",
    "fxrstor [rax]",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",
    "iretq",
);

unsafe extern "C" {
    fn timer_entry();
    fn block_entry();
}

/// Endereço da rotina de entrada do timer (ver `global_asm!` acima), para
/// registrar no vetor do timer via `idt::set_handler`.
pub fn timer_entry_addr() -> u64 {
    timer_entry as *const () as u64
}

/// Endereço da rotina de entrada de bloqueio (ver `global_asm!` acima), para
/// registrar em [`BLOCK_VECTOR`] via `idt::set_handler`.
pub fn block_entry_addr() -> u64 {
    block_entry as *const () as u64
}

/// Instala o handler de [`BLOCK_VECTOR`] na IDT — deve ser chamada uma única
/// vez durante o boot, antes de qualquer `WaitQueue::block_current()` (mesmo
/// requisito de `kernel::timer::init` para o vetor do timer).
///
/// # Safety
/// Mesmos requisitos de `kernel::timer::init`: deve ser chamada depois de
/// `gdt::init()` e com a IDT já carregável (antes de `idt::load()` ou antes
/// de qualquer `int {BLOCK_VECTOR}` ser executado).
pub fn install_block_vector() {
    crate::idt::set_handler(BLOCK_VECTOR, block_entry_addr(), 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduz o comportamento padrão do antigo `advance()` monolítico
    /// (sempre recolocar `was_running` no fim) compondo os passos menores —
    /// usado pelos testes abaixo para confirmar que a refatoração não altera
    /// o caminho padrão (nenhuma intenção de bloqueio pendente).
    fn advance_without_blocking(queue: &mut ReadyQueue) -> Option<(ThreadId, ThreadId)> {
        let was_running = queue.pop_front()?;
        queue.push_back(was_running);
        let next = queue.front().expect("acabou de inserir um elemento; a fila não pode estar vazia");
        Some((was_running, next))
    }

    #[test]
    fn single_thread_queue_always_advances_to_itself() {
        let mut queue = ReadyQueue::new();
        queue.push_back(ThreadId(7));

        assert_eq!(advance_without_blocking(&mut queue), Some((ThreadId(7), ThreadId(7))));
        assert_eq!(advance_without_blocking(&mut queue), Some((ThreadId(7), ThreadId(7))));
    }

    #[test]
    fn round_robin_visits_every_thread_before_repeating() {
        let mut queue = ReadyQueue::new();
        queue.push_back(ThreadId(0));
        queue.push_back(ThreadId(1));
        queue.push_back(ThreadId(2));

        assert_eq!(advance_without_blocking(&mut queue), Some((ThreadId(0), ThreadId(1))));
        assert_eq!(advance_without_blocking(&mut queue), Some((ThreadId(1), ThreadId(2))));
        assert_eq!(advance_without_blocking(&mut queue), Some((ThreadId(2), ThreadId(0))));
        // Completou um ciclo completo: de volta ao estado do primeiro advance.
        assert_eq!(advance_without_blocking(&mut queue), Some((ThreadId(0), ThreadId(1))));
    }

    #[test]
    fn empty_queue_has_nothing_to_advance_to() {
        let mut queue = ReadyQueue::new();
        assert_eq!(advance_without_blocking(&mut queue), None);
    }

    #[test]
    fn popping_without_pushing_back_removes_the_thread_from_rotation() {
        // Cobre o novo ponto de decisão: quando o chamador opta por NÃO
        // recolocar a thread (ex.: ela pediu bloqueio), ela deixa de
        // aparecer em `front()` — ver design.md, "ReadyQueue ganha um ponto
        // de decisão explícito".
        let mut queue = ReadyQueue::new();
        queue.push_back(ThreadId(0));
        queue.push_back(ThreadId(1));

        let was_running = queue.pop_front().unwrap();
        assert_eq!(was_running, ThreadId(0));
        // Deliberadamente não recolocada.
        assert_eq!(queue.front(), Some(ThreadId(1)));

        // Uma `wake_one`/`make_ready` equivalente a devolve à rotação.
        queue.push_back(was_running);
        assert_eq!(queue.front(), Some(ThreadId(1)));
        let _ = queue.pop_front();
        assert_eq!(queue.front(), Some(ThreadId(0)));
    }

    // -------------------------------------------------------------
    // `SYSCALL_EXIT` / remoção permanente de thread terminada
    // (change `libc-minimal-port`) — ver design.md, "`SYSCALL_EXIT` e
    // remoção permanente de `ReadyQueue`".
    // -------------------------------------------------------------

    #[test]
    fn mark_terminated_then_is_terminated_reflects_the_marking() {
        // Não usa `register_thread` (chama `with_interrupts_disabled`,
        // inseguro em host — `cli`/`sti` são privilegiadas) — só reserva uma
        // entrada nova em `TERMINATED` diretamente, mesmo precedente de
        // `kernel::pthread_handles` registrando entradas globais em testes.
        let id = {
            let mut table = TERMINATED.lock();
            table.push(false);
            ThreadId((table.len() - 1) as u64)
        };
        assert!(!is_terminated(id), "thread recem-registrada nao deveria estar marcada como terminada");
        mark_terminated(id);
        assert!(is_terminated(id));
    }

    /// Mesma composição de [`advance_without_blocking`], mas decidindo via
    /// um predicado de terminação — reproduz a lógica real de
    /// `schedule_next` (`!take_block_intent(..) && !is_terminated(..)`) só
    /// para o lado da terminação, sem tocar `TERMINATED`/
    /// `with_interrupts_disabled`.
    fn advance_unless_terminated(queue: &mut ReadyQueue, is_terminated: impl Fn(ThreadId) -> bool) -> Option<ThreadId> {
        let was_running = queue.pop_front()?;
        if !is_terminated(was_running) {
            queue.push_back(was_running);
        }
        queue.front()
    }

    #[test]
    fn a_terminated_thread_never_reappears_in_the_queue() {
        let mut queue = ReadyQueue::new();
        queue.push_back(ThreadId(0));
        queue.push_back(ThreadId(1));

        // ThreadId(0) "termina" nesta rodada.
        let next = advance_unless_terminated(&mut queue, |id| id == ThreadId(0));
        assert_eq!(next, Some(ThreadId(1)));
        assert_eq!(queue.front(), Some(ThreadId(1)));

        // Só ThreadId(1) continua girando — nunca mais vemos ThreadId(0).
        let next = advance_unless_terminated(&mut queue, |_| false);
        assert_eq!(next, Some(ThreadId(1)));
        let next = advance_unless_terminated(&mut queue, |_| false);
        assert_eq!(next, Some(ThreadId(1)));
    }

    #[test]
    fn when_every_thread_terminates_the_queue_becomes_empty() {
        // Mesmo resultado de `empty_queue_has_nothing_to_advance_to`: nenhuma
        // thread pronta — o `schedule_next` real, integrado com hardware,
        // ainda entra em pânico nesse caso (limitação pré-existente, fora do
        // escopo desta change — ver comentário em `schedule_next`); este
        // teste cobre só a lógica pura da `ReadyQueue` em si.
        let mut queue = ReadyQueue::new();
        queue.push_back(ThreadId(0));

        let next = advance_unless_terminated(&mut queue, |_| true);
        assert_eq!(next, None);
        assert_eq!(queue.front(), None);
    }

    // -------------------------------------------------------------
    // Gate "última thread do processo" na limpeza de término (change
    // `kernel-user-thread-create`) — ver design.md.
    // -------------------------------------------------------------

    #[test]
    fn removing_the_only_thread_reports_the_process_as_last() {
        let mut threads = alloc::vec![ThreadId(0)];
        assert!(remove_thread_and_check_if_last(&mut threads, ThreadId(0)));
        assert!(threads.is_empty());
    }

    #[test]
    fn removing_one_of_several_threads_reports_the_process_as_not_last() {
        let mut threads = alloc::vec![ThreadId(0), ThreadId(1)];
        assert!(!remove_thread_and_check_if_last(&mut threads, ThreadId(0)));
        assert_eq!(threads, alloc::vec![ThreadId(1)], "so a thread terminada deve ser removida, a thread-irma continua na lista");
    }

    #[test]
    fn removing_the_last_remaining_thread_reports_the_process_as_last() {
        // Reproduz o caso real: a thread-irma ja terminou antes (removida
        // numa chamada anterior), e agora a ultima tambem termina.
        let mut threads = alloc::vec![ThreadId(1)];
        assert!(remove_thread_and_check_if_last(&mut threads, ThreadId(1)));
    }

    #[test]
    fn with_process_mut_finds_nothing_for_an_unknown_process_id() {
        // Nao precisa construir nenhum `Process`/`AddressSpace` real (ver
        // comentario de `remove_thread_and_check_if_last`): `PROCESSES` so
        // precisa estar vazio para este caso, e o gate em `schedule_next`
        // trata `None` como "ultima thread" (`unwrap_or(true)`), preservando
        // o comportamento anterior a esta change.
        let unknown = ProcessId(u64::MAX);
        assert_eq!(with_process_mut(unknown, |_process| ()), None);
    }
}
