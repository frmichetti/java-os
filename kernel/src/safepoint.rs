//! Registro de faixas de endereço virtual cuja falta de página por ausência
//! é recuperável — a base do "sinal de safepoint" da JVM. Ver spec
//! `kernel-safepoint` e `design.md` da change `kernel-safepoint-signal`.
//!
//! Dividido no mesmo padrão de `kernel::vmm`/`kernel::pthread_handles`: uma
//! camada pura ([`SafepointRegistry`], sem nenhum acesso a `kernel::vmm`) e
//! uma função de integração ([`register`]) que valida a faixa contra uma
//! reserva viva antes de inserir.

extern crate alloc;

use alloc::vec::Vec;

use crate::sync::Spinlock;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SafepointError {
    /// `[base, base+size)` não cai inteiramente dentro de uma reserva de
    /// memória virtual viva (`kernel::vmm`).
    NotAReservedRange,
}

/// Faixas registradas como ponto de poll de safepoint — lógica pura (sem
/// nenhum acesso a `kernel::vmm`/hardware), testável em host.
struct SafepointRegistry {
    ranges: Vec<(u64, u64)>,
}

impl SafepointRegistry {
    const fn empty() -> Self {
        Self { ranges: Vec::new() }
    }

    fn register(&mut self, base: u64, size: u64) {
        self.ranges.push((base, size));
    }

    /// `true` se `addr` cair dentro de alguma faixa registrada (`[base,
    /// base+size)`, limite superior exclusivo).
    fn contains(&self, addr: u64) -> bool {
        self.ranges.iter().any(|&(base, size)| addr >= base && addr < base + size)
    }
}

static REGISTRY: Spinlock<SafepointRegistry> = Spinlock::new(SafepointRegistry::empty());

/// Registra `[base, base+size)` como ponto de poll de safepoint: uma falta
/// de página por ausência nessa faixa deixa de ser fatal (ver
/// `kernel::idt::page_fault_handler`). Exige que a faixa caia inteiramente
/// dentro de uma reserva de memória virtual viva (`kernel::vmm::VMM`) —
/// cobre "Registro de Faixa de Safepoint" da spec.
pub fn register(base: u64, size: u64) -> Result<(), SafepointError> {
    let region = crate::vmm::VMM.lock().region_containing(base).ok_or(SafepointError::NotAReservedRange)?;
    let end = base.checked_add(size).ok_or(SafepointError::NotAReservedRange)?;
    if end > region.base() + region.size() {
        return Err(SafepointError::NotAReservedRange);
    }
    REGISTRY.lock().register(base, size);
    Ok(())
}

/// `true` se `addr` cair dentro de alguma faixa registrada via [`register`].
/// Consultada por `kernel::idt::page_fault_handler`.
pub fn is_registered(addr: u64) -> bool {
    REGISTRY.lock().contains(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------
    // SafepointRegistry (tarefa 1.1): lógica pura.
    // -------------------------------------------------------------

    #[test]
    fn address_inside_a_registered_range_is_contained() {
        let mut registry = SafepointRegistry::empty();
        registry.register(0x1000, 0x1000);
        assert!(registry.contains(0x1000));
        assert!(registry.contains(0x1800));
    }

    #[test]
    fn address_outside_any_registered_range_is_not_contained() {
        let mut registry = SafepointRegistry::empty();
        registry.register(0x1000, 0x1000);
        assert!(!registry.contains(0x0fff));
        assert!(!registry.contains(0x3000));
    }

    #[test]
    fn upper_bound_is_exclusive() {
        let mut registry = SafepointRegistry::empty();
        registry.register(0x1000, 0x1000);
        assert!(!registry.contains(0x1000 + 0x1000));
    }

    #[test]
    fn querying_works_across_multiple_registered_ranges() {
        let mut registry = SafepointRegistry::empty();
        registry.register(0x1000, 0x1000);
        registry.register(0x5000, 0x1000);
        assert!(registry.contains(0x1500));
        assert!(registry.contains(0x5500));
        assert!(!registry.contains(0x3000));
    }

    // -------------------------------------------------------------
    // register()/is_registered() (tarefas 1.2/1.3): integração com
    // `kernel::vmm::VMM`.
    // -------------------------------------------------------------

    /// Serializa os testes que tocam `kernel::vmm::VMM` — compartilhado com
    /// `kernel::syscall::tests`, já que módulos diferentes rodam em paralelo
    /// por padrão — ver `kernel::vmm::test_support`.
    use crate::vmm::test_support::{VMM_TEST_LOCK as TEST_LOCK, ensure_vmm_initialized};

    #[test]
    fn registering_a_range_inside_a_live_reservation_is_accepted() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ensure_vmm_initialized();

        let region = crate::vmm::VMM.lock().reserve(crate::paging::PAGE_SIZE).expect("reserva deveria ter sucesso");
        assert!(register(region.base(), region.size()).is_ok());
        assert!(is_registered(region.base()));
    }

    #[test]
    fn registering_a_range_outside_any_live_reservation_is_rejected() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ensure_vmm_initialized();

        // Endereco 0 nunca corresponde a nenhuma reserva viva da janela do VMM.
        assert_eq!(register(0, crate::paging::PAGE_SIZE), Err(SafepointError::NotAReservedRange));
        assert!(!is_registered(0));
    }

    #[test]
    fn an_address_never_registered_is_not_registered() {
        assert!(!is_registered(0xdead_beef));
    }
}
