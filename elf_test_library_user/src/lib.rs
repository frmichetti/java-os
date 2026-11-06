//! Biblioteca compartilhada ELF64 (`ET_DYN`) de teste do carregador
//! dinâmico, dependente de `elf_test_library_base` via `DT_NEEDED` — ver
//! spec `kernel-dynamic-linker` e `design.md` da change
//! `kernel-dynamic-linker`, "Goals" (relocação `R_X86_64_GLOB_DAT`/
//! `R_X86_64_JUMP_SLOT` resolvida entre bibliotecas, e "Carregamento de
//! Dependências Declaradas...").
//!
//! Declara `base_add` como símbolo externo (sem corpo) e a chama a partir
//! de [`user_call_base_add`] — o linker registra `elf_test_library_base`
//! como `DT_NEEDED` e emite uma relocação de símbolo externo para
//! `base_add`, resolvida pelo carregador do kernel em tempo de carga.

#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("nop") };
    }
}

unsafe extern "C" {
    fn base_add(a: i64, b: i64) -> i64;
}

#[unsafe(no_mangle)]
pub extern "C" fn user_call_base_add(a: i64, b: i64) -> i64 {
    unsafe { base_add(a, b) }
}
