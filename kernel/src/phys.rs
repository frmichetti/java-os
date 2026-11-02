use limine::memmap::{self, Entry};

use crate::hhdm;
use crate::sync::Spinlock;

pub const FRAME_SIZE: u64 = 4096;

/// Sentinela de lista vazia / "sem próximo" na lista livre intrusiva.
const NONE: u64 = u64::MAX;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FrameError {
    /// Não há frames livres no pool.
    OutOfMemory,
    /// O endereço não corresponde a nenhum frame dentro das regiões usáveis
    /// geridas por este alocador.
    NotManaged,
    /// O endereço já está livre (double-free).
    DoubleFree,
    /// O bootloader não forneceu um memory map.
    NoMemoryMap,
}

fn align_up(addr: u64) -> u64 {
    (addr + FRAME_SIZE - 1) & !(FRAME_SIZE - 1)
}

fn align_down(addr: u64) -> u64 {
    addr & !(FRAME_SIZE - 1)
}

/// Itera os endereços físicos de cada frame de 4 KiB inteiramente contido em
/// alguma região `usable` do memory map. Função pura, testável sem hardware.
pub fn usable_frame_addresses<'a>(entries: &'a [&'a Entry]) -> impl Iterator<Item = u64> + 'a {
    entries
        .iter()
        .filter(|e| e.type_ == memmap::MEMMAP_USABLE)
        .flat_map(|e| {
            let start = align_up(e.base);
            let end = align_down(e.base + e.length);
            (start..end).step_by(FRAME_SIZE as usize)
        })
}

/// Acesso ao "próximo ponteiro" de 8 bytes guardado dentro de um frame livre.
/// Abstrai o acesso real a memória física (via HHDM) para permitir testar a
/// lógica do alocador em um host sem hardware (ver `tests` abaixo).
///
/// # Safety
/// Implementações devem garantir que `phys` é um endereço físico
/// frame-aligned pertencente a uma região de memória válida e acessível.
pub unsafe trait FrameStore {
    unsafe fn read_next(&self, phys: u64) -> u64;
    unsafe fn write_next(&mut self, phys: u64, next: u64);
}

/// Implementação real: lê/escreve através da higher-half direct map.
pub struct HhdmFrameStore;

unsafe impl FrameStore for HhdmFrameStore {
    unsafe fn read_next(&self, phys: u64) -> u64 {
        let virt = hhdm::phys_to_virt(hhdm::offset(), phys) as *const u64;
        unsafe { virt.read_volatile() }
    }

    unsafe fn write_next(&mut self, phys: u64, next: u64) {
        let virt = hhdm::phys_to_virt(hhdm::offset(), phys) as *mut u64;
        unsafe { virt.write_volatile(next) };
    }
}

/// Alocador de frames físicos de 4 KiB: uma lista livre intrusiva (cada frame
/// livre guarda, nos seus próprios bytes, o endereço do próximo frame livre),
/// inicializada a partir das regiões `usable` do memory map do bootloader.
/// Ver `design.md` da change `kernel-memory-management` para o racional.
pub struct FrameAllocator<S: FrameStore> {
    store: S,
    free_head: u64,
    total_frames: u64,
    free_frames: u64,
    regions: &'static [&'static Entry],
}

impl<S: FrameStore> FrameAllocator<S> {
    pub const fn new(store: S) -> Self {
        Self {
            store,
            free_head: NONE,
            total_frames: 0,
            free_frames: 0,
            regions: &[],
        }
    }

    /// Inicializa o pool a partir da resposta de `MemmapRequest`. Retorna
    /// `Err(FrameError::NoMemoryMap)` em vez de assumir um layout arbitrário
    /// quando o bootloader não respondeu — cobre o cenário "Memory map
    /// indisponível impede a inicialização" da spec `kernel-physical-memory`.
    pub fn init(&mut self, entries: Option<&'static [&'static Entry]>) -> Result<(), FrameError> {
        let entries = entries.ok_or(FrameError::NoMemoryMap)?;
        self.regions = entries;
        for phys in usable_frame_addresses(entries) {
            unsafe { self.push_free(phys) };
        }
        Ok(())
    }

    unsafe fn push_free(&mut self, phys: u64) {
        unsafe { self.store.write_next(phys, self.free_head) };
        self.free_head = phys;
        self.total_frames += 1;
        self.free_frames += 1;
    }

    /// Aloca um frame livre. `Err(FrameError::OutOfMemory)` se não houver
    /// nenhum — nunca retorna um endereço inválido ou fabricado.
    pub fn alloc(&mut self) -> Result<u64, FrameError> {
        if self.free_head == NONE {
            return Err(FrameError::OutOfMemory);
        }
        let phys = self.free_head;
        let next = unsafe { self.store.read_next(phys) };
        self.free_head = next;
        self.free_frames -= 1;
        Ok(phys)
    }

    /// Devolve um frame ao pool. Rejeita endereços fora do intervalo
    /// gerenciado e frames já livres (double-free) em vez de corromper o
    /// estado interno silenciosamente.
    pub fn free(&mut self, phys: u64) -> Result<(), FrameError> {
        if !self.is_managed(phys) {
            return Err(FrameError::NotManaged);
        }
        if self.is_in_free_list(phys) {
            return Err(FrameError::DoubleFree);
        }
        unsafe { self.push_free(phys) };
        Ok(())
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    pub fn free_frames(&self) -> u64 {
        self.free_frames
    }

    fn is_managed(&self, phys: u64) -> bool {
        if phys % FRAME_SIZE != 0 {
            return false;
        }
        self.regions.iter().any(|e| {
            e.type_ == memmap::MEMMAP_USABLE
                && phys >= align_up(e.base)
                && phys < align_down(e.base + e.length)
        })
    }

    fn is_in_free_list(&self, phys: u64) -> bool {
        let mut cur = self.free_head;
        let mut steps = 0u64;
        while cur != NONE && steps <= self.total_frames {
            if cur == phys {
                return true;
            }
            cur = unsafe { self.store.read_next(cur) };
            steps += 1;
        }
        false
    }
}

/// Instância global do alocador de frames, inicializada em `kernel_main`
/// logo após o relato do mapa de memória.
pub static FRAME_ALLOCATOR: Spinlock<FrameAllocator<HhdmFrameStore>> =
    Spinlock::new(FrameAllocator::new(HhdmFrameStore));

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct SimulatedFrameStore {
        links: HashMap<u64, u64>,
    }

    impl SimulatedFrameStore {
        fn new() -> Self {
            Self { links: HashMap::new() }
        }
    }

    unsafe impl FrameStore for SimulatedFrameStore {
        unsafe fn read_next(&self, phys: u64) -> u64 {
            *self.links.get(&phys).unwrap_or(&NONE)
        }

        unsafe fn write_next(&mut self, phys: u64, next: u64) {
            self.links.insert(phys, next);
        }
    }

    fn entry(base: u64, length: u64, type_: u64) -> Entry {
        Entry { base, length, type_ }
    }

    #[test]
    fn counts_frames_only_from_usable_regions() {
        let data = [
            entry(0x0000, 0x2000, memmap::MEMMAP_USABLE),   // 2 frames
            entry(0x2000, 0x1000, memmap::MEMMAP_RESERVED), // ignorado
            entry(0x3000, 0x3000, memmap::MEMMAP_USABLE),   // 3 frames
        ];
        let refs: Vec<&Entry> = data.iter().collect();
        assert_eq!(usable_frame_addresses(&refs).count(), 5);
    }

    #[test]
    fn init_fails_explicitly_when_memmap_is_absent() {
        let mut alloc = FrameAllocator::new(SimulatedFrameStore::new());
        assert_eq!(alloc.init(None), Err(FrameError::NoMemoryMap));
        assert_eq!(alloc.total_frames(), 0);
    }

    fn init_two_frame_allocator() -> FrameAllocator<SimulatedFrameStore> {
        let data: &'static Entry = Box::leak(Box::new(entry(0x1000, 0x2000, memmap::MEMMAP_USABLE)));
        let leaked_refs: &'static [&'static Entry] = Box::leak(Box::new([data]));
        let mut alloc = FrameAllocator::new(SimulatedFrameStore::new());
        alloc.init(Some(leaked_refs)).unwrap();
        alloc
    }

    #[test]
    fn alloc_decrements_free_count_and_free_allows_reallocation() {
        let mut alloc = init_two_frame_allocator();
        assert_eq!(alloc.total_frames(), 2);
        assert_eq!(alloc.free_frames(), 2);

        let frame = alloc.alloc().expect("deveria haver frame livre");
        assert_eq!(alloc.free_frames(), 1);

        alloc.free(frame).expect("liberar um frame alocado deve funcionar");
        assert_eq!(alloc.free_frames(), 2);

        let frame_again = alloc.alloc().expect("frame liberado deve poder ser realocado");
        assert_eq!(frame_again, frame);
    }

    #[test]
    fn alloc_fails_explicitly_when_pool_is_empty() {
        let mut alloc = init_two_frame_allocator();
        alloc.alloc().unwrap();
        alloc.alloc().unwrap();
        assert_eq!(alloc.alloc(), Err(FrameError::OutOfMemory));
    }

    #[test]
    fn free_rejects_address_outside_managed_range() {
        let mut alloc = init_two_frame_allocator();
        assert_eq!(alloc.free(0xdead_b000), Err(FrameError::NotManaged));
    }

    #[test]
    fn free_rejects_double_free() {
        let mut alloc = init_two_frame_allocator();
        let frame = alloc.alloc().unwrap();
        alloc.free(frame).unwrap();
        assert_eq!(alloc.free(frame), Err(FrameError::DoubleFree));
    }
}
