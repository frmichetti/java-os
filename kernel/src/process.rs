//! Processos e threads do kernel — ver `design.md` da change
//! `kernel-process-scheduler`.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::gdt::{KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR, USER_CODE_SELECTOR, USER_DATA_SELECTOR};
use crate::paging::{Mapper, PageFlags};
use crate::paging::{AddressSpace, MapError};
use crate::phys::{FrameAllocator, FrameStore};
use crate::stack::{self, StackInitError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

fn next_process_id() -> ProcessId {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    ProcessId(NEXT.fetch_add(1, Ordering::Relaxed))
}

fn next_thread_id() -> ThreadId {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    ThreadId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Usados por `kernel::scheduler` para registrar a thread de boot (que já
/// está em execução, não passa por `Process::new`) com IDs do mesmo espaço
/// monotônico usado por todo o resto — nunca colidem com os de um processo
/// real.
pub(crate) fn next_process_id_for_scheduler() -> ProcessId {
    next_process_id()
}

pub(crate) fn next_thread_id_for_scheduler() -> ThreadId {
    next_thread_id()
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ProcessError {
    AddressSpace(MapError),
    Stack(StackInitError),
}

impl From<MapError> for ProcessError {
    fn from(e: MapError) -> Self {
        ProcessError::AddressSpace(e)
    }
}

impl From<StackInitError> for ProcessError {
    fn from(e: StackInitError) -> Self {
        ProcessError::Stack(e)
    }
}

/// Contexto salvo de uma thread entre trocas. O único estado que não vive na
/// própria stack dela é o ponteiro para o topo dessa stack no momento em que
/// foi interrompida (ou o valor sintético inicial, antes de ela nunca ter
/// rodado) — todo o resto (registradores de uso geral, RIP, RFLAGS) é
/// lido/escrito diretamente na stack pela rotina de troca de contexto em
/// assembly (`kernel::scheduler`). Ver design.md — "Troca de contexto".
pub struct Context {
    pub rsp: u64,
    /// Topo fixo da stack de *kernel* desta thread, definido uma única vez
    /// na criação e nunca mutado depois — distinto de `rsp`, que flutua a
    /// cada troca de contexto. Usado para manter `TSS.rsp0` sempre coerente
    /// com a thread corrente, para que qualquer transição de anel 3 para
    /// anel 0 (interrupção ou `syscall`) aterrisse na stack de kernel certa
    /// — ver `design.md` da change `kernel-syscalls`.
    pub kernel_stack_top: u64,
    /// Endereço físico da PML4 (`AddressSpace::root()`) do processo dono
    /// desta thread, definido uma única vez na criação. **Achado durante a
    /// implementação da change `kernel-syscalls`:** até então, nada jamais
    /// trocava `CR3` entre threads de processos diferentes — inofensivo
    /// enquanto só havia threads de kernel (a metade superior, única que
    /// elas tocam, é idêntica em toda `AddressSpace`), mas uma thread de
    /// usuário tem mapeamentos próprios na metade inferior que só existem
    /// na *sua* PML4. O escalonador agora troca `CR3` para este valor a
    /// cada troca de contexto — ver design.md.
    pub address_space_root: u64,
    /// Endereço da área de `FXSAVE` desta thread (`kernel::fpu::FxSaveArea`,
    /// 512 bytes alinhados a 16) — alocada por `kernel::fpu::new_area()`,
    /// nunca liberada (mesma disciplina de "leak aceitável por enquanto" de
    /// `address_space_root`/`kernel_stack_top`). Ver design.md da change
    /// `kernel-fpu-context-switch`.
    pub fpu_state: u64,
    /// Endereço do slot de `u64` (`kernel::scheduler::new_syscall_rsp_slot`)
    /// onde a rotina assembly `syscall_entry` guarda/restaura o `RSP` de
    /// modo usuário desta thread ao redor de `syscall`/`sysret` — alocado
    /// uma única vez na criação, nunca liberado (mesma disciplina de
    /// `fpu_state`). **Por-thread desde a change `kernel-timed-wait`**: antes
    /// disso era uma única `static` global (`SYSCALL_USER_RSP_SCRATCH`),
    /// corrompida sempre que uma syscall bloqueante cedia a CPU para outra
    /// thread que também fizesse syscalls antes da primeira ser despertada —
    /// ver `KNOW_ISSUES.md`.
    pub syscall_rsp_slot: u64,
    /// `true` se esta thread acabou de ser despertada por expiração de um
    /// prazo (`kernel::wait_queue::wake_expired_deadlines`), `false` se foi
    /// por notificação normal — lido e limpo pela própria thread logo após
    /// `WaitQueue::commit_block()` retornar. Necessário porque `commit_block`
    /// é só uma instrução `int`, sem nenhum canal de retorno de valor — ver
    /// design.md da change `kernel-timed-wait`.
    pub timed_out: bool,
}

/// Uma thread: unidade de escalonamento dentro de um `Process`, com seu
/// próprio contexto de registradores. A stack em si (com guard page) já foi
/// mapeada no momento em que este `Thread` é criado — ver [`init_stack`].
pub struct Thread {
    pub id: ThreadId,
    pub process: ProcessId,
    pub context: Context,
}

/// Número de registradores de uso geral salvos/restaurados em cada troca de
/// contexto (todos exceto `RSP`, cujo "salvamento" é o próprio `Context::rsp`).
const SAVED_GPR_COUNT: u64 = 15;

/// Tamanho, em bytes, do frame de hardware empilhado pela CPU ao entregar
/// uma interrupção. **Achado durante a implementação, corrigindo uma
/// suposição errada de `design.md`:** em modo 64 bits, `iretq` sempre
/// consome os 5 campos (RIP, CS, RFLAGS, RSP, SS), mesmo sem troca de
/// privilégio e sem IST — ao contrário do modo protegido de 32 bits legado
/// (onde só 3 campos são usados sem troca de privilégio). Confirmado em
/// boot real via GDB: com um frame de apenas 3 campos, `iretq` lia 8 bytes
/// além do fim da stack mapeada, causando um `#PF` na própria instrução
/// `iretq`. `design.md` atualizado registrando essa correção.
const HW_FRAME_SIZE: u64 = 5 * 8;

/// `RFLAGS` inicial de uma thread nova: bit 1 (sempre 1, reservado) e bit 9
/// (`IF`, interrupções habilitadas).
const INITIAL_RFLAGS: u64 = 0x202;

/// Ajuste de alinhamento de ABI aplicado ao `RSP` que o frame restaura (ver
/// [`init_stack`]): a entrada em `entry` acontece via `iretq`, não via
/// `call`, então o `RSP` com que `entry` começa a executar precisa ser
/// manualmente ajustado para `% 16 == 8`, como a ABI System V exige na
/// entrada de qualquer função chamada (mesmo racional do `-8` em
/// `kernel::stack::switch_to`).
const ABI_ALIGNMENT_PAD: u64 = 8;

/// Escreve, no topo da stack de **kernel** recém-mapeada de uma thread
/// (`stack_top`, como retornado por `kernel::stack::alloc_thread_stack`),
/// um "trap frame" sintético equivalente ao que a rotina de troca de
/// contexto do escalonador deixaria lá se esta thread já tivesse sido
/// interrompida uma vez — de forma que a primeira vez que o escalonador a
/// retome siga exatamente o mesmo caminho de restauração de qualquer outra
/// thread, sem nenhum caso especial de "primeira execução". Devolve o `rsp`
/// salvo a usar como estado inicial do [`Context`] desta thread.
///
/// Layout, de `stack_top` para baixo (deve bater exatamente com a ordem de
/// empilhar/desempilhar da rotina de troca de contexto em
/// `kernel::scheduler`): `[SS][RSP][RFLAGS][CS][RIP][RAX][RBX][RCX][RDX]
/// [RSI][RDI][RBP][R8][R9][R10][R11][R12][R13][R14][R15]`.
///
/// `entry` é o endereço (`RIP`) por onde a thread começa a executar — um
/// endereço de uma função real do kernel (via `as *const () as u64`) para
/// uma thread de kernel, ou um endereço dentro de uma página de usuário
/// (ex.: `kernel::user_demo::copy_into`) para uma thread de modo usuário,
/// já que não há nenhuma função Rust tipada vivendo em memória acessível
/// por usuário.
///
/// `cs`/`ss` definem o nível de privilégio com que `entry` começa a
/// executar: `KERNEL_CODE_SELECTOR`/`KERNEL_DATA_SELECTOR` para uma thread
/// de kernel, ou `USER_CODE_SELECTOR`/`USER_DATA_SELECTOR` (anel 3) para uma
/// thread de modo usuário — ver `design.md` da change `kernel-syscalls`.
/// `landing_rsp` é o campo `RSP` do frame — o valor que `iretq`
/// efetivamente restaura para `entry` começar a executar (não o `rsp`
/// salvo devolvido por esta função, que é bem mais abaixo, no início do
/// bloco de registradores): para uma thread de kernel, um ponto da própria
/// `stack_top` (ver [`init_kernel_stack`]); para uma thread de modo
/// usuário, o topo de uma stack de usuário separada.
///
/// # Safety
/// `stack_top` deve ser o topo de uma stack de kernel mapeada e exclusiva
/// desta thread, com pelo menos `HW_FRAME_SIZE + SAVED_GPR_COUNT * 8` bytes
/// de espaço de escrita válida abaixo dele. `landing_rsp` deve ser um
/// endereço válido para uso como stack (de kernel ou de usuário, conforme
/// `cs`/`ss`) assim que `entry` começar a executar. `entry` deve apontar
/// para código válido e executável com o privilégio de `cs`.
pub unsafe fn init_stack(stack_top: u64, entry: u64, cs: u16, ss: u16, landing_rsp: u64) -> u64 {
    let frame_size = HW_FRAME_SIZE + SAVED_GPR_COUNT * 8;

    unsafe {
        ((stack_top - 8) as *mut u64).write(ss as u64);
        ((stack_top - 16) as *mut u64).write(landing_rsp);
        ((stack_top - 24) as *mut u64).write(INITIAL_RFLAGS);
        ((stack_top - 32) as *mut u64).write(cs as u64);
        ((stack_top - 40) as *mut u64).write(entry);
        for i in 0..SAVED_GPR_COUNT {
            ((stack_top - 48 - i * 8) as *mut u64).write(0);
        }
    }

    stack_top - frame_size
}

/// Conveniência para o caso mais comum (uma thread de kernel, que executa
/// com os seletores de kernel sobre a própria stack de kernel): calcula o
/// `landing_rsp` já ajustado para o alinhamento de ABI (ver
/// `ABI_ALIGNMENT_PAD`) e delega a [`init_stack`].
///
/// # Safety
/// Mesmos requisitos de [`init_stack`].
pub unsafe fn init_kernel_stack(stack_top: u64, entry: extern "C" fn() -> !) -> u64 {
    unsafe {
        init_stack(stack_top, entry as *const () as u64, KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR, stack_top - ABI_ALIGNMENT_PAD)
    }
}

/// Um processo: dono de um espaço de endereçamento (`AddressSpace`) próprio,
/// isolado do de qualquer outro processo, com pelo menos uma thread. Ver
/// spec `kernel-processes`.
pub struct Process {
    pub id: ProcessId,
    pub address_space: AddressSpace,
    pub threads: Vec<ThreadId>,
}

/// Aloca a stack de kernel de uma nova thread e sintetiza seu frame de
/// entrada em modo kernel. Lógica comum entre `Process::new` e
/// `Process::spawn_thread`.
pub(crate) fn create_kernel_thread<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    process: ProcessId,
    root: u64,
    entry: extern "C" fn() -> !,
) -> Result<Thread, ProcessError> {
    let stack_top = stack::alloc_thread_stack(frame_allocator)?;
    let rsp = unsafe { init_kernel_stack(stack_top, entry) };
    let id = next_thread_id();
    Ok(Thread { id, process, context: Context { rsp, kernel_stack_top: stack_top, address_space_root: root, fpu_state: 0, syscall_rsp_slot: 0, timed_out: false } })
}

/// Aloca a stack de kernel (para troca de contexto/`syscall`) e a stack de
/// usuário (nova página mapeada com `PageFlags { user_accessible: true,
/// writable: true, executable: false }` em `root`) de uma nova thread, e
/// sintetiza seu frame de entrada em anel 3. Lógica comum entre
/// `Process::finish_new_user` e `Process::spawn_user_thread`. Ver spec
/// `kernel-user-mode` e `design.md` da change `kernel-syscalls`.
///
/// `entry` é um endereço (`RIP`) dentro de uma página já mapeada em `root`
/// como acessível por usuário e executável — nunca uma função Rust comum
/// (código do kernel nunca é acessível por usuário).
fn create_user_thread<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    root: u64,
    process: ProcessId,
    entry: u64,
    user_stack_virt_base: u64,
    user_stack_size: u64,
) -> Result<Thread, ProcessError> {
    let kernel_stack_top = stack::alloc_thread_stack(frame_allocator)?;

    let pages = user_stack_size / crate::paging::PAGE_SIZE;
    let mut mapper = Mapper::for_root(frame_allocator, root);
    for i in 0..pages {
        let virt = user_stack_virt_base + i * crate::paging::PAGE_SIZE;
        mapper
            .map_new(virt, PageFlags { writable: true, user_accessible: true, executable: false }, false)
            .map_err(ProcessError::AddressSpace)?;
    }
    let user_stack_top = user_stack_virt_base + user_stack_size;

    let rsp = unsafe {
        init_stack(kernel_stack_top, entry, USER_CODE_SELECTOR, USER_DATA_SELECTOR, user_stack_top - ABI_ALIGNMENT_PAD)
    };
    let id = next_thread_id();
    Ok(Thread { id, process, context: Context { rsp, kernel_stack_top, address_space_root: root, fpu_state: 0, syscall_rsp_slot: 0, timed_out: false } })
}

impl Process {
    /// Cria um novo processo: um espaço de endereçamento próprio (com a
    /// metade do kernel já presente, ver `AddressSpace::new`) e uma
    /// primeira thread, em modo kernel, executando `entry`.
    pub fn new<S: FrameStore>(
        frame_allocator: &mut FrameAllocator<S>,
        entry: extern "C" fn() -> !,
    ) -> Result<(Process, Thread), ProcessError> {
        let address_space = AddressSpace::new(frame_allocator)?;
        let process_id = next_process_id();
        let root = address_space.root();
        let thread = create_kernel_thread(frame_allocator, process_id, root, entry)?;
        let process = Process { id: process_id, address_space, threads: alloc::vec![thread.id] };
        Ok((process, thread))
    }

    /// Primeira fase da criação de um processo de modo usuário: monta só o
    /// espaço de endereçamento, devolvendo-o para que o chamador mapeie
    /// nele o código da thread inicial (ex.: copiando um programa de teste
    /// para uma página de usuário — ver `kernel::user_demo`) antes dela ser
    /// criada. Diferente de `Process::new`, não dá para criar a thread
    /// inicial num único passo aqui: seu endereço de entrada só existe
    /// depois desse mapeamento, já que não há nenhuma função Rust tipada
    /// vivendo em memória acessível por usuário. Ver
    /// [`Process::finish_new_user`].
    pub fn begin_new_user<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<AddressSpace, ProcessError> {
        Ok(AddressSpace::new(frame_allocator)?)
    }

    /// Segunda fase: dado um espaço de endereçamento já preparado por
    /// [`Process::begin_new_user`] (com `entry` já mapeado nele como
    /// acessível por usuário e executável), cria o processo e sua primeira
    /// thread, em modo usuário, com uma stack de usuário própria mapeada em
    /// `[user_stack_virt_base, user_stack_virt_base + user_stack_size)`.
    pub fn finish_new_user<S: FrameStore>(
        frame_allocator: &mut FrameAllocator<S>,
        address_space: AddressSpace,
        entry: u64,
        user_stack_virt_base: u64,
        user_stack_size: u64,
    ) -> Result<(Process, Thread), ProcessError> {
        let process_id = next_process_id();
        let root = address_space.root();
        let thread =
            create_user_thread(frame_allocator, root, process_id, entry, user_stack_virt_base, user_stack_size)?;
        let process = Process { id: process_id, address_space, threads: alloc::vec![thread.id] };
        Ok((process, thread))
    }

    /// Cria uma nova thread adicional neste mesmo processo, em modo kernel,
    /// reusando sua `AddressSpace` já existente (só uma nova stack + contexto).
    pub fn spawn_thread<S: FrameStore>(
        &mut self,
        frame_allocator: &mut FrameAllocator<S>,
        entry: extern "C" fn() -> !,
    ) -> Result<Thread, ProcessError> {
        let root = self.address_space.root();
        let thread = create_kernel_thread(frame_allocator, self.id, root, entry)?;
        self.threads.push(thread.id);
        Ok(thread)
    }

    /// Cria uma nova thread adicional neste mesmo processo, mas executando
    /// `entry` (endereço já mapeado em `self.address_space` como acessível
    /// por usuário e executável) em **modo usuário** (anel 3) — ver
    /// [`create_user_thread`].
    pub fn spawn_user_thread<S: FrameStore>(
        &mut self,
        frame_allocator: &mut FrameAllocator<S>,
        entry: u64,
        user_stack_virt_base: u64,
        user_stack_size: u64,
    ) -> Result<Thread, ProcessError> {
        let root = self.address_space.root();
        let thread =
            create_user_thread(frame_allocator, root, self.id, entry, user_stack_virt_base, user_stack_size)?;
        self.threads.push(thread.id);
        Ok(thread)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn dummy_entry() -> ! {
        loop {}
    }

    #[test]
    fn init_kernel_stack_writes_expected_synthetic_trap_frame() {
        let mut buf = alloc::vec![0xAAu8; 4096];
        let stack_top = buf.as_mut_ptr() as u64 + buf.len() as u64;

        let saved_rsp = unsafe { init_kernel_stack(stack_top, dummy_entry) };

        let read_u64 = |addr: u64| unsafe { (addr as *const u64).read() };

        assert_eq!(read_u64(stack_top - 40), dummy_entry as *const () as u64, "RIP deve apontar para a funcao de entrada");
        assert_eq!(read_u64(stack_top - 32), KERNEL_CODE_SELECTOR as u64, "CS deve ser o seletor de codigo do kernel");
        assert_eq!(read_u64(stack_top - 24) & 0x200, 0x200, "RFLAGS deve ter IF habilitado");
        let landing_rsp = read_u64(stack_top - 16);
        assert_eq!(landing_rsp, stack_top - 8, "campo RSP do frame deve ser o RSP de pouso ajustado para a ABI");
        assert_eq!(landing_rsp % 16, 8, "RSP de pouso deve estar alinhado como se a entrada tivesse sido feita via call");
        assert_eq!(read_u64(stack_top - 8), KERNEL_DATA_SELECTOR as u64, "SS deve ser o seletor de dados do kernel");
        assert_eq!(saved_rsp, stack_top - (HW_FRAME_SIZE + SAVED_GPR_COUNT * 8));
    }

    #[test]
    fn init_stack_for_user_mode_uses_user_selectors_and_user_landing_rsp() {
        let mut kernel_buf = alloc::vec![0xAAu8; 4096];
        let kernel_stack_top = kernel_buf.as_mut_ptr() as u64 + kernel_buf.len() as u64;
        let mut user_buf = alloc::vec![0xBBu8; 4096];
        let user_stack_top = user_buf.as_mut_ptr() as u64 + user_buf.len() as u64;
        let user_landing_rsp = user_stack_top - ABI_ALIGNMENT_PAD;

        let saved_rsp = unsafe {
            init_stack(
                kernel_stack_top,
                dummy_entry as *const () as u64,
                USER_CODE_SELECTOR,
                USER_DATA_SELECTOR,
                user_landing_rsp,
            )
        };

        let read_u64 = |addr: u64| unsafe { (addr as *const u64).read() };

        assert_eq!(read_u64(kernel_stack_top - 32), USER_CODE_SELECTOR as u64, "CS deve ser o seletor de codigo de usuario");
        assert_eq!(read_u64(kernel_stack_top - 32) & 0x3, 3, "CS deve ter RPL 3");
        assert_eq!(read_u64(kernel_stack_top - 8), USER_DATA_SELECTOR as u64, "SS deve ser o seletor de dados de usuario");
        assert_eq!(read_u64(kernel_stack_top - 8) & 0x3, 3, "SS deve ter RPL 3");
        assert_eq!(read_u64(kernel_stack_top - 16), user_landing_rsp, "RSP do frame deve apontar para a stack de usuario, nao a de kernel");
        assert_eq!(saved_rsp, kernel_stack_top - (HW_FRAME_SIZE + SAVED_GPR_COUNT * 8), "rsp salvo continua na stack de kernel");
    }

    #[test]
    fn successive_process_and_thread_ids_are_distinct() {
        let p1 = next_process_id();
        let p2 = next_process_id();
        assert_ne!(p1, p2);

        let t1 = next_thread_id();
        let t2 = next_thread_id();
        assert_ne!(t1, t2);
    }
}
