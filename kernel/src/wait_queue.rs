//! Primitiva de bloqueio/despertar de thread (fila de espera) — ver spec
//! `kernel-scheduler` (delta da change `kernel-wait-queue`) e design.md.
//!
//! Uma thread em execução pode bloquear voluntariamente numa [`WaitQueue`]
//! (sai da rotação do escalonador, sem busy-wait) até outra thread — ou uma
//! ISR — a despertar (`wake_one`/`wake_all`).

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

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

    /// Remove `id` desta fila, em qualquer posição — devolve se encontrou e
    /// removeu. Usada por [`WaitQueue::cancel_if_blocked`] (change
    /// `kernel-timed-wait`): uma thread com prazo expirado pode não estar
    /// mais na cabeça da fila.
    fn remove(&mut self, id: ThreadId) -> bool {
        if let Some(pos) = self.order.iter().position(|&blocked| blocked == id) {
            self.order.remove(pos);
            true
        } else {
            false
        }
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
        self.commit_block();
    }

    /// Remove `id` desta fila se ela ainda estiver bloqueada aqui — devolve
    /// se removeu. Usada só por [`wake_expired_deadlines`], sempre dentro da
    /// seção crítica de `on_timer_tick` (interrupções já desabilitadas, mesmo
    /// precedente de [`park`](Self::park)/[`take_block_intent`]): se `id` já
    /// não está mais bloqueada aqui, já foi despertada normalmente antes do
    /// prazo expirar, e esta chamada não deve ter nenhum efeito. Lógica pura
    /// — testável em host sem tocar hardware.
    fn cancel_if_blocked(&self, id: ThreadId) -> bool {
        self.blocked.lock().remove(id)
    }

    /// Bloqueia a thread corrente nesta fila **somente se** `predicate`
    /// ainda for verdadeiro no instante em que a intenção de bloqueio seria
    /// registrada — a reavaliação de `predicate` e o registro da intenção
    /// acontecem como uma única operação atômica em relação a interrupções
    /// (`with_interrupts_disabled`), para que nenhum `wake_one`/`wake_all`
    /// concorrente (que torne `predicate` falso) possa acontecer entre a
    /// checagem original do chamador e este bloqueio — ver design.md da
    /// change `kernel-pthread-sync`, "`Mutex::lock()` evita despertar
    /// perdido". Usada por `kernel::pthread::Mutex::lock`.
    pub(crate) fn block_current_if(&'static self, predicate: impl FnOnce() -> bool) {
        let current = scheduler::current_thread_id();
        let should_block = scheduler::with_interrupts_disabled(|| self.try_register_block_intent(current, predicate));
        if should_block {
            self.commit_block();
        }
    }

    /// Lógica pura de `block_current_if`: reavalia `predicate` e, se
    /// verdadeiro, registra a intenção de bloqueio para `current` nesta
    /// fila, devolvendo se registrou. Separada do método público só para
    /// ser testável em host sem depender de `scheduler::current_thread_id`
    /// (hardware real). O chamador garante que isto roda com interrupções
    /// já desabilitadas.
    fn try_register_block_intent(&'static self, current: ThreadId, predicate: impl FnOnce() -> bool) -> bool {
        if predicate() {
            set_block_intent(current, self);
            true
        } else {
            false
        }
    }

    /// Registra a intenção de bloqueio da thread corrente nesta fila, **sem
    /// ainda bloquear** — usado por `kernel::pthread::CondVar::wait`, que
    /// precisa registrar a intenção atomicamente **junto com** a liberação
    /// do mutex associado (chamador deve envolver esta chamada e a
    /// liberação do mutex no mesmo `scheduler::with_interrupts_disabled`),
    /// para que nenhum tick real de hardware possa interceptar a intenção e
    /// bloquear a thread *antes* dela liberar o mutex (o que a deixaria
    /// presa para sempre, com o mutex nunca liberado) — ver design.md da
    /// change `kernel-pthread-sync`. Chamador deve, em seguida, chamar
    /// [`Self::commit_block`] para efetivar o bloqueio de fato.
    pub(crate) fn register_intent_for(&'static self, current: ThreadId) {
        set_block_intent(current, self);
    }

    /// Mesmo papel de [`Self::register_intent_for`], mas também registra um
    /// prazo em [`TIMED_WAITERS`] — usada por
    /// `kernel::pthread::CondVar::wait_timeout`, que precisa da mesma
    /// atomicidade de liberar o mutex associado junto com o registro da
    /// intenção (ver design.md da change `kernel-pthread-sync`), agora
    /// também com um prazo.
    pub(crate) fn register_timed_intent_for(&'static self, current: ThreadId, deadline_nanos: u64) {
        set_block_intent(current, self);
        TIMED_WAITERS.lock().push((current, deadline_nanos, self));
    }

    /// Dispara a interrupção de software dedicada a pedidos de bloqueio
    /// (`scheduler::BLOCK_VECTOR`), reentrando no mecanismo de
    /// salvar/restaurar contexto já usado pelo timer, mas num vetor
    /// distinto — ver design.md da change `kernel-wait-queue`, "Achados
    /// durante a implementação". Só retorna quando esta thread for de fato
    /// despertada e escolhida de novo pelo escalonador, retomando
    /// exatamente de onde bloqueou. Assume que uma intenção de bloqueio já
    /// foi registrada para a thread corrente (por `block_current`,
    /// `block_current_if` ou `register_intent_for`).
    pub(crate) fn commit_block(&self) {
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

/// Prazos pendentes: `(thread, prazo em nanossegundos monotônicos, fila de
/// onde despertar)` — varrida inteiramente por [`wake_expired_deadlines`] a
/// cada tick. Lista simples, sem ordenação (ver design.md da change
/// `kernel-timed-wait`, "Lista de prazos pendentes"): o número de esperas
/// com prazo concorrentes é pequeno, mesma escala de `ReadyQueue`.
static TIMED_WAITERS: Spinlock<Vec<(ThreadId, u64, &'static WaitQueue)>> = Spinlock::new(Vec::new());

fn set_block_intent(id: ThreadId, queue: &'static WaitQueue) {
    // Acha real (change kernel-timed-wait): descarta qualquer prazo pendente
    // desta mesma thread antes de registrar qualquer nova intenção de
    // bloqueio (com ou sem prazo) - sem isso, uma thread notificada antes do
    // prazo antigo expirar, que iniciasse uma nova espera antes dele
    // disparar, poderia ser acordada incorretamente pelo prazo antigo. Ver
    // design.md, "Invariante contra prazos presos".
    TIMED_WAITERS.lock().retain(|&(waiter, _, _)| waiter != id);
    *BLOCK_INTENT.lock() = Some((id, queue));
}

/// Remove de [`TIMED_WAITERS`] toda entrada cujo prazo já expirou
/// (`<= now_nanos`) e devolve `(thread, fila)` de cada uma — lógica pura,
/// testável em host sem tocar o escalonador. Separada de
/// [`wake_expired_deadlines`] pelo mesmo motivo de
/// `WaitQueue::try_register_block_intent`: isolar a decisão pura da parte que
/// toca hardware/escalonador.
fn take_expired_deadlines(now_nanos: u64) -> Vec<(ThreadId, &'static WaitQueue)> {
    let mut waiters = TIMED_WAITERS.lock();
    let mut expired = Vec::new();
    waiters.retain(|&(id, deadline, queue)| {
        if deadline <= now_nanos {
            expired.push((id, queue));
            false
        } else {
            true
        }
    });
    expired
}

/// Chamada por `kernel::scheduler::on_timer_tick`, sempre com interrupções já
/// desabilitadas (mesmo precedente de [`take_block_intent`]): para cada
/// prazo expirado (ver [`take_expired_deadlines`]), tenta despertar a thread
/// correspondente (`WaitQueue::cancel_if_blocked`) — se ela ainda estava
/// bloqueada ali, marca `timed_out` e a devolve à rotação; se não (já
/// despertada normalmente antes), não faz nada. Ver design.md, "Lista de
/// prazos pendentes".
pub(crate) fn wake_expired_deadlines(now_nanos: u64) {
    for (id, queue) in take_expired_deadlines(now_nanos) {
        if queue.cancel_if_blocked(id) {
            scheduler::mark_timed_out(id);
            scheduler::make_ready(id);
        }
    }
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

    /// Serializa os testes que tocam o estado global `BLOCK_INTENT`: o
    /// executor de testes do Rust roda testes em paralelo (threads
    /// distintas) por padrão, e `BLOCK_INTENT` é um único `static`
    /// compartilhado por todo o processo de teste — sem isso, dois desses
    /// testes rodando ao mesmo tempo podem interferir um no outro (cada um
    /// limpando/checando o estado que o outro acabou de escrever). Achado
    /// ao adicionar mais testes que tocam o mesmo estado nesta change
    /// (`kernel-pthread-sync`); os testes puros de `BlockedQueue`, que não
    /// tocam estado global nenhum, não precisam deste lock.
    static BLOCK_INTENT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    fn try_register_block_intent_does_nothing_when_predicate_is_false() {
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *BLOCK_INTENT.lock() = None;
        static QUEUE: WaitQueue = WaitQueue::new();

        let registered = QUEUE.try_register_block_intent(ThreadId(5), || false);

        assert!(!registered);
        assert!(BLOCK_INTENT.lock().is_none());
    }

    #[test]
    fn try_register_block_intent_registers_when_predicate_is_true() {
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *BLOCK_INTENT.lock() = None;
        static QUEUE: WaitQueue = WaitQueue::new();

        let registered = QUEUE.try_register_block_intent(ThreadId(6), || true);

        assert!(registered);
        // `take_block_intent` consome a intenção registrada: confirma que
        // ela realmente foi gravada para a thread certa, na fila certa.
        assert!(take_block_intent(ThreadId(6)));
        assert_eq!(QUEUE.blocked.lock().pop(), Some(ThreadId(6)));
    }

    #[test]
    fn taking_the_intent_parks_the_thread_in_the_target_queue() {
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *BLOCK_INTENT.lock() = None;

        static QUEUE: WaitQueue = WaitQueue::new();
        set_block_intent(ThreadId(99), &QUEUE);
        assert!(take_block_intent(ThreadId(99)));

        assert_eq!(QUEUE.blocked.lock().pop(), Some(ThreadId(99)));
    }

    #[test]
    fn cancel_if_blocked_removes_a_thread_still_in_the_queue() {
        static QUEUE: WaitQueue = WaitQueue::new();
        QUEUE.blocked.lock().push(ThreadId(200));

        assert!(QUEUE.cancel_if_blocked(ThreadId(200)));
        assert_eq!(QUEUE.blocked.lock().pop(), None);
    }

    #[test]
    fn cancel_if_blocked_does_nothing_when_the_thread_already_left_the_queue() {
        static QUEUE: WaitQueue = WaitQueue::new();
        // Nunca foi colocada na fila (equivalente a já ter sido despertada
        // normalmente antes do prazo expirar).
        assert!(!QUEUE.cancel_if_blocked(ThreadId(201)));
    }

    #[test]
    fn set_block_intent_purges_a_stale_timed_waiter_for_the_same_thread() {
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *BLOCK_INTENT.lock() = None;
        TIMED_WAITERS.lock().clear();

        static OLD_QUEUE: WaitQueue = WaitQueue::new();
        static NEW_QUEUE: WaitQueue = WaitQueue::new();
        TIMED_WAITERS.lock().push((ThreadId(300), 1_000, &OLD_QUEUE));

        // Mesma thread registra uma NOVA intenção de bloqueio (com ou sem
        // prazo) antes do prazo antigo expirar — ver design.md, "Invariante
        // contra prazos presos".
        set_block_intent(ThreadId(300), &NEW_QUEUE);

        assert!(TIMED_WAITERS.lock().iter().all(|&(id, _, _)| id != ThreadId(300)));
    }

    #[test]
    fn set_block_intent_does_not_purge_a_different_thread_s_pending_deadline() {
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *BLOCK_INTENT.lock() = None;
        TIMED_WAITERS.lock().clear();

        static QUEUE_A: WaitQueue = WaitQueue::new();
        static QUEUE_B: WaitQueue = WaitQueue::new();
        TIMED_WAITERS.lock().push((ThreadId(301), 1_000, &QUEUE_A));

        set_block_intent(ThreadId(302), &QUEUE_B);

        assert!(TIMED_WAITERS.lock().iter().any(|&(id, _, _)| id == ThreadId(301)));
    }

    #[test]
    fn take_expired_deadlines_removes_only_entries_at_or_before_now() {
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        TIMED_WAITERS.lock().clear();
        static QUEUE: WaitQueue = WaitQueue::new();
        TIMED_WAITERS.lock().push((ThreadId(400), 100, &QUEUE));
        TIMED_WAITERS.lock().push((ThreadId(401), 200, &QUEUE));

        let expired = take_expired_deadlines(100);

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, ThreadId(400));
        assert!(core::ptr::eq(expired[0].1, &QUEUE));
        // A entrada ainda não expirada continua pendente.
        assert!(TIMED_WAITERS.lock().iter().any(|&(id, _, _)| id == ThreadId(401)));
        assert!(TIMED_WAITERS.lock().iter().all(|&(id, _, _)| id != ThreadId(400)));
    }

    #[test]
    fn take_expired_deadlines_finds_nothing_when_no_deadline_passed() {
        let _guard = BLOCK_INTENT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        TIMED_WAITERS.lock().clear();
        static QUEUE: WaitQueue = WaitQueue::new();
        TIMED_WAITERS.lock().push((ThreadId(402), 1_000, &QUEUE));

        assert!(take_expired_deadlines(500).is_empty());
        assert!(TIMED_WAITERS.lock().iter().any(|&(id, _, _)| id == ThreadId(402)));
    }
}
