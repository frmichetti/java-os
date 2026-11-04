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

use crate::pthread::{CondVar, Mutex};
use crate::sync::Spinlock;

static MUTEXES: Spinlock<Vec<&'static Mutex>> = Spinlock::new(Vec::new());
static CONDVARS: Spinlock<Vec<&'static CondVar>> = Spinlock::new(Vec::new());

/// Cria um novo mutex nomeado, inicialmente livre, e devolve o `handle` pelo
/// qual ele pode ser referenciado por [`mutex`]. Nunca falha nem é
/// reaproveitado (a tabela só cresce — ver design.md, Non-Goals: sem
/// destruição).
pub fn create_mutex() -> u64 {
    let leaked: &'static Mutex = Box::leak(Box::new(Mutex::new()));
    let mut table = MUTEXES.lock();
    table.push(leaked);
    (table.len() - 1) as u64
}

/// Devolve o mutex identificado por `handle`, ou `None` se `handle` não
/// corresponder a nenhum mutex criado por [`create_mutex`].
pub fn mutex(handle: u64) -> Option<&'static Mutex> {
    MUTEXES.lock().get(handle as usize).copied()
}

/// Cria uma nova variável de condição nomeada, sem nenhuma thread esperando
/// nela, e devolve o `handle` pelo qual ela pode ser referenciada por
/// [`cond`]. Nunca falha nem é reaproveitado (mesma política de
/// [`create_mutex`]).
pub fn create_cond() -> u64 {
    let leaked: &'static CondVar = Box::leak(Box::new(CondVar::new()));
    let mut table = CONDVARS.lock();
    table.push(leaked);
    (table.len() - 1) as u64
}

/// Devolve a variável de condição identificada por `handle`, ou `None` se
/// `handle` não corresponder a nenhuma criada por [`create_cond`].
pub fn cond(handle: u64) -> Option<&'static CondVar> {
    CONDVARS.lock().get(handle as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_mutex_twice_returns_distinct_handles() {
        let a = create_mutex();
        let b = create_mutex();
        assert_ne!(a, b);
    }

    #[test]
    fn a_freshly_created_mutex_is_initially_free() {
        let handle = create_mutex();
        let found = mutex(handle).expect("handle recem-criado deveria ser encontrado");
        // `lock()` de um mutex livre retorna imediatamente (CAS pura, sem
        // tocar `WaitQueue`/escalonador) — ver design.md, "Escopo de testes
        // em host". Não chama `unlock()`: sempre toca
        // `scheduler::with_interrupts_disabled` (`cli`/`sti`), inseguro em
        // host mesmo sem ninguém esperando.
        found.lock();
    }

    #[test]
    fn mutex_with_an_out_of_bounds_handle_is_not_found() {
        let handle = create_mutex();
        assert!(mutex(handle + 1_000_000).is_none());
    }

    #[test]
    fn create_cond_twice_returns_distinct_handles() {
        let a = create_cond();
        let b = create_cond();
        assert_ne!(a, b);
    }

    #[test]
    fn cond_with_an_out_of_bounds_handle_is_not_found() {
        let handle = create_cond();
        assert!(cond(handle + 1_000_000).is_none());
    }
}
