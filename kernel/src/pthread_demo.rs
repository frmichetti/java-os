//! Demonstração de verificação de `kernel::pthread` — dois cenários,
//! análogos aos demos anteriores: (a) várias threads disputando um `Mutex`
//! para incrementar um contador compartilhado, provando exclusão mútua real
//! (nenhum incremento perdido); (b) um par produtor/consumidor usando
//! `Mutex`+`CondVar` no estilo `Object.wait`/`notify` — ver design.md da
//! change `kernel-pthread-sync`.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::pthread::{CondVar, Mutex};

// --- Cenário (a): contador disputado por várias threads sob um `Mutex` ---

/// Quantas threads disputam `COUNTER_MUTEX` — todas executam a mesma função
/// ([`counter_worker`]), cada uma com sua própria thread de kernel.
pub const COUNTER_THREAD_COUNT: u64 = 4;

const INCREMENTS_PER_THREAD: u64 = 2000;

static COUNTER_MUTEX: Mutex = Mutex::new();

/// Protegido inteiramente por `COUNTER_MUTEX` — nunca acessado fora de uma
/// seção `lock()`/`unlock()`.
static mut COUNTER: u64 = 0;

/// Quantas das `COUNTER_THREAD_COUNT` threads já terminaram seus
/// incrementos — a última a terminar reporta o resultado final.
static COUNTER_THREADS_DONE: AtomicU64 = AtomicU64::new(0);

fn idle_forever() -> ! {
    loop {
        unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
    }
}

/// Thread de kernel: incrementa `COUNTER` `INCREMENTS_PER_THREAD` vezes,
/// sempre sob `COUNTER_MUTEX`. A última das `COUNTER_THREAD_COUNT` threads a
/// terminar confirma que o total bate exatamente com a soma esperada — se
/// `Mutex` deixasse passar um incremento concorrente sem exclusão real, o
/// total ficaria abaixo do esperado.
pub extern "C" fn counter_worker() -> ! {
    for _ in 0..INCREMENTS_PER_THREAD {
        COUNTER_MUTEX.lock();
        unsafe { COUNTER += 1 };
        COUNTER_MUTEX.unlock();
    }

    if COUNTER_THREADS_DONE.fetch_add(1, Ordering::AcqRel) + 1 == COUNTER_THREAD_COUNT {
        let total = unsafe { COUNTER };
        let expected = INCREMENTS_PER_THREAD * COUNTER_THREAD_COUNT;
        if total == expected {
            crate::serial_println!("pthread: contador final={} (esperado={}) - PASSOU", total, expected);
        } else {
            crate::serial_println!(
                "pthread: FALHOU contador final={} (esperado={}, incremento perdido)",
                total,
                expected
            );
        }
    }

    idle_forever();
}

// --- Cenário (b): produtor/consumidor com `Mutex` + `CondVar` ---

static BOX_MUTEX: Mutex = Mutex::new();
static BOX_READY: CondVar = CondVar::new();

/// Protegido inteiramente por `BOX_MUTEX` — nunca acessado fora de uma
/// seção `lock()`/`unlock()`.
static mut BOX_ITEM: Option<u64> = None;

const BOX_ITEM_VALUE: u64 = 42;

/// Thread produtora: adquire `BOX_MUTEX`, deposita o item e notifica
/// `BOX_READY` — exatamente como `synchronized { item = x; notify(); }` em
/// Java.
pub extern "C" fn box_producer() -> ! {
    BOX_MUTEX.lock();
    unsafe { *core::ptr::addr_of_mut!(BOX_ITEM) = Some(BOX_ITEM_VALUE) };
    BOX_READY.notify_one();
    BOX_MUTEX.unlock();
    crate::serial_println!("pthread: produtora colocou o item {} na caixa", BOX_ITEM_VALUE);

    idle_forever();
}

/// Thread consumidora: espera (via `CondVar`, sem busy-wait) até a caixa não
/// estar mais vazia — em laço, seguindo a disciplina padrão de
/// `pthread_cond_wait`/`Object.wait` (reavaliar a condição ao acordar, nunca
/// assumir que o despertar implica a condição verdadeira) — exatamente como
/// `synchronized { while (item == null) wait(); x = item; }` em Java.
pub extern "C" fn box_consumer() -> ! {
    BOX_MUTEX.lock();
    while unsafe { (*core::ptr::addr_of!(BOX_ITEM)).is_none() } {
        BOX_READY.wait(&BOX_MUTEX);
    }
    let item = unsafe { (*core::ptr::addr_of_mut!(BOX_ITEM)).take() };
    BOX_MUTEX.unlock();

    match item {
        Some(value) if value == BOX_ITEM_VALUE => {
            crate::serial_println!("pthread: consumidora recebeu o item {} - PASSOU", value);
        }
        other => {
            crate::serial_println!("pthread: FALHOU consumidora recebeu {:?}, esperava {}", other, BOX_ITEM_VALUE);
        }
    }

    idle_forever();
}
