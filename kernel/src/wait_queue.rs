//! Primitiva de bloqueio/despertar de thread (fila de espera) — ver spec
//! `kernel-scheduler` (delta da change `kernel-wait-queue`) e design.md.
//!
//! Uma thread em execução pode bloquear voluntariamente numa [`WaitQueue`]
//! (sai da rotação do escalonador, sem busy-wait) até outra thread — ou uma
//! ISR — a despertar (`wake_one`/`wake_all`).

extern crate alloc;

use alloc::collections::VecDeque;

use crate::process::ThreadId;
use crate::scheduler;
use crate::sync::Spinlock;

/// Fila FIFO pura de threads bloqueadas — sem nenhum acesso a hardware,
/// testável em host. Usada como a lista interna de cada [`WaitQueue`].
struct BlockedQueue {
    order: VecDeque<ThreadId>,
}

impl BlockedQueue {
    const fn new() -> Self {
        Self { order: VecDeque::new() }
    }

    fn push(&mut self, id: ThreadId) {
        self.order.push_back(id);
    }

    /// Remove e devolve a thread bloqueada há mais tempo — `None` se não
    /// houver nenhuma.
    fn pop(&mut self) -> Option<ThreadId> {
        self.order.pop_front()
    }
}

/// Uma fila de espera nomeada: ver design.md, "`WaitQueue`: FIFO simples,
/// `wake_one`/`wake_all`, sem associação a um valor de memória específico".
/// Uma instância `static` por "canal" de espera (mesmo padrão de
/// `kernel::keyboard::QUEUE`).
pub struct WaitQueue {
    blocked: Spinlock<BlockedQueue>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self { blocked: Spinlock::new(BlockedQueue::new()) }
    }

    /// Bloqueia a thread corrente nesta fila: registra a intenção de
    /// bloqueio (consultada por `scheduler::schedule_next` antes de decidir
    /// o destino da thread interrompida) e dispara a interrupção de software
    /// dedicada a pedidos de bloqueio (`scheduler::BLOCK_VECTOR`), reentrando
    /// no mesmo mecanismo de salvar/restaurar contexto já usado pelo timer,
    /// mas num vetor distinto — ver design.md, "Achados durante a
    /// implementação" (reaproveitar o próprio vetor do timer travava o
    /// Local APIC, por causa do EOI espúrio de um `int` de software). Só
    /// retorna quando esta thread for de fato despertada (`wake_one`/
    /// `wake_all`) e escolhida de novo pelo escalonador, retomando
    /// exatamente de onde bloqueou.
    pub fn block_current(&'static self) {
        let current = scheduler::current_thread_id();
        scheduler::with_interrupts_disabled(|| set_block_intent(current, self));

        unsafe {
            core::arch::asm!(
                "int {vector}",
                vector = const crate::scheduler::BLOCK_VECTOR,
                options(nomem, nostack)
            );
        }
    }

    /// Desperta a thread bloqueada há mais tempo nesta fila (se houver),
    /// devolvendo-a à rotação normal do escalonador. Seguro para chamar a
    /// partir de uma ISR — toda a seção crítica usa
    /// `with_interrupts_disabled`, nunca mantendo um lock além do mínimo
    /// necessário (ver design.md, "Riscos/Trade-offs").
    pub fn wake_one(&self) {
        let woken = scheduler::with_interrupts_disabled(|| self.blocked.lock().pop());
        if let Some(id) = woken {
            scheduler::make_ready(id);
        }
    }

    /// Desperta todas as threads atualmente bloqueadas nesta fila, na mesma
    /// ordem FIFO em que bloquearam.
    pub fn wake_all(&self) {
        loop {
            let woken = scheduler::with_interrupts_disabled(|| self.blocked.lock().pop());
            match woken {
                Some(id) => scheduler::make_ready(id),
                None => break,
            }
        }
    }

    /// Move `id` para a fila de bloqueados desta `WaitQueue`, em vez de
    /// recolocá-la na `ReadyQueue`. Chamada só por [`take_block_intent`],
    /// sempre dentro da seção crítica de `on_timer_tick` (interrupções já
    /// desabilitadas), nunca diretamente.
    fn park(&self, id: ThreadId) {
        self.blocked.lock().push(id);
    }
}

/// "Intenção de bloqueio" pendente: qual thread pretende ser desviada para
/// qual `WaitQueue`, registrada por [`WaitQueue::block_current`] e consumida
/// por [`take_block_intent`] — ver design.md. Toda leitura/escrita acontece
/// com interrupções desabilitadas (mesmo precedente de
/// `kernel::keyboard::poll_event`).
static BLOCK_INTENT: Spinlock<Option<(ThreadId, &'static WaitQueue)>> = Spinlock::new(None);

fn set_block_intent(id: ThreadId, queue: &'static WaitQueue) {
    *BLOCK_INTENT.lock() = Some((id, queue));
}

/// Chamada por `kernel::scheduler::on_timer_tick`, sempre com interrupções já
/// desabilitadas: se há uma intenção de bloqueio pendente para
/// `was_running`, consome-a (nunca persiste além de um único tick) e move a
/// thread para a `WaitQueue` indicada, devolvendo `true`. Caso contrário
/// (qualquer tick normal, ou uma intenção pendente para outra thread), não
/// faz nada e devolve `false` — o chamador recoloca `was_running` no fim da
/// `ReadyQueue` normalmente.
pub(crate) fn take_block_intent(was_running: ThreadId) -> bool {
    let mut intent = BLOCK_INTENT.lock();
    match *intent {
        Some((id, queue)) if id == was_running => {
            *intent = None;
            drop(intent);
            queue.park(was_running);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_queue_wakes_threads_in_fifo_order() {
        let mut queue = BlockedQueue::new();
        queue.push(ThreadId(1));
        queue.push(ThreadId(2));

        assert_eq!(queue.pop(), Some(ThreadId(1)));
        assert_eq!(queue.pop(), Some(ThreadId(2)));
    }

    #[test]
    fn empty_blocked_queue_has_nothing_to_wake() {
        let mut queue = BlockedQueue::new();
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn take_block_intent_matches_only_the_registered_thread() {
        // Limpa qualquer intenção deixada por outro teste rodando em
        // paralelo no mesmo processo de host (estado global compartilhado).
        *BLOCK_INTENT.lock() = None;

        static QUEUE: WaitQueue = WaitQueue::new();
        set_block_intent(ThreadId(42), &QUEUE);

        // Uma thread diferente não deveria consumir a intenção alheia.
        assert!(!take_block_intent(ThreadId(7)));

        assert!(take_block_intent(ThreadId(42)));
        // A intenção já foi consumida: uma segunda tentativa não encontra nada.
        assert!(!take_block_intent(ThreadId(42)));
    }

    #[test]
    fn taking_the_intent_parks_the_thread_in_the_target_queue() {
        *BLOCK_INTENT.lock() = None;

        static QUEUE: WaitQueue = WaitQueue::new();
        set_block_intent(ThreadId(99), &QUEUE);
        assert!(take_block_intent(ThreadId(99)));

        assert_eq!(QUEUE.blocked.lock().pop(), Some(ThreadId(99)));
    }
}
