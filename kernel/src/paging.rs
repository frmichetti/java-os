//! Gerenciamento de paginação x86_64 (4 níveis), escrito à mão (sem a crate
//! `x86_64`) para manter consistência com o resto do kernel — ver
//! `design.md` da change `kernel-memory-management`.

use crate::hhdm;
use crate::phys::{FrameAllocator, FrameStore};

pub const PAGE_SIZE: u64 = 4096;

pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;
pub const PTE_ACCESSED: u64 = 1 << 5;
pub const PTE_DIRTY: u64 = 1 << 6;
/// Só tem significado em entradas de nível intermediário (PDPTE/PDE); numa
/// PTE de nível 1 esse bit é o PAT.
pub const PTE_HUGE_PAGE: u64 = 1 << 7;
pub const PTE_NO_EXECUTE: u64 = 1 << 63;

/// Bits 12..=51: endereço físico da tabela/frame apontado por esta entrada.
const PHYS_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

/// Uma entrada de tabela de página (PML4E/PDPTE/PDE/PTE) — todas têm o mesmo
/// layout de 64 bits no x86_64. `repr(transparent)` garante o mesmo layout
/// de memória de um `u64` puro, exigido para viver dentro de uma tabela real.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn new(phys_addr: u64, flags: u64) -> Self {
        Self((phys_addr & PHYS_ADDR_MASK) | (flags & !PHYS_ADDR_MASK))
    }

    pub fn is_present(&self) -> bool {
        self.0 & PTE_PRESENT != 0
    }

    /// Endereço físico apontado por esta entrada (tabela de próximo nível,
    /// ou frame final), sem os bits de flag.
    pub fn addr(&self) -> u64 {
        self.0 & PHYS_ADDR_MASK
    }

    /// Bits de flag desta entrada, sem o endereço físico.
    pub fn flags(&self) -> u64 {
        self.0 & !PHYS_ADDR_MASK
    }

    pub fn raw(&self) -> u64 {
        self.0
    }
}

/// Permissões de acesso para uma página mapeada, na granularidade que o
/// kernel expõe (ver spec `kernel-paging` — "Mapeamento de Página Virtual").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageFlags {
    pub writable: bool,
    pub user_accessible: bool,
    pub executable: bool,
}

impl PageFlags {
    pub const KERNEL_RW: Self = Self { writable: true, user_accessible: false, executable: false };
    pub const KERNEL_CODE: Self = Self { writable: false, user_accessible: false, executable: true };

    /// Converte para os bits de flag de uma `PageTableEntry` (sempre inclui
    /// `PTE_PRESENT`, já que uma entrada sem essa flag não representa uma
    /// página mapeada).
    pub fn to_pte_bits(self) -> u64 {
        let mut bits = PTE_PRESENT;
        if self.writable {
            bits |= PTE_WRITABLE;
        }
        if self.user_accessible {
            bits |= PTE_USER;
        }
        if !self.executable {
            bits |= PTE_NO_EXECUTE;
        }
        bits
    }
}

/// Uma tabela de página de 512 entradas, com o alinhamento de 4 KiB exigido
/// pelo hardware.
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MapError {
    /// `virt` ou `phys` não é múltiplo de `PAGE_SIZE`.
    Misaligned,
    /// O alocador de frames físicos não tem mais memória para criar uma
    /// tabela de nível intermediário.
    OutOfFrames,
    /// `virt` já está mapeado e `allow_replace` não foi pedido explicitamente.
    AlreadyMapped,
    /// `virt` não está mapeado (nada para desmapear).
    NotMapped,
}

struct VirtIndices {
    pml4: usize,
    pdpt: usize,
    pd: usize,
    pt: usize,
}

/// Decompõe um endereço virtual canônico nos 4 índices de 9 bits usados para
/// caminhar PML4 → PDPT → PD → PT. Função pura, testável sem hardware.
fn decompose(virt: u64) -> VirtIndices {
    VirtIndices {
        pml4: ((virt >> 39) & 0x1ff) as usize,
        pdpt: ((virt >> 30) & 0x1ff) as usize,
        pd: ((virt >> 21) & 0x1ff) as usize,
        pt: ((virt >> 12) & 0x1ff) as usize,
    }
}

fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value & PHYS_ADDR_MASK
}

fn flush_tlb(virt: u64) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
    }
}

fn phys_to_table_ptr(phys: u64) -> *mut PageTable {
    hhdm::phys_to_virt(hhdm::offset(), phys) as *mut PageTable
}

/// A raiz (PML4) sobre a qual um `Mapper` opera: ou a atualmente ativa
/// (lida de `CR3` sob demanda, nunca na construção — ler `CR3` é uma
/// instrução privilegiada, e `Mapper::new` precisa continuar seguro de
/// construir em host, onde só roda em anel 3, para os testes unitários de
/// `translate`/`decompose` existentes), ou uma PML4 explícita.
enum Root {
    Active,
    Explicit(u64),
}

/// Mapeia/desmapeia páginas virtuais de 4 KiB sobre uma paginação de 4
/// níveis explícita (`root`), criando tabelas intermediárias sob demanda via
/// o alocador de frames físicos. Ver spec `kernel-paging`.
pub struct Mapper<'a, S: FrameStore> {
    frame_allocator: &'a mut FrameAllocator<S>,
    root: Root,
}

impl<'a, S: FrameStore> Mapper<'a, S> {
    /// Opera sobre a paginação atualmente ativa (`CR3`) — mesmo
    /// comportamento de sempre, usado por `kernel::heap`/`kernel::stack`.
    pub fn new(frame_allocator: &'a mut FrameAllocator<S>) -> Self {
        Self { frame_allocator, root: Root::Active }
    }

    /// Opera sobre uma PML4 explícita (`root`), não necessariamente a
    /// atualmente ativa — usada para montar o espaço de endereçamento de um
    /// processo antes de trocar para ele (ver `AddressSpace`).
    pub fn for_root(frame_allocator: &'a mut FrameAllocator<S>, root: u64) -> Self {
        Self { frame_allocator, root: Root::Explicit(root) }
    }

    fn root_phys(&self) -> u64 {
        match self.root {
            Root::Active => read_cr3(),
            Root::Explicit(root) => root,
        }
    }

    fn table_ptr(&self, phys: u64) -> *mut PageTable {
        phys_to_table_ptr(phys)
    }

    /// # Safety
    /// `phys` deve ser o endereço físico de uma `PageTable` válida e
    /// acessível via HHDM.
    unsafe fn table_mut<'t>(&self, phys: u64) -> &'t mut PageTable {
        unsafe { &mut *self.table_ptr(phys) }
    }

    /// Retorna o endereço físico da tabela do próximo nível referenciada por
    /// `table.entries[index]`, criando-a (zerada) sob demanda se ainda não
    /// existir. Entradas intermediárias são sempre permissivas
    /// (presente+escrita+usuário); a restrição real de acesso é aplicada
    /// apenas na entrada folha, conforme a convenção padrão do x86_64 (a
    /// permissão efetiva é o AND de todos os níveis).
    fn next_table_or_create(&mut self, table: &mut PageTable, index: usize) -> Result<u64, MapError> {
        let entry = table.entries[index];
        if entry.is_present() {
            return Ok(entry.addr());
        }
        let frame = self.frame_allocator.alloc().map_err(|_| MapError::OutOfFrames)?;
        let ptr = self.table_ptr(frame);
        unsafe { core::ptr::write_bytes(ptr as *mut u8, 0, PAGE_SIZE as usize) };
        table.entries[index] = PageTableEntry::new(frame, PTE_PRESENT | PTE_WRITABLE | PTE_USER);
        Ok(frame)
    }

    /// Mapeia a página virtual de 4 KiB em `virt` para o frame físico `phys`,
    /// com as permissões `flags`. Rejeita um `virt` já mapeado a menos que
    /// `allow_replace` seja `true` — cobre "Mapear um endereço já mapeado
    /// sem solicitar substituição é rejeitado" da spec.
    pub fn map(&mut self, virt: u64, phys: u64, flags: PageFlags, allow_replace: bool) -> Result<(), MapError> {
        if virt % PAGE_SIZE != 0 || phys % PAGE_SIZE != 0 {
            return Err(MapError::Misaligned);
        }
        let idx = decompose(virt);
        let pml4_phys = self.root_phys();
        let pdpt_phys = self.next_table_or_create(unsafe { self.table_mut(pml4_phys) }, idx.pml4)?;
        let pd_phys = self.next_table_or_create(unsafe { self.table_mut(pdpt_phys) }, idx.pdpt)?;
        let pt_phys = self.next_table_or_create(unsafe { self.table_mut(pd_phys) }, idx.pd)?;
        let pt = unsafe { self.table_mut(pt_phys) };

        if pt.entries[idx.pt].is_present() && !allow_replace {
            return Err(MapError::AlreadyMapped);
        }

        pt.entries[idx.pt] = PageTableEntry::new(phys, flags.to_pte_bits());
        flush_tlb(virt);
        Ok(())
    }

    /// Aloca um novo frame físico e mapeia `virt` para ele, em um único
    /// passo. Conveniência usada por quem só precisa de uma página nova
    /// (ex.: o heap do kernel), sem já ter um frame físico em mãos.
    pub fn map_new(&mut self, virt: u64, flags: PageFlags, allow_replace: bool) -> Result<u64, MapError> {
        let frame = self.frame_allocator.alloc().map_err(|_| MapError::OutOfFrames)?;
        self.map(virt, frame, flags, allow_replace)?;
        Ok(frame)
    }

    /// Consulta somente-leitura: retorna o endereço físico mapeado para a
    /// página virtual em `virt`, ou `None` se qualquer nível da caminhada
    /// (PML4/PDPT/PD/PT) não estiver presente, ou se `virt` não for alinhado
    /// a `PAGE_SIZE`. Ao contrário de [`unmap`], nunca cria nem remove
    /// nenhuma tabela — usada para verificar que uma página (ex.: a guard
    /// page de `kernel::stack`) permanece deliberadamente não mapeada.
    pub fn translate(&self, virt: u64) -> Option<u64> {
        if virt % PAGE_SIZE != 0 {
            return None;
        }
        let idx = decompose(virt);

        let pml4 = unsafe { self.table_mut(self.root_phys()) };
        let pml4e = pml4.entries[idx.pml4];
        if !pml4e.is_present() {
            return None;
        }
        let pdpt = unsafe { self.table_mut(pml4e.addr()) };
        let pdpte = pdpt.entries[idx.pdpt];
        if !pdpte.is_present() {
            return None;
        }
        let pd = unsafe { self.table_mut(pdpte.addr()) };
        let pde = pd.entries[idx.pd];
        if !pde.is_present() {
            return None;
        }
        let pt = unsafe { self.table_mut(pde.addr()) };
        let pte = pt.entries[idx.pt];
        if !pte.is_present() {
            return None;
        }
        Some(pte.addr())
    }

    /// Desmapeia a página virtual em `virt`, retornando o frame físico que
    /// estava mapeado. `Err(MapError::NotMapped)` se não houver mapeamento.
    pub fn unmap(&mut self, virt: u64) -> Result<u64, MapError> {
        if virt % PAGE_SIZE != 0 {
            return Err(MapError::Misaligned);
        }
        let idx = decompose(virt);

        let pml4 = unsafe { self.table_mut(self.root_phys()) };
        if !pml4.entries[idx.pml4].is_present() {
            return Err(MapError::NotMapped);
        }
        let pdpt = unsafe { self.table_mut(pml4.entries[idx.pml4].addr()) };
        if !pdpt.entries[idx.pdpt].is_present() {
            return Err(MapError::NotMapped);
        }
        let pd = unsafe { self.table_mut(pdpt.entries[idx.pdpt].addr()) };
        if !pd.entries[idx.pd].is_present() {
            return Err(MapError::NotMapped);
        }
        let pt = unsafe { self.table_mut(pd.entries[idx.pd].addr()) };
        let entry = pt.entries[idx.pt];
        if !entry.is_present() {
            return Err(MapError::NotMapped);
        }

        pt.entries[idx.pt] = PageTableEntry::empty();
        flush_tlb(virt);
        Ok(entry.addr())
    }
}

/// Primeiro índice de PML4 compartilhado entre todas as address spaces: a
/// partir daqui (HHDM, heap, pool de stacks de thread, código do kernel —
/// tudo "do kernel", nunca específico de um processo), as entradas são
/// copiadas por referência de uma `AddressSpace` para outra na criação, de
/// forma que o kernel continue acessível depois de uma troca de `CR3`. Ver
/// `design.md` da change `kernel-process-scheduler`.
const KERNEL_PML4_START: usize = 256;

/// Um espaço de endereçamento virtual independente: sua própria PML4, com a
/// metade superior (ver `KERNEL_PML4_START`) compartilhada com todas as
/// outras address spaces e a metade inferior inicialmente vazia, disponível
/// para mapeamentos específicos de um processo. Ver spec `kernel-paging`
/// ("Criação de Novo Espaço de Endereçamento").
pub struct AddressSpace {
    pml4_phys: u64,
}

impl AddressSpace {
    /// Cria uma nova address space: aloca e zera uma PML4, depois copia
    /// nela as entradas `KERNEL_PML4_START..512` da PML4 atualmente ativa,
    /// preservando o acesso ao kernel assim que a CPU trocar para ela.
    pub fn new<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<Self, MapError> {
        let frame = frame_allocator.alloc().map_err(|_| MapError::OutOfFrames)?;
        let new_ptr = phys_to_table_ptr(frame);
        unsafe { core::ptr::write_bytes(new_ptr as *mut u8, 0, PAGE_SIZE as usize) };

        let active_ptr = phys_to_table_ptr(read_cr3());
        let new_table = unsafe { &mut *new_ptr };
        let active_table = unsafe { &*active_ptr };
        new_table.entries[KERNEL_PML4_START..].copy_from_slice(&active_table.entries[KERNEL_PML4_START..]);

        Ok(Self { pml4_phys: frame })
    }

    /// Endereço físico da PML4 desta address space — usado por
    /// `Mapper::for_root` para montar mapeamentos nela antes de trocar para
    /// ela, e por `switch` para carregar `CR3`.
    pub fn root(&self) -> u64 {
        self.pml4_phys
    }

    /// Troca o espaço de endereçamento ativo da CPU para este.
    ///
    /// # Safety
    /// O chamador deve garantir que nenhum código em execução depende de um
    /// mapeamento da metade inferior que só existia no espaço de
    /// endereçamento anterior — a metade superior (kernel) é sempre
    /// preservada por construção (ver `new`), mas a inferior é própria de
    /// cada address space.
    pub unsafe fn switch(&self) {
        unsafe {
            core::arch::asm!("mov cr3, {0}", in(reg) self.pml4_phys, options(nostack, preserves_flags));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_table_entry_round_trips_address_and_flags() {
        let phys = 0x0000_000a_bcde_f000;
        let flags = PTE_PRESENT | PTE_WRITABLE;
        let pte = PageTableEntry::new(phys, flags);
        assert_eq!(pte.addr(), phys);
        assert_eq!(pte.flags(), flags);
        assert!(pte.is_present());
    }

    #[test]
    fn empty_entry_is_not_present() {
        assert!(!PageTableEntry::empty().is_present());
    }

    #[test]
    fn page_flags_kernel_rw_is_writable_not_executable_not_user() {
        let bits = PageFlags::KERNEL_RW.to_pte_bits();
        assert_eq!(bits & PTE_PRESENT, PTE_PRESENT);
        assert_eq!(bits & PTE_WRITABLE, PTE_WRITABLE);
        assert_eq!(bits & PTE_USER, 0);
        assert_eq!(bits & PTE_NO_EXECUTE, PTE_NO_EXECUTE);
    }

    #[test]
    fn page_flags_kernel_code_is_executable_not_writable() {
        let bits = PageFlags::KERNEL_CODE.to_pte_bits();
        assert_eq!(bits & PTE_WRITABLE, 0);
        assert_eq!(bits & PTE_NO_EXECUTE, 0);
    }

    #[test]
    fn page_flags_user_accessible_sets_user_bit() {
        let flags = PageFlags { writable: true, user_accessible: true, executable: false };
        assert_eq!(flags.to_pte_bits() & PTE_USER, PTE_USER);
    }

    #[test]
    fn translate_rejects_misaligned_address_without_touching_cr3() {
        // O guard de desalinhamento retorna antes de qualquer acesso a
        // CR3/HHDM, então é seguro chamar `translate` em host mesmo sem uma
        // página real por trás do `HhdmFrameStore` (nunca usado aqui). A
        // caminhada real de tabelas (endereço mapeado/não mapeado) só é
        // verificável em boot real — ver tasks.md.
        use crate::phys::{FrameAllocator, HhdmFrameStore};
        let mut allocator = FrameAllocator::new(HhdmFrameStore);
        let mapper = Mapper::new(&mut allocator);
        assert_eq!(mapper.translate(0x1001), None);
    }

    #[test]
    fn decompose_extracts_the_four_nine_bit_indices() {
        // Índices escolhidos deliberadamente distintos para detectar troca de bits.
        let virt = (1u64 << 39) | (2u64 << 30) | (3u64 << 21) | (4u64 << 12);
        let idx = decompose(virt);
        assert_eq!(idx.pml4, 1);
        assert_eq!(idx.pdpt, 2);
        assert_eq!(idx.pd, 3);
        assert_eq!(idx.pt, 4);
    }
}
