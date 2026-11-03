//! Demonstração de verificação de `kernel::wait_queue` — análoga a
//! `keyboard_demo`/`storage_demo`/`network_demo`: uma thread produtora
//! desperta periodicamente uma thread consumidora bloqueada — ver design.md
//! da change `kernel-wait-queue`.
//!
//! **Deliberadamente nunca imprime nada pelo console serial**: ver
//! `KNOW_ISSUES.md`, "Duas threads de kernel imprimindo concorrentemente
//! travam o timer de hardware para sempre" — um bug pré-existente,
//! encontrado durante esta change mas não relacionado a `wait_queue` (nem
//! corrigido ainda), que trava o kernel assim que uma segunda thread chama
//! `serial_println!` repetidamente enquanto `kernel::user_demo` também está
//! imprimindo. Mesmo um único `serial_println!` aqui se mostrou pouco
//! confiável em boot real (colidiu na maioria das tentativas) — ver
//! design.md. A verificação em boot real desta change usa inspeção de
//! memória (GDB) sobre [`wakeups`] em vez de mensagens no console serial.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::wait_queue::WaitQueue;

/// Fila de espera compartilhada entre as duas threads deste demo.
static QUEUE: WaitQueue = WaitQueue::new();

/// Quantas vezes a consumidora já foi despertada — único canal de
/// verificação deste demo (ver módulo), inspecionável por GDB em boot real
/// (`print kernel::wait_demo::WAKEUPS`).
static WAKEUPS: AtomicU64 = AtomicU64::new(0);

/// A cada quantos ticks do timer (100 Hz, ver `kernel::timer`) a produtora
/// desperta a consumidora.
const WAKE_INTERVAL_TICKS: u64 = 50;

/// Thread produtora: a cada `WAKE_INTERVAL_TICKS`, desperta a consumidora.
/// `hlt` (com interrupções habilitadas) entre verificações, em vez de
/// busy-wait apertado — mesmo cuidado já adotado por `keyboard_demo` (ver
/// seu design.md) para não competir desnecessariamente por CPU.
pub extern "C" fn producer() -> ! {
    let mut last_wake = crate::timer::ticks();
    loop {
        let now = crate::timer::ticks();
        if now.wrapping_sub(last_wake) >= WAKE_INTERVAL_TICKS {
            last_wake = now;
            QUEUE.wake_one();
        }
        unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
    }
}

/// Thread consumidora: bloqueia em `QUEUE` em laço. Enquanto bloqueada, não
/// aparece mais na rotação do escalonador — ver design.md, "Achados durante
/// a implementação".
pub extern "C" fn consumer() -> ! {
    loop {
        QUEUE.block_current();
        WAKEUPS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Quantas vezes a consumidora já foi despertada até agora — ver módulo.
pub fn wakeups() -> u64 {
    WAKEUPS.load(Ordering::Relaxed)
}
