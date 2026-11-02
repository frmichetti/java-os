//! Processos e threads do kernel — ver `design.md` da change
//! `kernel-process-scheduler`.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::gdt::{KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR};
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

/// Escreve, no topo de uma stack recém-mapeada (`stack_top`, como retornado
/// por `kernel::stack::alloc_thread_stack`), um "trap frame" sintético
/// equivalente ao que a rotina de troca de contexto do escalonador deixaria
/// lá se esta thread já tivesse sido interrompida uma vez — de forma que a
/// primeira vez que o escalonador a retome siga exatamente o mesmo caminho
/// de restauração de qualquer outra thread, sem nenhum caso especial de
/// "primeira execução". Devolve o `rsp` salvo a usar como estado inicial do
/// [`Context`] desta thread.
///
/// Layout, de `stack_top` para baixo (deve bater exatamente com a ordem de
/// empilhar/desempilhar da rotina de troca de contexto em
/// `kernel::scheduler`): `[SS][RSP][RFLAGS][CS][RIP][RAX][RBX][RCX][RDX]
/// [RSI][RDI][RBP][R8][R9][R10][R11][R12][R13][R14][R15]`. O campo `RSP`
/// do frame (não o `rsp` salvo devolvido por esta função, que é bem mais
/// abaixo, no início do bloco de registradores) é o valor que `iretq`
/// efetivamente restaura para `entry` começar a executar — ver
/// `ABI_ALIGNMENT_PAD`.
///
/// # Safety
/// `stack_top` deve ser o topo de uma stack mapeada e exclusiva desta
/// thread, com pelo menos `HW_FRAME_SIZE + SAVED_GPR_COUNT * 8` bytes de
/// espaço de escrita válida abaixo dele.
pub unsafe fn init_stack(stack_top: u64, entry: extern "C" fn() -> !) -> u64 {
    let frame_size = HW_FRAME_SIZE + SAVED_GPR_COUNT * 8;
    let landing_rsp = stack_top - ABI_ALIGNMENT_PAD;

    unsafe {
        ((stack_top - 8) as *mut u64).write(KERNEL_DATA_SELECTOR as u64); // SS
        ((stack_top - 16) as *mut u64).write(landing_rsp); // RSP
        ((stack_top - 24) as *mut u64).write(INITIAL_RFLAGS);
        ((stack_top - 32) as *mut u64).write(KERNEL_CODE_SELECTOR as u64); // CS
        ((stack_top - 40) as *mut u64).write(entry as *const () as u64); // RIP
        for i in 0..SAVED_GPR_COUNT {
            ((stack_top - 48 - i * 8) as *mut u64).write(0);
        }
    }

    stack_top - frame_size
}

/// Um processo: dono de um espaço de endereçamento (`AddressSpace`) próprio,
/// isolado do de qualquer outro processo, com pelo menos uma thread. Ver
/// spec `kernel-processes`.
pub struct Process {
    pub id: ProcessId,
    pub address_space: AddressSpace,
    pub threads: Vec<ThreadId>,
}

impl Process {
    /// Cria um novo processo: um espaço de endereçamento próprio (com a
    /// metade do kernel já presente, ver `AddressSpace::new`) e uma
    /// primeira thread executando `entry`.
    pub fn new<S: FrameStore>(
        frame_allocator: &mut FrameAllocator<S>,
        entry: extern "C" fn() -> !,
    ) -> Result<(Process, Thread), ProcessError> {
        let address_space = AddressSpace::new(frame_allocator)?;
        let process_id = next_process_id();

        let stack_top = stack::alloc_thread_stack(frame_allocator)?;
        let rsp = unsafe { init_stack(stack_top, entry) };
        let thread_id = next_thread_id();
        let thread = Thread { id: thread_id, process: process_id, context: Context { rsp } };

        let process = Process { id: process_id, address_space, threads: alloc::vec![thread_id] };
        Ok((process, thread))
    }

    /// Cria uma nova thread adicional neste mesmo processo, reusando sua
    /// `AddressSpace` já existente (só uma nova stack + contexto).
    pub fn spawn_thread<S: FrameStore>(
        &mut self,
        frame_allocator: &mut FrameAllocator<S>,
        entry: extern "C" fn() -> !,
    ) -> Result<Thread, ProcessError> {
        let stack_top = stack::alloc_thread_stack(frame_allocator)?;
        let rsp = unsafe { init_stack(stack_top, entry) };
        let thread_id = next_thread_id();
        self.threads.push(thread_id);
        Ok(Thread { id: thread_id, process: self.id, context: Context { rsp } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn dummy_entry() -> ! {
        loop {}
    }

    #[test]
    fn init_stack_writes_expected_synthetic_trap_frame() {
        let mut buf = alloc::vec![0xAAu8; 4096];
        let stack_top = buf.as_mut_ptr() as u64 + buf.len() as u64;

        let saved_rsp = unsafe { init_stack(stack_top, dummy_entry) };

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
    fn successive_process_and_thread_ids_are_distinct() {
        let p1 = next_process_id();
        let p2 = next_process_id();
        assert_ne!(p1, p2);

        let t1 = next_thread_id();
        let t2 = next_thread_id();
        assert_ne!(t1, t2);
    }
}
