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

/// Base da janela virtual reservada a stacks de usuário de threads
/// adicionais criadas via `SYSCALL_THREAD_CREATE` — metade inferior do
/// endereçamento (privada por processo, ao contrário do pool de stacks de
/// kernel acima, que vive na metade superior compartilhada). Ver
/// `design.md` da change `kernel-user-thread-create` — "Janela de stack de
/// usuário dedicada a threads adicionais...".
///
/// **Achado real durante a implementação:** `design.md` originalmente
/// propunha `0x0000_1000_0000_0000`, achando esse endereço livre — na
/// verdade já é `USER_DEMO_CODE_VIRT` em `kernel::scheduler`. A faixa
/// `0x0000_01xx` até `0x0000_7xxx` (incrementos de `0x0000_0100_0000_0000`)
/// já está inteiramente ocupada por constantes `*_CODE_VIRT`/`*_STACK_VIRT`
/// de programas de teste existentes. Usa-se aqui o primeiro incremento
/// livre dessa mesma faixa, `0x0000_0f00_0000_0000` (entre
/// `EFAULT_TEST_PROGRAM_STACK_VIRT = 0x0000_0e00_...` e
/// `USER_DEMO_CODE_VIRT = 0x0000_1000_...`), com `USER_THREAD_STACK_SLOT_STRIDE`
/// bytes de sobra antes do próximo endereço já em uso — espaço suficiente
/// para um número de threads muito maior que o que este kernel cria hoje.
pub const USER_THREAD_STACK_POOL_BASE: u64 = 0x0000_0f00_0000_0000;

/// Tamanho máximo de stack de usuário que um slot desta janela reserva.
/// `kernel::syscall::sys_thread_create` rejeita (`EINVAL`) um pedido maior
/// que isto antes de alocar qualquer slot.
pub const USER_THREAD_STACK_MAX_SIZE: u64 = 8 * 1024 * 1024;

/// Distância entre o início de um slot desta janela e o do próximo: guard
/// page (4 KiB) + o tamanho máximo reservado por slot.
pub const USER_THREAD_STACK_SLOT_STRIDE: u64 = PAGE_SIZE + USER_THREAD_STACK_MAX_SIZE;

/// Próximo índice de slot livre da janela de stacks de usuário de threads
/// adicionais. Contador **global** (não por processo) — seguro porque dois
/// processos diferentes têm `CR3`/PML4 diferentes, então o mesmo endereço
/// virtual de slot nunca colide entre processos (ver design.md). Mesma
/// política de "nunca recicla" que [`NEXT_THREAD_STACK_SLOT`] já usa para
/// stacks de kernel.
static NEXT_USER_THREAD_STACK_SLOT: AtomicU64 = AtomicU64::new(0);

fn next_user_thread_stack_slot_index() -> u64 {
    NEXT_USER_THREAD_STACK_SLOT.fetch_add(1, Ordering::Relaxed)
}

/// Endereço da guard page, do limite inferior e do topo da stack do slot
/// `index` desta janela, para uma stack de `mapped_size` bytes (já
/// arredondado para página). Função pura, testável sem hardware.
fn user_thread_stack_slot_addresses(index: u64, mapped_size: u64) -> (u64, u64, u64) {
    let slot_base = USER_THREAD_STACK_POOL_BASE + index * USER_THREAD_STACK_SLOT_STRIDE;
    let guard_page = slot_base;
    let stack_base = slot_base + PAGE_SIZE;
    let stack_top = stack_base + mapped_size;
    (guard_page, stack_base, stack_top)
}

/// Reserva o endereço virtual base do próximo slot livre da janela de
/// stacks de usuário de threads adicionais, deixando a guard page do slot
/// deliberadamente sem mapear — sem mapear nada além disso.
///
/// Deliberadamente não mapeia a stack em si, ao contrário de
/// [`alloc_thread_stack`] (stacks de kernel): esta janela vive na metade
/// inferior do endereçamento — privada por processo — e mapear exigiria a
/// `root` (PML4) do processo específico, que esta função não recebe. Quem
/// chama [`next_user_thread_stack_base`] (`kernel::syscall::sys_thread_create`)
/// repassa o endereço devolvido para `Process::spawn_user_thread`, que já
/// mapeia a stack de usuário via `Mapper::for_root` — ver `design.md` da
/// change `kernel-user-thread-create`, "Resultado real da implementação"
/// (evita mapear a mesma stack duas vezes).
pub fn next_user_thread_stack_base() -> u64 {
    let index = next_user_thread_stack_slot_index();
    let (_guard_page, stack_base, _stack_top) = user_thread_stack_slot_addresses(index, 0);
    stack_base
}

/// Base da janela virtual reservada a blocos de TLS (armazenamento local de
/// thread) de threads de usuário — metade inferior do endereçamento
/// (privada por processo, mesmo racional de
/// [`USER_THREAD_STACK_POOL_BASE`]). Primeiro incremento livre da mesma
/// faixa de `0x0000_0100_0000_0000` depois de
/// `SIBLING_THREAD_TEST_PROGRAM_STACK_VIRT = 0x0000_1100_...`
/// (`kernel::scheduler`), ver `design.md` da change
/// `kernel-thread-local-storage`.
pub const TLS_BLOCK_POOL_BASE: u64 = 0x0000_1200_0000_0000;

/// Distância entre o início de um slot desta janela e o do próximo — um
/// único slot por página (4 KiB): ao contrário das janelas de stack acima,
/// um bloco de TLS não precisa de guard page (tamanho fixo, nunca cresce
/// dinamicamente como uma stack).
pub const TLS_BLOCK_POOL_STRIDE: u64 = PAGE_SIZE;

/// Offset fixo, dentro de um slot, onde o TCB (bloco de controle de thread —
/// uma palavra de 8 bytes com um ponteiro para si mesma, convenção do
/// modelo ELF TLS "Variant II") fica. `FS.base` da thread = endereço do TCB.
/// `2048` (meio da página) é múltiplo de qualquer alinhamento prático
/// (8/16/32/64), então `TLS_TCB_OFFSET - round_up(memsz, align)` (o início
/// da imagem de `.tdata`/`.tbss`) sempre cai alinhado corretamente, sem
/// nenhuma lógica de arredondamento de endereço adicional. Ver design.md —
/// "Modelo ELF TLS "Variant II" com TCB em offset fixo dentro do slot".
pub const TLS_TCB_OFFSET: u64 = 2048;

/// Tamanho máximo (`memsz`) de um bloco de TLS que um slot desta janela
/// comporta — ver design.md, "Limite de tamanho". Quem extrai o segmento
/// `PT_TLS` de um executável (`kernel::scheduler::spawn_from_elf`) rejeita
/// um `memsz` maior que isto antes de chegar a alocar qualquer slot.
pub const TLS_BLOCK_MAX_MEMSZ: u64 = TLS_TCB_OFFSET;

/// Próximo índice de slot livre da janela de blocos de TLS. Contador
/// **global** (não por processo) — mesmo racional de
/// [`NEXT_USER_THREAD_STACK_SLOT`]: endereços virtuais de processos
/// diferentes nunca colidem (PML4s diferentes).
static NEXT_TLS_BLOCK_SLOT: AtomicU64 = AtomicU64::new(0);

fn next_tls_block_slot_index() -> u64 {
    NEXT_TLS_BLOCK_SLOT.fetch_add(1, Ordering::Relaxed)
}

/// Endereço base do slot `index` desta janela. Função pura, testável sem
/// hardware.
fn tls_block_slot_base(index: u64) -> u64 {
    TLS_BLOCK_POOL_BASE + index * TLS_BLOCK_POOL_STRIDE
}

/// Dado o endereço base de um slot e o `memsz`/`align` de um bloco de TLS,
/// devolve o endereço do TCB (`FS.base` da thread) e o endereço onde a
/// imagem de `.tdata`/`.tbss` começa (`tcb_addr - round_up(memsz,
/// align)`). Função pura, testável sem hardware — ver design.md.
///
/// `align` é tratado como `1` se for `0` (nenhum alinhamento declarado —
/// `round_up` com divisor `0` seria divisão por zero).
pub(crate) fn tls_block_addresses(slot_base: u64, memsz: u64, align: u64) -> (u64, u64) {
    let align = if align == 0 { 1 } else { align };
    let tcb_addr = slot_base + TLS_TCB_OFFSET;
    let module_size = memsz.div_ceil(align) * align;
    let image_start = tcb_addr - module_size;
    (tcb_addr, image_start)
}

/// Reserva o endereço base do próximo slot livre da janela de blocos de
/// TLS, sem mapear nada — mesmo racional de [`next_user_thread_stack_base`]:
/// mapear exigiria a `root` do processo específico, que esta função não
/// recebe. Quem chama [`next_tls_block_slot_base`]
/// (`kernel::process::init_tls_block`) mapeia a página e escreve o
/// conteúdo do bloco via `Mapper::for_root` + o alias HHDM, mesmo padrão já
/// usado por `kernel::elf::load_segment`.
pub fn next_tls_block_slot_base() -> u64 {
    let index = next_tls_block_slot_index();
    tls_block_slot_base(index)
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

    #[test]
    fn user_thread_stack_pool_base_is_distinct_from_kernel_pools() {
        assert_ne!(USER_THREAD_STACK_POOL_BASE, KERNEL_STACK_BASE);
        assert_ne!(USER_THREAD_STACK_POOL_BASE, THREAD_STACK_POOL_BASE);
    }

    #[test]
    fn user_thread_stack_slot_guard_page_sits_immediately_below_its_stack() {
        let (guard_page, stack_base, stack_top) = user_thread_stack_slot_addresses(0, PAGE_SIZE * 4);
        assert_eq!(guard_page + PAGE_SIZE, stack_base);
        assert_eq!(stack_base + PAGE_SIZE * 4, stack_top);
    }

    #[test]
    fn user_thread_stack_consecutive_slots_never_overlap_even_at_max_size() {
        // O slot usa sempre a mesma distância fixa (USER_THREAD_STACK_MAX_SIZE),
        // mesmo quando a alocação real pede menos — o próximo slot nunca pode
        // se sobrepor, mesmo no caso de o slot anterior ter sido mapeado com o
        // tamanho máximo permitido.
        let (_, _, top0) = user_thread_stack_slot_addresses(0, USER_THREAD_STACK_MAX_SIZE);
        let (guard1, _, _) = user_thread_stack_slot_addresses(1, PAGE_SIZE);
        assert_eq!(top0, guard1, "o fim do slot 0 (tamanho máximo) deve coincidir exatamente com a guard page do slot 1");
    }

    #[test]
    fn successive_user_thread_stack_slot_allocations_are_distinct() {
        let a = next_user_thread_stack_slot_index();
        let b = next_user_thread_stack_slot_index();
        assert_ne!(a, b);
    }

    #[test]
    fn successive_user_thread_stack_bases_are_distinct_and_page_aligned() {
        let a = next_user_thread_stack_base();
        let b = next_user_thread_stack_base();
        assert_ne!(a, b);
        assert_eq!(a % PAGE_SIZE, 0);
        assert_eq!(b % PAGE_SIZE, 0);
    }

    #[test]
    fn tls_block_pool_base_is_distinct_from_every_other_window() {
        assert_ne!(TLS_BLOCK_POOL_BASE, KERNEL_STACK_BASE);
        assert_ne!(TLS_BLOCK_POOL_BASE, THREAD_STACK_POOL_BASE);
        assert_ne!(TLS_BLOCK_POOL_BASE, USER_THREAD_STACK_POOL_BASE);
    }

    #[test]
    fn tls_tcb_offset_is_a_multiple_of_every_practical_alignment() {
        for align in [8u64, 16, 32, 64] {
            assert_eq!(TLS_TCB_OFFSET % align, 0);
        }
    }

    #[test]
    fn tls_block_addresses_image_start_is_aligned_and_below_tcb() {
        let (tcb_addr, image_start) = tls_block_addresses(0, 8, 16);
        assert_eq!(tcb_addr, TLS_TCB_OFFSET);
        assert_eq!(image_start % 16, 0, "inicio da imagem deve respeitar o alinhamento pedido");
        assert!(image_start < tcb_addr, "imagem deve ficar em offset negativo em relacao ao TCB");
        assert_eq!(tcb_addr - image_start, 16, "memsz=8 arredondado para align=16 ocupa 16 bytes");
    }

    #[test]
    fn tls_block_addresses_rounds_memsz_up_to_alignment() {
        // memsz=10, align=8 -> arredonda para 16 (proximo multiplo de 8).
        let (tcb_addr, image_start) = tls_block_addresses(0, 10, 8);
        assert_eq!(tcb_addr - image_start, 16);
    }

    #[test]
    fn tls_block_addresses_treats_zero_alignment_as_one() {
        let (tcb_addr, image_start) = tls_block_addresses(0, 5, 0);
        assert_eq!(tcb_addr - image_start, 5);
    }

    #[test]
    fn successive_tls_block_slots_do_not_overlap() {
        let a = next_tls_block_slot_base();
        let b = next_tls_block_slot_base();
        assert_ne!(a, b);
        assert_eq!(b - a, TLS_BLOCK_POOL_STRIDE);
    }
}
