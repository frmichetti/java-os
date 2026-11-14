//! Tabelas de identificador (`handle`) opaco para `Mutex`/`CondVar`
//! nomeados, criados dinamicamente a pedido de um chamador de modo usuário —
//! ver design.md da change `kernel-jvm-posix-syscalls`, "Tabelas de handle
//! para `Mutex`/`CondVar`".
//!
//! `kernel::pthread::Mutex`/`CondVar` exigem referência `'static` em todos
//! os métodos bloqueantes; este módulo é o único lugar do kernel que aloca
//! instâncias dinamicamente (`Box::leak`) para obter essa referência,
//! mantendo `kernel::pthread` em si livre dessa decisão — a mesma separação
//! de responsabilidade já usada entre `kernel::vmm` (a primitiva) e a
//! tradução addr→`Region` em `kernel::syscall` (a ponte de syscall).

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::process::ProcessId;
use crate::pthread::{CondVar, Mutex};
use crate::sync::Spinlock;

static MUTEXES: Spinlock<Vec<(ProcessId, &'static Mutex)>> = Spinlock::new(Vec::new());
static CONDVARS: Spinlock<Vec<(ProcessId, &'static CondVar)>> = Spinlock::new(Vec::new());

/// Cria um novo mutex nomeado, inicialmente livre, dono `owner`, e devolve o
/// `handle` pelo qual ele pode ser referenciado por [`mutex`] (só pelo mesmo
/// `owner` — ver design.md da change `kernel-handle-ownership`). Nunca falha
/// nem é reaproveitado (a tabela só cresce — ver design.md, Non-Goals: sem
/// destruição).
pub fn create_mutex(owner: ProcessId) -> u64 {
    let leaked: &'static Mutex = Box::leak(Box::new(Mutex::new()));
    let mut table = MUTEXES.lock();
    table.push((owner, leaked));
    (table.len() - 1) as u64
}

/// Devolve o mutex identificado por `handle`, só se `caller` for o processo
/// que o criou (`None` tanto para um `handle` que nunca existiu quanto para
/// um que existe mas pertence a outro processo — mesmo sentinela de erro das
/// duas situações, ver design.md).
pub fn mutex(handle: u64, caller: ProcessId) -> Option<&'static Mutex> {
    match MUTEXES.lock().get(handle as usize) {
        Some(&(owner, mutex)) if owner == caller => Some(mutex),
        _ => None,
    }
}

/// Cria uma nova variável de condição nomeada, sem nenhuma thread esperando
/// nela, dona `owner`, e devolve o `handle` pelo qual ela pode ser
/// referenciada por [`cond`] (só pelo mesmo `owner`). Nunca falha nem é
/// reaproveitado (mesma política de [`create_mutex`]).
pub fn create_cond(owner: ProcessId) -> u64 {
    let leaked: &'static CondVar = Box::leak(Box::new(CondVar::new()));
    let mut table = CONDVARS.lock();
    table.push((owner, leaked));
    (table.len() - 1) as u64
}

/// Devolve a variável de condição identificada por `handle`, só se `caller`
/// for o processo que a criou — mesmo racional de [`mutex`].
pub fn cond(handle: u64, caller: ProcessId) -> Option<&'static CondVar> {
    match CONDVARS.lock().get(handle as usize) {
        Some(&(owner, cond)) if owner == caller => Some(cond),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: ProcessId = ProcessId(1);
    const OTHER: ProcessId = ProcessId(2);

    #[test]
    fn create_mutex_twice_returns_distinct_handles() {
        let a = create_mutex(OWNER);
        let b = create_mutex(OWNER);
        assert_ne!(a, b);
    }

    #[test]
    fn a_freshly_created_mutex_is_initially_free() {
        let handle = create_mutex(OWNER);
        let found = mutex(handle, OWNER).expect("handle recem-criado deveria ser encontrado pelo dono");
        // `lock()` de um mutex livre retorna imediatamente (CAS pura, sem
        // tocar `WaitQueue`/escalonador) — ver design.md, "Escopo de testes
        // em host". Não chama `unlock()`: sempre toca
        // `scheduler::with_interrupts_disabled` (`cli`/`sti`), inseguro em
        // host mesmo sem ninguém esperando.
        found.lock();
    }

    #[test]
    fn mutex_with_an_out_of_bounds_handle_is_not_found() {
        let handle = create_mutex(OWNER);
        assert!(mutex(handle + 1_000_000, OWNER).is_none());
    }

    #[test]
    fn mutex_created_by_another_process_is_not_found() {
        let handle = create_mutex(OWNER);
        assert!(mutex(handle, OTHER).is_none());
    }

    #[test]
    fn create_cond_twice_returns_distinct_handles() {
        let a = create_cond(OWNER);
        let b = create_cond(OWNER);
        assert_ne!(a, b);
    }

    #[test]
    fn cond_with_an_out_of_bounds_handle_is_not_found() {
        let handle = create_cond(OWNER);
        assert!(cond(handle + 1_000_000, OWNER).is_none());
    }

    #[test]
    fn cond_created_by_another_process_is_not_found() {
        let handle = create_cond(OWNER);
        assert!(cond(handle, OTHER).is_none());
    }
}
