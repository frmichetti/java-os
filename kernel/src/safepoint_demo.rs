//! Demonstração de verificação de `kernel::safepoint` — análoga a
//! `kernel::wait_demo`/`kernel::pthread_demo`: duas threads de kernel
//! compartilhando o espaço de endereçamento corrente. Uma thread
//! coordenadora reserva, compromete e registra uma página como ponto de poll
//! de safepoint, a descompromete (arma), aguarda uma janela de ticks e a
//! compromete de novo (libera); uma thread sondadora lê a página
//! continuamente até a coordenadora terminar, provando que nenhuma falta de
//! página dentro da janela armada derruba o kernel — ver design.md da change
//! `kernel-safepoint-signal`.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::paging::{PageFlags, PAGE_SIZE};

/// Quantos ticks do timer (100 Hz) a coordenadora mantém a página
/// descomprometida (armada) — ~1s, generoso o suficiente para a sondadora
/// experimentar várias faltas de página recuperáveis dentro da janela,
/// mesma ordem de grandeza de `kernel::wait_demo::WAKE_INTERVAL_TICKS`.
const ARMED_TICKS: u64 = 100;

/// Endereço da página de poll, publicado pela coordenadora para a sondadora
/// (`0` enquanto ainda não publicado).
static POLL_ADDR: AtomicU64 = AtomicU64::new(0);

/// `true` quando a coordenadora já recomprometeu a página (liberou o
/// safepoint) — sinal para a sondadora parar de contar e reportar o
/// resultado.
static RELEASED: AtomicBool = AtomicBool::new(false);

/// Quantas leituras a sondadora completou (bem-sucedidas ou recuperadas de
/// uma falta de página) desde que começou a sondar.
static POLL_COUNT: AtomicU64 = AtomicU64::new(0);

fn wait_ticks(count: u64) {
    let start = crate::timer::ticks();
    while crate::timer::ticks().wrapping_sub(start) < count {
        unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
    }
}

fn idle_forever() -> ! {
    loop {
        unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
    }
}

/// Thread coordenadora: reserva e compromete uma página, registra-a como
/// ponto de poll de safepoint, a descompromete (arma), aguarda
/// [`ARMED_TICKS`] e a compromete de novo (libera) — cobre o cenário "página
/// de poll de safepoint" ponta a ponta.
pub extern "C" fn coordinator() -> ! {
    let region = crate::scheduler::with_interrupts_disabled(|| {
        let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
        let mut vmm = crate::vmm::VMM.lock();
        let region = vmm.reserve(PAGE_SIZE).expect("reserva da pagina de safepoint falhou");
        vmm.commit(&mut frame_allocator, &region, 0, PAGE_SIZE, PageFlags::KERNEL_RW)
            .expect("comprometimento inicial da pagina de safepoint falhou");
        region
    });

    crate::safepoint::register(region.base(), region.size()).expect("registro de safepoint falhou");
    crate::serial_println!("safepoint: pagina de poll registrada em 0x{:x}", region.base());
    POLL_ADDR.store(region.base(), Ordering::Release);

    // Dá tempo da sondadora publicar sua primeira leitura antes de armar.
    wait_ticks(5);

    crate::scheduler::with_interrupts_disabled(|| {
        let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
        let mut vmm = crate::vmm::VMM.lock();
        vmm.decommit(&mut frame_allocator, &region, 0, PAGE_SIZE).expect("descomprometimento (armar) falhou");
    });
    crate::serial_println!("safepoint: armado (pagina descomprometida)");

    wait_ticks(ARMED_TICKS);

    crate::scheduler::with_interrupts_disabled(|| {
        let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
        let mut vmm = crate::vmm::VMM.lock();
        vmm.commit(&mut frame_allocator, &region, 0, PAGE_SIZE, PageFlags::KERNEL_RW)
            .expect("comprometimento (liberar) falhou");
    });
    crate::serial_println!("safepoint: liberado (pagina comprometida de novo)");
    RELEASED.store(true, Ordering::Release);

    idle_forever();
}

/// Thread sondadora: lê repetidamente a página de poll (via
/// `read_volatile`) até a coordenadora liberar o safepoint, contando cada
/// leitura completada — se a leitura nunca retomasse automaticamente (ou se
/// o kernel haltasse por causa de uma falta de página fatal), esta thread
/// nunca chegaria a reportar o resultado.
pub extern "C" fn poller() -> ! {
    let mut addr = POLL_ADDR.load(Ordering::Acquire);
    while addr == 0 {
        unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
        addr = POLL_ADDR.load(Ordering::Acquire);
    }
    let ptr = addr as *const u8;

    while !RELEASED.load(Ordering::Acquire) {
        unsafe { core::ptr::read_volatile(ptr) };
        POLL_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    // Confirma que a leitura continua funcionando normalmente depois da
    // liberação (nenhuma falta de página deveria mais ocorrer aqui).
    for _ in 0..1000 {
        unsafe { core::ptr::read_volatile(ptr) };
        POLL_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    crate::serial_println!(
        "safepoint: {} leituras completadas sem halt (incluindo durante a janela armada) - PASSOU",
        POLL_COUNT.load(Ordering::Relaxed)
    );

    idle_forever();
}
