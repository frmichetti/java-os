use core::sync::atomic::{AtomicU64, Ordering};

/// Sentinela: nenhuma HHDM válida do Limine começa em 0, então usamos 0 para
/// detectar "ainda não inicializado" sem precisar de `Option` atômico.
const UNINITIALIZED: u64 = 0;

static HHDM_OFFSET: AtomicU64 = AtomicU64::new(UNINITIALIZED);

/// Deve ser chamada uma única vez, no início de `kernel_main`, com o offset
/// reportado por `HhdmRequest`, antes de qualquer uso de `phys_to_virt`/
/// `virt_to_phys` ou de qualquer código que dependa deles (alocador físico,
/// paginação, heap).
pub fn init(offset: u64) {
    HHDM_OFFSET.store(offset, Ordering::Release);
}

/// Offset HHDM atualmente ativo. Entra em pânico se chamado antes de `init`.
pub fn offset() -> u64 {
    let offset = HHDM_OFFSET.load(Ordering::Acquire);
    assert!(offset != UNINITIALIZED, "hhdm::offset() chamado antes de hhdm::init()");
    offset
}

/// Converte um endereço físico no endereço virtual correspondente dentro da
/// higher-half direct map, dado um offset HHDM específico. Função pura,
/// testável sem hardware.
pub const fn phys_to_virt(hhdm_offset: u64, phys_addr: u64) -> u64 {
    phys_addr + hhdm_offset
}

/// Inverso de [`phys_to_virt`]: recupera o endereço físico a partir de um
/// endereço virtual dentro da HHDM.
pub const fn virt_to_phys(hhdm_offset: u64, virt_addr: u64) -> u64 {
    virt_addr - hhdm_offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phys_to_virt_applies_offset() {
        let offset = 0xffff_8000_0000_0000;
        let phys = 0x1234_5678u64;
        assert_eq!(phys_to_virt(offset, phys), phys + offset);
    }

    #[test]
    fn virt_to_phys_is_the_inverse_of_phys_to_virt() {
        let offset = 0xffff_8000_0000_0000;
        let phys = 0x0000_0008_7000u64;
        let virt = phys_to_virt(offset, phys);
        assert_eq!(virt_to_phys(offset, virt), phys);
    }

    #[test]
    fn round_trip_holds_for_zero_physical_address() {
        let offset = 0xffff_9000_0000_0000;
        assert_eq!(virt_to_phys(offset, phys_to_virt(offset, 0)), 0);
    }
}
