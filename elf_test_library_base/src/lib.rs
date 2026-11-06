//! Biblioteca compartilhada ELF64 (`ET_DYN`) de teste do carregador
//! dinâmico (`kernel::dylib`) — ver spec `kernel-dynamic-linker` e
//! `design.md` da change `kernel-dynamic-linker`, "Goals" (relocação
//! `R_X86_64_RELATIVE` intra-biblioteca).
//!
//! Exporta `base_add` (função simples) e `BASE_ADD_FN` (um `static` cujo
//! valor inicial é o endereço de `base_add`): como este código é PIC, o
//! endereço final de `base_add` só é conhecido em tempo de carregamento,
//! então o linker emite uma relocação `R_X86_64_RELATIVE` para preencher
//! `BASE_ADD_FN` — exercitada pelo carregador do kernel, não só pela
//! matemática pura já coberta em `cargo test -p kernel --lib`.

#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("nop") };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn base_add(a: i64, b: i64) -> i64 {
    a + b
}

/// Valor inicial é o endereço de [`base_add`] — como `base_add` é um
/// símbolo `GLOBAL` exportado (potencialmente interponível), o linker
/// emite uma relocação `R_X86_64_64` (resolvida por símbolo), não
/// `R_X86_64_RELATIVE` — ver [`INTERNAL_DOUBLE_FN`] para essa.
#[unsafe(no_mangle)]
pub static BASE_ADD_FN: extern "C" fn(i64, i64) -> i64 = base_add;

/// Função privada (sem `#[no_mangle]`, sem vinculação `GLOBAL`/sem entrada
/// em `.dynsym`) — referenciar seu endereço é sempre relativo à base de
/// carga desta biblioteca, nunca interponível por outra definição. Força a
/// relocação `R_X86_64_RELATIVE` que `BASE_ADD_FN` (acima) não produz — ver
/// design.md da change `kernel-dynamic-linker`, Goals.
extern "C" fn internal_double(x: i64) -> i64 {
    x * 2
}

#[unsafe(no_mangle)]
pub static INTERNAL_DOUBLE_FN: extern "C" fn(i64) -> i64 = internal_double;
