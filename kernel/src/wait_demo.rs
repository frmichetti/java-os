//! Demonstração de verificação de `kernel::wait_queue` — análoga a
//! `keyboard_demo`/`storage_demo`/`network_demo`: uma thread produtora
//! desperta periodicamente uma thread consumidora bloqueada, que reporta
//! cada despertar pelo console serial — ver design.md da change
//! `kernel-wait-queue`.
//!
//! Antes da correção de `kernel::serial::_print` (ver `KNOW_ISSUES.md`,
//! "Duas threads de kernel imprimindo concorrentemente travam o timer de
//! hardware para sempre"), este demo evitava imprimir a cada despertar. Com
//! a correção (`_print`/`MirroredWriter` agora atômicos em relação a
//! interrupções), voltou a imprimir normalmente.

use crate::wait_queue::WaitQueue;

/// Fila de espera compartilhada entre as duas threads deste demo.
static QUEUE: WaitQueue = WaitQueue::new();

/// A cada quantos ticks do timer (100 Hz, ver `kernel::timer`) a produtora
/// desperta a consumidora — ~2s por despertar, frequência baixa o
/// suficiente para cada mensagem ser claramente distinguível no console
/// serial.
const WAKE_INTERVAL_TICKS: u64 = 200;

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

/// Thread consumidora: bloqueia em `QUEUE` em laço, reportando cada
/// despertar pelo console serial. Enquanto bloqueada, não aparece mais na
/// rotação do escalonador — ver design.md, "Achados durante a
/// implementação".
pub extern "C" fn consumer() -> ! {
    let mut wakeups: u64 = 0;
    loop {
        QUEUE.block_current();
        wakeups += 1;
        crate::serial_println!("wait_queue: consumidora despertou (contagem={})", wakeups);
    }
}
