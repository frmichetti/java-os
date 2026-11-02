//! Stack dedicada do kernel, protegida por uma guard page — ver `design.md`
//! da change `kernel-stack-guard-page`. Também o pool de stacks de thread
//! (uma por thread, mesma proteção de guard page) — ver `design.md` da
//! change `kernel-process-scheduler`.

use core::sync::atomic::{AtomicU64, Ordering};

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

/// Base da faixa virtual reservada ao pool de stacks de thread — faixa
/// isolada, distante da HHDM (índice 256), do heap (índice 288) e da stack
/// principal do kernel (índice 320). Ver `design.md` da change
/// `kernel-process-scheduler` — "Stack de cada thread".
pub const THREAD_STACK_POOL_BASE: u64 = 0xffff_b000_0000_0000;

/// Distância, em bytes, entre o início de um slot do pool e o do próximo:
/// guard page (4 KiB) + stack (mesmo tamanho da stack principal do kernel).
pub const SLOT_STRIDE: u64 = PAGE_SIZE + KERNEL_STACK_SIZE;

/// Endereço da guard page, do limite inferior e do topo da stack do slot
/// `index` do pool. Função pura, testável sem hardware.
fn slot_addresses(index: u64) -> (u64, u64, u64) {
    let slot_base = THREAD_STACK_POOL_BASE + index * SLOT_STRIDE;
    let guard_page = slot_base;
    let stack_base = slot_base + PAGE_SIZE;
    let stack_top = stack_base + KERNEL_STACK_SIZE;
    (guard_page, stack_base, stack_top)
}

/// Próximo índice de slot livre do pool de stacks de thread. Sem
/// reciclagem (ver `design.md` — Non-Goals): cada chamada devolve um índice
/// novo, nunca reaproveitado.
static NEXT_THREAD_STACK_SLOT: AtomicU64 = AtomicU64::new(0);

fn next_slot_index() -> u64 {
    NEXT_THREAD_STACK_SLOT.fetch_add(1, Ordering::Relaxed)
}

/// Garante que a cadeia de tabelas intermediárias (PDPT/PD/PT) do pool de
/// stacks de thread já exista na paginação ativa — chamada uma única vez,
/// **antes** de qualquer `AddressSpace::new()` (criação de processo) pela
/// primeira vez.
///
/// **Achado durante a implementação da change `kernel-syscalls`:**
/// `AddressSpace::new()` copia as entradas de PML4 256-511 *por valor*, no
/// momento em que é chamada. Se o índice de PML4 do pool (`THREAD_STACK_POOL_BASE`)
/// ainda não tiver uma PDPT criada nesse momento (porque nenhuma stack de
/// thread foi alocada ainda), a cópia registra esse índice como "ausente" —
/// e uma stack alocada *depois* disso (o caso comum: a primeira thread de um
/// processo recém-criado) nunca fica visível para esse processo quando ele
/// for escalonado, mesmo a tabela existindo na PML4 ativa a partir daí (só a
/// entrada de PML4 em si é compartilhada por referência; criar essa entrada
/// *depois* de uma cópia não propaga retroativamente). Ver design.md.
///
/// Mapeia e desmapeia imediatamente uma página de disfarce (a primeira
/// página real do slot 0, não a guard page): a tabela intermediária criada
/// como efeito colateral do mapeamento persiste mesmo depois do
/// desmapeamento (que só remove a entrada-folha).
///
/// **Limitação conhecida, aceitável no escopo atual:** isso garante a PDPT
/// (e, na prática, também a PD/PT da própria página de disfarce) do índice
/// de PML4 do pool, mas uma `PT` cobre ~30 slots (2 MiB / `SLOT_STRIDE`) —
/// o pool crescer além disso exigiria uma nova PT, sujeita ao mesmo
/// problema se criada depois de outra `AddressSpace::new()`. Aceitável
/// porque este kernel ainda cria poucas threads; revisitar se/quando isso
/// deixar de ser verdade.
pub fn prime_thread_stack_pool<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<(), StackInitError> {
    let probe_virt = THREAD_STACK_POOL_BASE + PAGE_SIZE;
    let mut mapper = Mapper::new(frame_allocator);
    mapper.map_new(probe_virt, PageFlags::KERNEL_RW, false).map_err(|_| StackInitError::MapFailed)?;
    mapper.unmap(probe_virt).map_err(|_| StackInitError::MapFailed)?;
    Ok(())
}

/// Aloca o próximo slot livre do pool e mapeia sua stack (64 KiB),
/// deixando a guard page do slot deliberadamente sem mapear. Retorna o topo
/// da stack alocada, pronta para `process::init_stack`.
///
/// Sempre mapeada via `Mapper::new` (a paginação atualmente ativa), nunca
/// via `Mapper::for_root` de um processo específico: a stack de uma thread
/// é memória de kernel (ring 0) e, como tal, deve estar igualmente visível
/// em toda e qualquer address space — exatamente a razão de o pool viver na
/// faixa compartilhada do PML4 (índices 256-511, ver design.md). Mapear via
/// `for_root` sobre a PML4 recém-criada e nunca ativada de um processo faria
/// a nova tabela intermediária (PDPT/PD/PT) existir só *naquela* PML4
/// específica — não "voltar" a ser compartilhada com a PML4 realmente
/// ativa (a do kernel), causando um page fault na primeira escrita real na
/// stack (achado ao testar a tarefa 5.2 em boot real: a CPU nunca troca de
/// `CR3` para threads kernel-mode nesta change, então a stack precisa estar
/// mapeada onde quer que a PML4 ativa esteja, desde o início).
pub fn alloc_thread_stack<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<u64, StackInitError> {
    let index = next_slot_index();
    let (_guard_page, stack_base, stack_top) = slot_addresses(index);

    let mut mapper = Mapper::new(frame_allocator);
    let pages = KERNEL_STACK_SIZE / PAGE_SIZE;
    for i in 0..pages {
        let virt = stack_base + i * PAGE_SIZE;
        mapper.map_new(virt, PageFlags::KERNEL_RW, false).map_err(|_| StackInitError::MapFailed)?;
    }
    Ok(stack_top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_guard_page_sits_immediately_below_its_stack() {
        let (guard_page, stack_base, stack_top) = slot_addresses(0);
        assert_eq!(guard_page + PAGE_SIZE, stack_base);
        assert_eq!(stack_base + KERNEL_STACK_SIZE, stack_top);
    }

    #[test]
    fn consecutive_slots_do_not_overlap() {
        let (_, _, top0) = slot_addresses(0);
        let (guard1, _, _) = slot_addresses(1);
        assert_eq!(top0, guard1, "o fim do slot 0 deve coincidir com a guard page do slot 1, sem lacuna nem sobreposição");
    }

    #[test]
    fn successive_slot_allocations_are_distinct() {
        let a = next_slot_index();
        let b = next_slot_index();
        assert_ne!(a, b);
    }
}
