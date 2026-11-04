//! Reserva, comprometimento, descomprometimento, proteção e liberação de
//! faixas de memória virtual anônima — a base de `mmap`/`munmap`/`mprotect`
//! POSIX. Ver spec `kernel-virtual-memory` e `design.md` da change
//! `kernel-posix-virtual-memory`.
//!
//! Dividido em duas camadas, seguindo o mesmo padrão de `kernel::heap`/
//! `kernel::paging` (lógica pura testável em host, separada do código que
//! toca hardware de verdade):
//! - [`RangeAllocator`]/[`ReservationTable`]: contabilidade pura (faixas
//!   virtuais livres, estado de comprometimento por página, handles com
//!   geração) — sem nenhum acesso a [`Mapper`]/[`FrameAllocator`].
//! - [`Vmm`]: liga essa contabilidade a `Mapper`/`FrameAllocator` reais.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::hhdm;
use crate::paging::{MapError, Mapper, PageFlags, PAGE_SIZE};
use crate::phys::{FrameAllocator, FrameStore};
use crate::sync::Spinlock;

/// Primeiro endereço virtual da faixa fixa reservada para `kernel::vmm`:
/// índice 384 da PML4 — ver design.md, "Faixa virtual fixa para reservas".
///
/// **Correção (change `kernel-safepoint-signal`):** originalmente este valor
/// era `0xffff_b000_0000_0000` (índice 352), a partir da suposição (errada)
/// de que seria "o primeiro índice livre após a stack dedicada do kernel
/// (índice 320)" — mas o índice 352 já estava ocupado por
/// `kernel::stack::THREAD_STACK_POOL_BASE` (o pool de stacks de kernel de
/// cada thread, de `kernel-process-scheduler`, uma change anterior). Como
/// nenhuma verificação anterior exercitava `kernel::vmm` e o pool de stacks
/// ao mesmo tempo além de poucas páginas, a colisão ficou latente até o demo
/// de `kernel-safepoint-signal` tentar comprometer uma página que já era a
/// stack de kernel viva de outra thread — `Mapper::map` rejeitou com
/// `AlreadyMapped` (ou, pior, corromperia a stack silenciosamente se
/// `allow_replace` fosse `true`). Ver `docs/architecture/overview.md`, seção
/// 1, para o índice de PML4 de cada faixa reservada.
pub const VMM_WINDOW_START: u64 = 0xffff_c000_0000_0000;

/// Tamanho da sub-faixa de fato gerenciada pelo alocador de faixas, dentro
/// do índice de PML4 reservado (que cobre 512 GiB inteiros).
pub const VMM_WINDOW_SIZE: u64 = 64 * 1024 * 1024 * 1024; // 64 GiB

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum VmmError {
    /// `size` (de `reserve`) é zero, ou `offset`/`len` não é múltiplo de
    /// `PAGE_SIZE`, ou `len` é zero.
    Misaligned,
    /// Não há espaço virtual livre suficiente, dentro da janela gerenciada,
    /// para a reserva solicitada.
    OutOfVirtualSpace,
    /// `offset`/`len` ultrapassa os limites da reserva.
    OutOfBounds,
    /// A sub-região solicitada (para `commit`) já está total ou
    /// parcialmente comprometida.
    AlreadyCommitted,
    /// A sub-região solicitada (para `decommit`/`protect`) não está
    /// totalmente comprometida.
    NotCommitted,
    /// O `Region` apresentado não corresponde a uma reserva viva — ou foi
    /// liberada, ou pertence a uma geração anterior reaproveitada por outra
    /// reserva no mesmo endereço (ver design.md, "`Region` como handle
    /// opaco com geração").
    StaleHandle,
    /// O alocador de frames físicos não tem mais memória.
    OutOfFrames,
}

// ---------------------------------------------------------------------
// Camada pura: alocador de faixas de endereço virtual livres.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FreeRange {
    base: u64,
    size: u64,
}

/// Alocador de faixas de endereço virtual livres dentro de uma janela fixa —
/// lógica pura, sem nenhum acesso a `Mapper`/`FrameAllocator`. *First-fit*
/// com *split*; `release` funde (*coalesce*) com vizinhas adjacentes — ver
/// design.md, "Alocador de faixas livres: lista ordenada com coalescência,
/// *first-fit*".
///
/// Invariante mantida por toda a vida da struct: `free` permanece ordenado
/// por `base`, sem faixas sobrepostas, e sem duas faixas livres adjacentes
/// (toda fronteira comum já foi fundida).
struct RangeAllocator {
    free: Vec<FreeRange>,
}

impl RangeAllocator {
    const fn empty() -> Self {
        Self { free: Vec::new() }
    }

    /// Registra `base..base+size` como a janela gerenciada inicial. Deve ser
    /// chamada uma única vez, antes de qualquer `allocate`/`release`.
    fn add_window(&mut self, base: u64, size: u64) {
        self.free.push(FreeRange { base, size });
    }

    /// Busca a primeira faixa livre grande o suficiente para `size` bytes
    /// (*first-fit*) e a retira do pool, fazendo *split* se sobrar espaço.
    /// `None` se não houver nenhuma faixa livre grande o suficiente.
    fn allocate(&mut self, size: u64) -> Option<u64> {
        let idx = self.free.iter().position(|r| r.size >= size)?;
        let range = self.free[idx];
        if range.size == size {
            self.free.remove(idx);
        } else {
            self.free[idx] = FreeRange { base: range.base + size, size: range.size - size };
        }
        Some(range.base)
    }

    /// Devolve `base..base+size` ao pool de faixas livres, fundindo com uma
    /// vizinha imediatamente anterior e/ou seguinte, se livres.
    fn release(&mut self, base: u64, size: u64) {
        let insert_at = self.free.partition_point(|r| r.base < base);
        self.free.insert(insert_at, FreeRange { base, size });

        // Funde com a vizinha seguinte primeiro: fundir essa não desloca o
        // índice de uma eventual vizinha anterior ainda não tratada.
        if insert_at + 1 < self.free.len() {
            let next = self.free[insert_at + 1];
            let cur = self.free[insert_at];
            if cur.base + cur.size == next.base {
                self.free[insert_at].size += next.size;
                self.free.remove(insert_at + 1);
            }
        }
        if insert_at > 0 {
            let prev = self.free[insert_at - 1];
            let cur = self.free[insert_at];
            if prev.base + prev.size == cur.base {
                self.free[insert_at - 1].size += cur.size;
                self.free.remove(insert_at);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Camada pura: reservas vivas, com handle opaco e geração.
// ---------------------------------------------------------------------

/// Handle opaco de uma reserva de memória virtual, devolvido por
/// [`Vmm::reserve`]. Ver design.md, "`Region` como handle opaco com
/// geração" — a `generation` permite detectar uso de um handle depois da
/// reserva ter sido liberada (`VmmError::StaleHandle`), mesmo que o
/// endereço base tenha sido reaproveitado por uma reserva nova.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    base: u64,
    size: u64,
    generation: u64,
}

impl Region {
    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

struct ReservationState {
    size: u64,
    generation: u64,
    /// `committed[i]` é `true` se a página `i` (a partir de `base`) está
    /// comprometida.
    committed: Vec<bool>,
}

/// Contabilidade pura das reservas vivas: handles com geração, limites e
/// estado de comprometimento por página — sem nenhum acesso a
/// `Mapper`/`FrameAllocator`. Ver design.md, "`Region` como handle opaco
/// com geração".
struct ReservationTable {
    reservations: BTreeMap<u64, ReservationState>,
    next_generation: u64,
}

impl ReservationTable {
    const fn empty() -> Self {
        Self { reservations: BTreeMap::new(), next_generation: 0 }
    }

    fn reserve(&mut self, base: u64, size: u64) -> Region {
        let generation = self.next_generation;
        self.next_generation += 1;
        let pages = (size / PAGE_SIZE) as usize;
        self.reservations.insert(base, ReservationState { size, generation, committed: alloc::vec![false; pages] });
        Region { base, size, generation }
    }

    fn lookup(&self, region: &Region) -> Result<&ReservationState, VmmError> {
        match self.reservations.get(&region.base) {
            Some(state) if state.generation == region.generation => Ok(state),
            _ => Err(VmmError::StaleHandle),
        }
    }

    fn lookup_mut(&mut self, region: &Region) -> Result<&mut ReservationState, VmmError> {
        match self.reservations.get_mut(&region.base) {
            Some(state) if state.generation == region.generation => Ok(state),
            _ => Err(VmmError::StaleHandle),
        }
    }

    fn page_range(state: &ReservationState, offset: u64, len: u64) -> Result<core::ops::Range<usize>, VmmError> {
        if offset % PAGE_SIZE != 0 || len % PAGE_SIZE != 0 || len == 0 {
            return Err(VmmError::Misaligned);
        }
        let end = offset.checked_add(len).ok_or(VmmError::OutOfBounds)?;
        if end > state.size {
            return Err(VmmError::OutOfBounds);
        }
        let start_page = (offset / PAGE_SIZE) as usize;
        let end_page = (end / PAGE_SIZE) as usize;
        Ok(start_page..end_page)
    }

    /// Valida que `offset..offset+len` está dentro dos limites da reserva e
    /// totalmente livre (nenhuma página já comprometida), sem alterar nada —
    /// cobre "nenhum frame físico SHALL ser alocado como efeito colateral"
    /// de um comprometimento rejeitado.
    fn validate_commit(&self, region: &Region, offset: u64, len: u64) -> Result<(), VmmError> {
        let state = self.lookup(region)?;
        let pages = Self::page_range(state, offset, len)?;
        if state.committed[pages].iter().any(|&c| c) {
            return Err(VmmError::AlreadyCommitted);
        }
        Ok(())
    }

    /// Marca `offset..offset+len` como comprometida. Só deve ser chamada
    /// depois de `validate_commit` ter sido aceita e de todo o mapeamento
    /// real correspondente já ter sido feito com sucesso.
    fn mark_committed(&mut self, region: &Region, offset: u64, len: u64) {
        let state = self.lookup_mut(region).expect("region validada por validate_commit deveria continuar valida");
        let pages = Self::page_range(state, offset, len).expect("limites ja validados por validate_commit");
        for committed in &mut state.committed[pages] {
            *committed = true;
        }
    }

    /// Valida que `offset..offset+len` está dentro dos limites da reserva e
    /// totalmente comprometida — usado por `decommit` e `protect`.
    fn validate_fully_committed(&self, region: &Region, offset: u64, len: u64) -> Result<(), VmmError> {
        let state = self.lookup(region)?;
        let pages = Self::page_range(state, offset, len)?;
        if state.committed[pages].iter().any(|&c| !c) {
            return Err(VmmError::NotCommitted);
        }
        Ok(())
    }

    /// Marca `offset..offset+len` como não comprometida. Só deve ser
    /// chamada depois de `validate_fully_committed` ter sido aceita e de
    /// todo o desmapeamento real correspondente já ter sido feito.
    fn mark_decommitted(&mut self, region: &Region, offset: u64, len: u64) {
        let state = self.lookup_mut(region).expect("region validada por validate_fully_committed deveria continuar valida");
        let pages = Self::page_range(state, offset, len).expect("limites ja validados por validate_fully_committed");
        for committed in &mut state.committed[pages] {
            *committed = false;
        }
    }

    /// Busca a reserva viva cujo intervalo `[base, base+size)` contém
    /// `addr` — ver design.md da change `kernel-jvm-posix-syscalls`, "VM por
    /// endereço". `None` se `addr` não cair dentro de nenhuma reserva viva
    /// (incluindo uma já liberada: `remove` já apagou sua entrada deste
    /// `BTreeMap`).
    fn region_containing(&self, addr: u64) -> Option<Region> {
        let (&base, state) = self.reservations.range(..=addr).next_back()?;
        if addr < base + state.size {
            Some(Region { base, size: state.size, generation: state.generation })
        } else {
            None
        }
    }

    /// Remove a reserva por completo, devolvendo os offsets (em bytes, a
    /// partir da base da reserva) de cada página que ainda estava
    /// comprometida — para o chamador descomprometer de verdade antes de
    /// devolver o espaço virtual ao `RangeAllocator`.
    fn remove(&mut self, region: &Region) -> Result<Vec<u64>, VmmError> {
        self.lookup(region)?; // valida a geração antes de mexer em qualquer coisa
        let state = self.reservations.remove(&region.base).expect("presenca ja confirmada por lookup");
        let committed_offsets = state
            .committed
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c)
            .map(|(i, _)| i as u64 * PAGE_SIZE)
            .collect();
        Ok(committed_offsets)
    }
}

// ---------------------------------------------------------------------
// Camada real: liga a contabilidade pura a `Mapper`/`FrameAllocator`.
// ---------------------------------------------------------------------

/// Reserva, comprometimento, descomprometimento, proteção e liberação de
/// memória virtual anônima sobre a janela fixa (`VMM_WINDOW_START`/
/// `VMM_WINDOW_SIZE`) e o espaço de endereçamento atualmente ativo. Ver
/// spec `kernel-virtual-memory` e design.md.
pub struct Vmm {
    ranges: RangeAllocator,
    table: ReservationTable,
}

impl Vmm {
    pub const fn new() -> Self {
        Self { ranges: RangeAllocator::empty(), table: ReservationTable::empty() }
    }

    /// Registra a janela fixa de endereço virtual como o espaço gerenciado
    /// inicial. Deve ser chamada uma única vez, depois do heap do kernel
    /// estar pronto (esta contabilidade usa `alloc::{Vec, BTreeMap}`) e
    /// antes de qualquer `reserve`.
    pub fn init(&mut self) {
        self.ranges.add_window(VMM_WINDOW_START, VMM_WINDOW_SIZE);
    }

    /// Traduz um endereço virtual para o `Region` da reserva viva que o
    /// contém, se houver — ver design.md da change `kernel-jvm-posix-syscalls`,
    /// "VM por endereço": necessário porque a ABI de syscall (convenção
    /// `mmap`/`munmap`/`mprotect`: endereço + tamanho) não tem onde carregar
    /// de volta o `Region` opaco que `reserve` devolveu.
    pub fn region_containing(&self, addr: u64) -> Option<Region> {
        self.table.region_containing(addr)
    }

    /// Reserva `size` bytes (arredondado para cima em páginas) de memória
    /// virtual anônima, sem comprometer nenhuma memória física — cobre
    /// "Reserva de Faixa de Memória Virtual Anônima" da spec.
    pub fn reserve(&mut self, size: u64) -> Result<Region, VmmError> {
        if size == 0 {
            return Err(VmmError::Misaligned);
        }
        let rounded = size.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let base = self.ranges.allocate(rounded).ok_or(VmmError::OutOfVirtualSpace)?;
        Ok(self.table.reserve(base, rounded))
    }

    /// Compromete a sub-região `offset..offset+len` de `region`: aloca um
    /// frame físico por página, zera seu conteúdo via HHDM e o mapeia com
    /// `flags` — cobre "Comprometimento Sob Demanda de uma Sub-região
    /// Reservada" (incluindo "Memória comprometida é entregue zerada") da
    /// spec. `offset`/`len` devem ser múltiplos de `PAGE_SIZE`.
    pub fn commit<S: FrameStore>(
        &mut self,
        frame_allocator: &mut FrameAllocator<S>,
        region: &Region,
        offset: u64,
        len: u64,
        flags: PageFlags,
    ) -> Result<(), VmmError> {
        self.table.validate_commit(region, offset, len)?;

        let pages = len / PAGE_SIZE;
        for i in 0..pages {
            let virt = region.base + offset + i * PAGE_SIZE;
            let frame = frame_allocator.alloc().map_err(|_| VmmError::OutOfFrames)?;
            unsafe {
                let ptr = hhdm::phys_to_virt(hhdm::offset(), frame) as *mut u8;
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE as usize);
            }
            Mapper::new(frame_allocator)
                .map(virt, frame, flags, false)
                .expect("pagina recem-reservada nao deveria colidir com um mapeamento existente");
        }

        self.table.mark_committed(region, offset, len);
        Ok(())
    }

    /// Descompromete a sub-região `offset..offset+len` de `region`:
    /// desmapeia e libera os frames físicos de volta ao alocador, mantendo
    /// a reserva virtual válida para um `commit` futuro — cobre
    /// "Descomprometimento de uma Sub-região Comprometida" da spec.
    pub fn decommit<S: FrameStore>(
        &mut self,
        frame_allocator: &mut FrameAllocator<S>,
        region: &Region,
        offset: u64,
        len: u64,
    ) -> Result<(), VmmError> {
        self.table.validate_fully_committed(region, offset, len)?;

        let pages = len / PAGE_SIZE;
        for i in 0..pages {
            let virt = region.base + offset + i * PAGE_SIZE;
            let frame = Mapper::new(frame_allocator)
                .unmap(virt)
                .expect("pagina validada como comprometida deveria estar mapeada");
            frame_allocator.free(frame).expect("frame recem-desmapeado deveria pertencer a este alocador");
        }

        self.table.mark_decommitted(region, offset, len);
        Ok(())
    }

    /// Altera as permissões de acesso da sub-região `offset..offset+len` de
    /// `region`, que deve estar totalmente comprometida — cobre "Alteração
    /// de Proteção de uma Sub-região Comprometida" da spec.
    pub fn protect<S: FrameStore>(
        &mut self,
        frame_allocator: &mut FrameAllocator<S>,
        region: &Region,
        offset: u64,
        len: u64,
        flags: PageFlags,
    ) -> Result<(), VmmError> {
        self.table.validate_fully_committed(region, offset, len)?;

        let pages = len / PAGE_SIZE;
        let mut mapper = Mapper::new(frame_allocator);
        for i in 0..pages {
            let virt = region.base + offset + i * PAGE_SIZE;
            match mapper.protect(virt, flags) {
                Ok(()) => {}
                Err(MapError::NotMapped) => {
                    unreachable!("pagina validada como comprometida deveria estar mapeada")
                }
                Err(e) => unreachable!("Mapper::protect so deveria falhar com NotMapped, obteve {e:?}"),
            }
        }
        Ok(())
    }

    /// Libera `region` por completo: descompromete qualquer sub-região
    /// ainda comprometida e devolve toda a faixa virtual ao pool de faixas
    /// livres, disponível para reservas futuras — cobre "Liberação de uma
    /// Reserva" e "Reaproveitamento de Faixas Virtuais Liberadas" da spec.
    pub fn release<S: FrameStore>(
        &mut self,
        frame_allocator: &mut FrameAllocator<S>,
        region: Region,
    ) -> Result<(), VmmError> {
        let committed_offsets = self.table.remove(&region)?;

        for offset in committed_offsets {
            let virt = region.base + offset;
            let frame = Mapper::new(frame_allocator)
                .unmap(virt)
                .expect("offset devolvido por remove() deveria estar mapeado");
            frame_allocator.free(frame).expect("frame recem-desmapeado deveria pertencer a este alocador");
        }

        self.ranges.release(region.base, region.size);
        Ok(())
    }
}

/// Instância global do `Vmm`, inicializada em `kernel_main`/`continue_boot`
/// logo após o heap do kernel estar pronto — mesmo padrão de
/// `kernel::phys::FRAME_ALLOCATOR`.
pub static VMM: Spinlock<Vmm> = Spinlock::new(Vmm::new());

/// Suporte compartilhado para qualquer módulo cujos testes de host precisem
/// reservar/liberar através do `VMM` global — `Vmm::init()` nunca roda fora
/// de `continue_boot`, e o `VMM` é um único `Spinlock` compartilhado por todo
/// o processo de teste (módulos diferentes rodam em paralelo por padrão). Ver
/// design.md da change `kernel-jvm-posix-syscalls`, "Escopo de testes em
/// host" — consumido por `kernel::syscall`/`kernel::safepoint`.
#[cfg(test)]
pub(crate) mod test_support {
    use super::VMM;

    /// Serializa qualquer teste (de qualquer módulo) que toque o `VMM`
    /// global.
    pub(crate) static VMM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Registra a janela gerenciada (`Vmm::init`) exatamente uma vez por
    /// processo de teste — chamar mais de uma vez duplicaria a janela livre
    /// (`RangeAllocator::add_window` não é idempotente).
    pub(crate) fn ensure_vmm_initialized() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| VMM.lock().init());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------
    // RangeAllocator (tarefa 2.1)
    // -------------------------------------------------------------

    #[test]
    fn allocate_exact_size_consumes_the_whole_range() {
        let mut ranges = RangeAllocator::empty();
        ranges.add_window(0x1000, PAGE_SIZE);
        assert_eq!(ranges.allocate(PAGE_SIZE), Some(0x1000));
        assert_eq!(ranges.allocate(PAGE_SIZE), None);
    }

    #[test]
    fn allocate_with_leftover_splits_the_range() {
        let mut ranges = RangeAllocator::empty();
        ranges.add_window(0x1000, 10 * PAGE_SIZE);
        assert_eq!(ranges.allocate(4 * PAGE_SIZE), Some(0x1000));
        // Sobraram 6 páginas a partir de 0x1000 + 4*PAGE_SIZE.
        assert_eq!(ranges.allocate(6 * PAGE_SIZE), Some(0x1000 + 4 * PAGE_SIZE));
        assert_eq!(ranges.allocate(PAGE_SIZE), None);
    }

    #[test]
    fn allocate_larger_than_available_space_is_rejected() {
        let mut ranges = RangeAllocator::empty();
        ranges.add_window(0x1000, 2 * PAGE_SIZE);
        assert_eq!(ranges.allocate(3 * PAGE_SIZE), None);
    }

    #[test]
    fn release_merges_with_adjacent_free_neighbors() {
        let mut ranges = RangeAllocator::empty();
        ranges.add_window(0x1000, 10 * PAGE_SIZE);
        let a = ranges.allocate(3 * PAGE_SIZE).unwrap();
        let b = ranges.allocate(3 * PAGE_SIZE).unwrap();
        let c = ranges.allocate(4 * PAGE_SIZE).unwrap();
        // Libera o do meio primeiro (sem vizinhos livres ainda), depois os
        // dois lados: deve fundir tudo de volta numa faixa única.
        ranges.release(b, 3 * PAGE_SIZE);
        ranges.release(a, 3 * PAGE_SIZE);
        ranges.release(c, 4 * PAGE_SIZE);
        assert_eq!(ranges.allocate(10 * PAGE_SIZE), Some(0x1000));
    }

    #[test]
    fn reserve_after_release_reuses_the_freed_virtual_space() {
        let mut ranges = RangeAllocator::empty();
        ranges.add_window(0x1000, 10 * PAGE_SIZE);
        let first = ranges.allocate(6 * PAGE_SIZE).unwrap();
        assert_eq!(ranges.allocate(4 * PAGE_SIZE), Some(0x1000 + 6 * PAGE_SIZE));
        // Janela inteira consumida: sem reaproveitamento, a próxima reserva
        // de 6 páginas falharia.
        assert_eq!(ranges.allocate(6 * PAGE_SIZE), None);

        ranges.release(first, 6 * PAGE_SIZE);
        assert_eq!(ranges.allocate(6 * PAGE_SIZE), Some(first));
    }

    // -------------------------------------------------------------
    // ReservationTable (tarefa 2.2)
    // -------------------------------------------------------------

    #[test]
    fn committing_within_bounds_marks_exactly_the_right_pages() {
        let mut table = ReservationTable::empty();
        let region = table.reserve(0x1000, 4 * PAGE_SIZE);

        table.validate_commit(&region, PAGE_SIZE, 2 * PAGE_SIZE).unwrap();
        table.mark_committed(&region, PAGE_SIZE, 2 * PAGE_SIZE);

        let state = table.lookup(&region).unwrap();
        assert_eq!(state.committed, alloc::vec![false, true, true, false]);
    }

    #[test]
    fn committing_out_of_bounds_is_rejected_without_changing_state() {
        let mut table = ReservationTable::empty();
        let region = table.reserve(0x1000, 2 * PAGE_SIZE);

        assert_eq!(table.validate_commit(&region, PAGE_SIZE, 2 * PAGE_SIZE), Err(VmmError::OutOfBounds));
        assert_eq!(table.lookup(&region).unwrap().committed, alloc::vec![false, false]);
    }

    #[test]
    fn committing_an_already_committed_sub_region_is_rejected() {
        let mut table = ReservationTable::empty();
        let region = table.reserve(0x1000, 4 * PAGE_SIZE);
        table.validate_commit(&region, 0, 2 * PAGE_SIZE).unwrap();
        table.mark_committed(&region, 0, 2 * PAGE_SIZE);

        // Sobreposição parcial com o que já está comprometido.
        assert_eq!(table.validate_commit(&region, PAGE_SIZE, 2 * PAGE_SIZE), Err(VmmError::AlreadyCommitted));
        // Estado inalterado pela tentativa rejeitada.
        assert_eq!(table.lookup(&region).unwrap().committed, alloc::vec![true, true, false, false]);
    }

    #[test]
    fn decommitting_clears_the_committed_pages() {
        let mut table = ReservationTable::empty();
        let region = table.reserve(0x1000, 2 * PAGE_SIZE);
        table.validate_commit(&region, 0, 2 * PAGE_SIZE).unwrap();
        table.mark_committed(&region, 0, 2 * PAGE_SIZE);

        table.validate_fully_committed(&region, 0, 2 * PAGE_SIZE).unwrap();
        table.mark_decommitted(&region, 0, 2 * PAGE_SIZE);

        assert_eq!(table.lookup(&region).unwrap().committed, alloc::vec![false, false]);
    }

    #[test]
    fn protect_requires_the_whole_sub_region_to_already_be_committed() {
        let mut table = ReservationTable::empty();
        let region = table.reserve(0x1000, 2 * PAGE_SIZE);

        assert_eq!(table.validate_fully_committed(&region, 0, 2 * PAGE_SIZE), Err(VmmError::NotCommitted));

        table.validate_commit(&region, 0, PAGE_SIZE).unwrap();
        table.mark_committed(&region, 0, PAGE_SIZE);
        // Só a primeira página está comprometida; a segunda ainda não.
        assert_eq!(table.validate_fully_committed(&region, 0, 2 * PAGE_SIZE), Err(VmmError::NotCommitted));

        table.validate_commit(&region, PAGE_SIZE, PAGE_SIZE).unwrap();
        table.mark_committed(&region, PAGE_SIZE, PAGE_SIZE);
        assert!(table.validate_fully_committed(&region, 0, 2 * PAGE_SIZE).is_ok());
    }

    #[test]
    fn region_containing_finds_the_reservation_that_owns_the_address() {
        let mut table = ReservationTable::empty();
        let region = table.reserve(0x1000, 4 * PAGE_SIZE);

        assert_eq!(table.region_containing(0x1000), Some(region));
        assert_eq!(table.region_containing(0x1000 + PAGE_SIZE), Some(region));
        assert_eq!(table.region_containing(0x1000 + 4 * PAGE_SIZE - 1), Some(region));
    }

    #[test]
    fn region_containing_rejects_an_address_outside_any_reservation() {
        let mut table = ReservationTable::empty();
        table.reserve(0x1000, PAGE_SIZE);

        assert_eq!(table.region_containing(0x0999), None);
        assert_eq!(table.region_containing(0x1000 + PAGE_SIZE), None);
    }

    #[test]
    fn region_containing_rejects_an_address_inside_a_released_reservation() {
        let mut table = ReservationTable::empty();
        let region = table.reserve(0x1000, PAGE_SIZE);
        table.remove(&region).unwrap();

        assert_eq!(table.region_containing(0x1000), None);
    }

    #[test]
    fn a_stale_handle_is_rejected_even_after_its_address_is_reused() {
        let mut table = ReservationTable::empty();
        let old = table.reserve(0x1000, PAGE_SIZE);
        table.remove(&old).unwrap();

        // Uma reserva nova reaproveita exatamente o mesmo endereço base
        // (simulando o `RangeAllocator` devolvendo o espaço liberado).
        let new = table.reserve(0x1000, PAGE_SIZE);
        assert_ne!(old.generation, new.generation);

        assert_eq!(table.validate_commit(&old, 0, PAGE_SIZE), Err(VmmError::StaleHandle));
        assert_eq!(table.remove(&old), Err(VmmError::StaleHandle));
        // O handle novo, em contraste, continua funcionando normalmente.
        assert!(table.validate_commit(&new, 0, PAGE_SIZE).is_ok());
    }
}
