//! Stack dedicada do kernel, protegida por uma guard page — ver `design.md`
//! da change `kernel-stack-guard-page`.

use crate::paging::{Mapper, PageFlags, PAGE_SIZE};
use crate::phys::{FrameAllocator, FrameError, FrameStore};

/// Endereço virtual do limite inferior (mais baixo) da stack do kernel —
/// faixa isolada, distante da HHDM (índice 256 do PML4), do heap
/// (`heap::HEAP_START`, índice 288) e do código do kernel (índice 511). Ver
/// `design.md` — "Faixa virtual reservada para a stack".
pub const KERNEL_STACK_BASE: u64 = 0xffff_a000_0000_0000;

/// Tamanho fixo da stack do kernel (ver `proposal.md` — "What Changes").
pub const KERNEL_STACK_SIZE: u64 = 64 * 1024;

/// Endereço virtual da página imediatamente abaixo do limite inferior da
/// stack — deliberadamente nunca mapeada (ver [`map_stack`]).
pub const GUARD_PAGE_ADDR: u64 = KERNEL_STACK_BASE - PAGE_SIZE;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StackInitError {
    /// Falha ao mapear alguma página da stack (ex.: sem frames físicos livres).
    MapFailed,
}

impl From<FrameError> for StackInitError {
    fn from(_: FrameError) -> Self {
        StackInitError::MapFailed
    }
}

/// Mapeia as `KERNEL_STACK_SIZE` bytes da stack do kernel a partir de
/// `KERNEL_STACK_BASE`, deixando a página em `GUARD_PAGE_ADDR` sem mapear
/// (nunca é referenciada por este mapeamento — permanece ausente por
/// construção). Retorna o endereço do topo da stack (`KERNEL_STACK_BASE +
/// KERNEL_STACK_SIZE`), o maior endereço um byte além do último mapeado.
///
/// Deliberadamente separada de [`switch_to`]: esta função só precisa do
/// alocador de frames (tipicamente obtido de um lock), e deve ser chamada —
/// com esse lock liberado por escopo normal logo em seguida — antes de
/// `switch_to`. Ver `design.md` — "Mecanismo de troca de stack" para o
/// porquê dessa separação (um guard de lock mantido vivo apenas por ser
/// passado como argumento nunca seria liberado, já que `switch_to` não
/// retorna — o destrutor do guard nunca rodaria, deixando o lock preso para
/// sempre).
pub fn map<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<u64, StackInitError> {
    let mut mapper = Mapper::new(frame_allocator);
    let pages = KERNEL_STACK_SIZE / PAGE_SIZE;
    for i in 0..pages {
        let virt = KERNEL_STACK_BASE + i * PAGE_SIZE;
        mapper.map_new(virt, PageFlags::KERNEL_RW, false).map_err(|_| StackInitError::MapFailed)?;
    }
    Ok(KERNEL_STACK_BASE + KERNEL_STACK_SIZE)
}

/// Troca `RSP` para o topo da stack dedicada do kernel (`stack_top`, como
/// retornado por [`map`]) e transfere a execução para `continuation`, que
/// nunca retorna.
///
/// # Safety
/// - Deve ser chamada só depois de [`map`] ter mapeado com sucesso a stack
///   cujo topo é `stack_top`.
/// - Nenhum recurso cujo `Drop` dependa de um retorno normal da função
///   chamadora (locks, guards, RAII em geral) pode continuar vivo no ponto
///   da chamada — como esta função nunca retorna, nenhum desses destrutores
///   rodaria. Em particular, nenhum lock deve estar retido no momento da
///   chamada.
/// - `continuation` não deve depender de nenhum estado vivo apenas na stack
///   atual (variáveis locais, endereços de retorno) — apenas de `static`s
///   globais, já que a stack antiga deixa de ser referenciada após a troca.
pub unsafe fn switch_to(stack_top: u64, continuation: extern "C" fn() -> !) -> ! {
    // `stack_top` é múltiplo de PAGE_SIZE (logo múltiplo de 16); subtrai 8
    // para simular o estado que um `call` normal teria deixado (RSP % 16 ==
    // 8 na entrada da função chamada), já que o `jmp` abaixo não empilha
    // nenhum endereço de retorno.
    let new_rsp = stack_top - 8;

    unsafe {
        core::arch::asm!(
            "mov rsp, {new_rsp}",
            "jmp {continuation}",
            new_rsp = in(reg) new_rsp,
            continuation = in(reg) continuation,
            options(noreturn),
        );
    }
}
