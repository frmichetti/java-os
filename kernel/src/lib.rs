#![cfg_attr(not(test), no_std)]
#![feature(abi_x86_interrupt)]

pub mod apic;
pub mod fmtbuf;
pub mod framebuffer;
pub mod gdt;
pub mod hal;
pub mod heap;
pub mod hhdm;
pub mod idt;
pub mod ioapic;
pub mod keyboard;
pub mod keyboard_demo;
pub mod memmap_report;
pub mod network;
pub mod network_demo;
pub mod paging;
pub mod pci;
pub mod phys;
pub mod pic;
pub mod port;
pub mod process;
pub mod scheduler;
pub mod serial;
pub mod stack;
pub mod storage;
pub mod storage_demo;
pub mod sync;
pub mod syscall;
pub mod timer;
pub mod user_demo;
pub mod vmm;
pub mod vmm_demo;

/// Halt definitivo de erro: interrupções desabilitadas, `hlt` em loop.
/// Usado a partir do `panic_handler`/`alloc_error_handler` e de qualquer
/// handler de exceção de CPU ou outro caminho de erro irrecuperável durante
/// o boot — interrupções ficam desabilitadas porque o estado do kernel pode
/// estar corrompido, e processar uma interrupção nesse ponto seria arriscado
/// (ver spec `kernel-boot`, "Comportamento Definido de Halt").
pub fn hlt_loop() -> ! {
    unsafe { core::arch::asm!("cli") };
    loop {
        unsafe { core::arch::asm!("hlt") }
    }
}

/// Halt ocioso de sucesso: usado ao final de uma sequência de boot
/// bem-sucedida, com interrupções habilitadas (satisfaz o mesmo requisito
/// de "loop de halt ocioso" do `kernel-boot` pela alternativa "thread
/// parada", já que aqui a CPU só acorda para tratar interrupções, como o
/// timer, e volta a dormir). Diferente de [`hlt_loop`], nunca desabilita
/// interrupções: a partir daqui, o kernel depende da IDT/APIC já estarem
/// configurados.
pub fn idle_loop() -> ! {
    unsafe { core::arch::asm!("sti") };
    loop {
        unsafe { core::arch::asm!("hlt") }
    }
}
