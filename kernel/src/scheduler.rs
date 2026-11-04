//! Escalonador preemptivo round-robin do kernel — ver `design.md` da change
//! `kernel-process-scheduler`.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::phys::{FrameAllocator, FrameStore};
use crate::process::{self, Context, Process, ProcessError, Thread, ThreadId};
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

/// Contextos de todas as threads registradas, indexados por `ThreadId`. Sem
/// remoção — esta change não implementa encerramento de thread (ver
/// design.md — Non-Goals).
static CONTEXTS: Spinlock<Vec<Context>> = Spinlock::new(Vec::new());

/// Processos criados via [`spawn`], retidos pelo escalonador pela mesma
/// razão (sem encerramento/limpeza nesta change). Indexados por posição de
/// criação, não por `ProcessId` diretamente.
pub(crate) static PROCESSES: Spinlock<Vec<Process>> = Spinlock::new(Vec::new());

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
                contexts.resize_with(index + 1, || Context { rsp: 0, kernel_stack_top: 0, address_space_root: 0 });
            }
            contexts[index] = thread.context;
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
    let thread = Thread { id, process: boot_process, context: Context { rsp: 0, kernel_stack_top, address_space_root } };
    // A stack de kernel confiável precisa estar correta desde já — não dá
    // para esperar o primeiro tick do timer, já que uma troca de privilégio
    // pode, em tese, acontecer antes dele (ver design.md da change
    // `kernel-syscalls`).
    set_current_kernel_stack(kernel_stack_top);
    register_thread(thread)
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
        // `was_running`, ela é desviada para a `WaitQueue` indicada; caso
        // contrário (qualquer chamada normal), comportamento padrão inalterado.
        if !crate::wait_queue::take_block_intent(was_running) {
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
        contexts[next.0 as usize].rsp
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
    "mov rdi, rsp",
    "and rsp, -16",
    "call on_block_request",
    "mov rsp, rax",
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
}
