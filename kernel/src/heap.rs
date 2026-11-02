use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicU64, Ordering};

use linked_list_allocator::LockedHeap;

use crate::paging::{Mapper, PageFlags, PAGE_SIZE};
use crate::phys::{FrameAllocator, FrameStore};

/// Endereço virtual base reservado para o heap do kernel: distante o
/// suficiente da HHDM (PML4 índice 256) e do código do kernel (PML4 índice
/// 511) para não colidir com nenhum dos dois. Ver `design.md` da change
/// `kernel-memory-management`.
pub const HEAP_START: u64 = 0xffff_9000_0000_0000;

/// Tamanho máximo que a faixa reservada do heap pode crescer.
pub const HEAP_MAX_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB

/// Quanto é mapeado e entregue ao allocator na inicialização.
const HEAP_INITIAL_SIZE: u64 = 64 * 1024; // 64 KiB

static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Mínimo de bytes mapeados por chamada de crescimento, para amortizar o
/// custo de mapear página a página a cada falta de espaço.
const MIN_GROWTH: u64 = 64 * 1024;

/// Envolve o `LockedHeap` para crescer a faixa mapeada sob demanda quando uma
/// alocação falha por falta de espaço (em vez de falhar imediatamente) —
/// cobre "Crescimento sob Demanda" da spec `kernel-heap`. A falha definitiva
/// (sem mais memória física, ou faixa reservada esgotada) é tratada pelo
/// `#[alloc_error_handler]`.
struct GrowableHeap;

unsafe impl GlobalAlloc for GrowableHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        if !ptr.is_null() {
            return ptr;
        }

        let grow_amount = (layout.size() as u64).max(MIN_GROWTH).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
        if grow(&mut frame_allocator, grow_amount).is_err() {
            return core::ptr::null_mut();
        }
        drop(frame_allocator);

        unsafe { ALLOCATOR.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { ALLOCATOR.dealloc(ptr, layout) }
    }
}

// Sob `cfg(test)` o host já linka `std` com seu próprio alocador global;
// registrar um segundo aqui seria um erro de compilação.
#[cfg_attr(not(test), global_allocator)]
static GLOBAL: GrowableHeap = GrowableHeap;

/// Quantos bytes da faixa reservada a partir de `HEAP_START` já estão
/// mapeados (com frames físicos reais por trás).
static MAPPED_SIZE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HeapError {
    /// Não foi possível mapear memória suficiente (sem frames físicos livres).
    MapFailed,
    /// A faixa virtual reservada para o heap (`HEAP_MAX_SIZE`) foi esgotada.
    MaxSizeExceeded,
}

/// Mapeia a região inicial do heap a partir de `HEAP_START` e inicializa o
/// allocator global sobre ela. Deve ser chamada uma única vez, depois de o
/// alocador de frames físicos já estar pronto.
pub fn init<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<(), HeapError> {
    map_additional(frame_allocator, HEAP_INITIAL_SIZE)?;
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_INITIAL_SIZE as usize);
    }
    Ok(())
}

/// Mapeia `additional` bytes a mais (arredondado para cima em páginas) logo
/// após a região já mapeada do heap, e estende o allocator para cobri-los —
/// cobre "Crescimento sob Demanda" da spec `kernel-heap`.
pub fn grow<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>, additional: u64) -> Result<(), HeapError> {
    map_additional(frame_allocator, additional)?;
    unsafe {
        ALLOCATOR.lock().extend(additional as usize);
    }
    Ok(())
}

/// Calcula o novo total mapeado após crescer `additional` bytes a partir de
/// `current`, rejeitando overflow e ultrapassagem de `HEAP_MAX_SIZE`. Função
/// pura, testável sem hardware.
fn checked_new_total(current: u64, additional: u64) -> Result<u64, HeapError> {
    let new_total = current.checked_add(additional).ok_or(HeapError::MaxSizeExceeded)?;
    if new_total > HEAP_MAX_SIZE {
        return Err(HeapError::MaxSizeExceeded);
    }
    Ok(new_total)
}

fn map_additional<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>, additional: u64) -> Result<(), HeapError> {
    let current = MAPPED_SIZE.load(Ordering::Acquire);
    let new_total = checked_new_total(current, additional)?;

    let pages = additional.div_ceil(PAGE_SIZE);
    let mut mapper = Mapper::new(frame_allocator);
    for i in 0..pages {
        let virt = HEAP_START + current + i * PAGE_SIZE;
        mapper
            .map_new(virt, PageFlags::KERNEL_RW, false)
            .map_err(|_| HeapError::MapFailed)?;
    }

    MAPPED_SIZE.store(new_total, Ordering::Release);
    Ok(())
}

/// Tamanho, em bytes, atualmente mapeado para o heap (para fins de relato no
/// banner de boot).
pub fn mapped_size() -> u64 {
    MAPPED_SIZE.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_within_max_size_is_allowed() {
        assert_eq!(checked_new_total(0, HEAP_MAX_SIZE), Ok(HEAP_MAX_SIZE));
    }

    #[test]
    fn growth_past_max_size_is_rejected() {
        assert_eq!(checked_new_total(HEAP_MAX_SIZE, 1), Err(HeapError::MaxSizeExceeded));
    }

    #[test]
    fn growth_overflowing_u64_is_rejected_not_wrapped() {
        assert_eq!(checked_new_total(u64::MAX, 1), Err(HeapError::MaxSizeExceeded));
    }
}
