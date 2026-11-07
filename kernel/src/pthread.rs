//! `pthread_mutex`/`pthread_cond` nomeados — exclusão mútua (`Mutex`) e
//! variável de condição (`CondVar`) com bloqueio real de thread (sem
//! busy-wait), construídas sobre `kernel::wait_queue::WaitQueue` — ver spec
//! `kernel-thread-sync` e design.md da change `kernel-pthread-sync`.
//!
//! Diferença chave para `kernel::sync::Spinlock`: uma thread que não
//! consegue adquirir `Mutex` sai de rotação do escalonador em vez de girar
//! em busy-wait — `Spinlock` continua sendo a primitiva certa para seções
//! críticas curtas internas do próprio kernel; `Mutex`/`CondVar` são a
//! camada nomeada voltada a um futuro consumidor hóspede (a JVM).

use core::sync::atomic::{AtomicBool, Ordering};

use crate::process::ThreadId;
use crate::scheduler;
use crate::wait_queue::WaitQueue;

/// Exclusão mútua não-reentrante com bloqueio real (sem busy-wait) — base de
/// `pthread_mutex`. **Não-reentrante**: uma thread que já detém o mutex e
/// tenta adquiri-lo de novo bloqueia para sempre (deadlock consigo mesma),
/// mesmo comportamento de `std::sync::Mutex` do próprio Rust — ver
/// design.md, "Trade-off: `Mutex` não-reentrante".
pub struct Mutex {
    locked: AtomicBool,
    queue: WaitQueue,
}

impl Mutex {
    pub const fn new() -> Self {
        Self { locked: AtomicBool::new(false), queue: WaitQueue::new() }
    }

    /// Tenta adquirir o mutex sem bloquear — `true` se adquirido agora,
    /// `false` se já estava tomado (sem nenhum efeito colateral nesse
    /// caso). Lógica pura, sem nenhum acesso a `WaitQueue`/escalonador —
    /// testável em host. Exposta a `kernel::syscall::sys_mutex_trylock`
    /// (change `os-javaos-platform-sync`), que precisa de uma tentativa de
    /// aquisição que nunca bloqueia — `lock()` (abaixo) é o caminho normal
    /// para qualquer chamador que aceita bloquear.
    pub(crate) fn try_acquire(&self) -> bool {
        self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    /// Libera o estado do mutex (só o flag atômico, sem acordar ninguém) —
    /// lógica pura. Exposta a `CondVar::wait`, que precisa liberar o mutex
    /// como parte da mesma operação atômica em que registra sua própria
    /// intenção de bloqueio (ver design.md) — `unlock` (abaixo) é o
    /// caminho normal para qualquer outro chamador.
    pub(crate) fn release_state(&self) {
        self.locked.store(false, Ordering::Release);
    }

    /// Acorda uma thread esperando para adquirir este mutex, se houver.
    /// Exposta a `CondVar::wait` pelo mesmo motivo de [`Self::release_state`].
    pub(crate) fn wake_one_waiter(&self) {
        self.queue.wake_one();
    }

    /// Adquire o mutex, bloqueando (sem busy-wait) se já estiver tomado.
    /// Nunca perde um `unlock()` concorrente: a checagem de "ainda tomado?"
    /// e o registro da intenção de bloqueio acontecem como uma única
    /// operação atômica em relação a interrupções (`WaitQueue::block_current_if`)
    /// — ver design.md, "`Mutex::lock()` evita despertar perdido".
    pub fn lock(&'static self) {
        loop {
            if self.try_acquire() {
                return;
            }
            self.queue.block_current_if(|| self.locked.load(Ordering::Acquire));
        }
    }

    /// Libera o mutex e acorda uma thread em espera para adquiri-lo, se
    /// houver.
    pub fn unlock(&self) {
        self.release_state();
        self.wake_one_waiter();
    }
}

/// Variável de condição no estilo POSIX/Java — `wait` libera o mutex
/// associado e bloqueia a thread corrente atomicamente (sem janela em que o
/// mutex pareça detido por uma thread já bloqueada), reaquirindo-o antes de
/// retornar quando despertada.
///
/// A garantia de nenhuma notificação perdida depende do chamador notificar
/// (`notify_one`/`notify_all`) com o mesmo mutex associado detido — mesmo
/// contrato de `pthread_cond_signal`/`Object.notify` (ver design.md,
/// "`CondVar::wait(mutex)` depende da convenção..."). Notificar sem deter o
/// mutex associado pode perder a notificação para uma thread que esteja,
/// nesse exato instante, no meio de uma chamada a `wait`.
pub struct CondVar {
    queue: WaitQueue,
}

impl CondVar {
    pub const fn new() -> Self {
        Self { queue: WaitQueue::new() }
    }

    /// Libera `mutex` e bloqueia a thread corrente nesta condição; reaquire
    /// `mutex` antes de retornar, quando despertada por `notify_one`/
    /// `notify_all`.
    ///
    /// A intenção de bloqueio é registrada e o mutex é liberado como uma
    /// única operação atômica em relação a interrupções — sem isso, um tick
    /// real de hardware poderia interceptar a intenção e bloquear esta
    /// thread *antes* dela liberar o mutex, deixando-o preso para sempre
    /// (ver design.md, achado durante a implementação desta change).
    pub fn wait(&'static self, mutex: &'static Mutex) {
        let current: ThreadId = scheduler::current_thread_id();
        scheduler::with_interrupts_disabled(|| {
            self.queue.register_intent_for(current);
            mutex.release_state();
        });
        mutex.wake_one_waiter();
        self.queue.commit_block();
        mutex.lock();
    }

    /// Desperta a thread esperando há mais tempo nesta condição, se houver.
    pub fn notify_one(&self) {
        self.queue.wake_one();
    }

    /// Desperta todas as threads atualmente esperando nesta condição.
    pub fn notify_all(&self) {
        self.queue.wake_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_acquire_succeeds_when_free_and_marks_it_taken() {
        let mutex = Mutex::new();
        assert!(mutex.try_acquire());
        assert!(mutex.locked.load(Ordering::Relaxed));
    }

    #[test]
    fn try_acquire_fails_without_side_effects_when_already_taken() {
        let mutex = Mutex::new();
        assert!(mutex.try_acquire());

        // A segunda tentativa falha e não altera nada.
        assert!(!mutex.try_acquire());
        assert!(mutex.locked.load(Ordering::Relaxed));
    }

    #[test]
    fn release_state_always_leaves_it_free() {
        let mutex = Mutex::new();
        mutex.try_acquire();

        mutex.release_state();

        assert!(!mutex.locked.load(Ordering::Relaxed));
        // Livre de novo: uma nova aquisição deve funcionar.
        assert!(mutex.try_acquire());
    }
}
