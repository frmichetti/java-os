//! Habilitação de FPU/SSE (`CR0`/`CR4`) e áreas de estado por thread
//! (`FXSAVE`/`FXRSTOR`) — ver `design.md` da change `kernel-fpu-context-switch`.
//!
//! O kernel em si permanece soft-float (`x86_64-java_os.json` inalterado,
//! nenhuma instrução SSE é emitida pelo compilador Rust sob esse target) —
//! este módulo só existe para que código de modo usuário (reabilitado em
//! `x86_64-java_os_user.json`) possa usar `float`/`double` de verdade sem
//! corromper esse estado na troca de contexto.

extern crate alloc;

use alloc::boxed::Box;

/// Área de estado de FPU/SSE no formato legado de `FXSAVE`/`FXRSTOR`: 512
/// bytes, alinhados a 16 — exigência da própria instrução, ao contrário dos
/// registradores de uso geral (que cabem em `push`/`pop` sem alinhamento
/// especial). Uma por thread, nunca liberada — mesma disciplina de "leak
/// aceitável por enquanto" já usada para `kernel::pthread_handles`/
/// `kernel::file_handles`.
#[repr(C, align(16))]
pub struct FxSaveArea(pub [u8; 512]);

/// MXCSR (word de controle de SSE) tem bits reservados que não podem ser
/// arbitrários — valor padrão com todas as exceções de ponto flutuante
/// mascaradas (Intel SDM, estado de reset do MXCSR).
const MXCSR_DEFAULT: u32 = 0x1F80;

/// Gera um estado de FPU/SSE inicial válido (`FNINIT` + `MXCSR` padrão +
/// `FXSAVE`) — nunca uma área zerada crua, que deixaria `MXCSR` num valor
/// potencialmente inválido. Função pura o bastante para ser chamada em
/// teste de host: `FNINIT`/`LDMXCSR`/`FXSAVE` não são instruções
/// privilegiadas (ao contrário do acesso a `CR0`/`CR4` em [`init`]).
fn generate_template() -> FxSaveArea {
    let mut area = FxSaveArea([0u8; 512]);
    unsafe {
        core::arch::asm!("fninit", options(nostack, nomem));
        let mxcsr: u32 = MXCSR_DEFAULT;
        core::arch::asm!("ldmxcsr [{0}]", in(reg) &mxcsr, options(readonly, nostack));
        core::arch::asm!("fxsave [{0}]", in(reg) &mut area as *mut FxSaveArea, options(nostack));
    }
    area
}

/// Template de estado inicial válido, gerado uma única vez por [`init`] e
/// copiado para a área de cada thread nova por [`new_area`]. `static mut`
/// simples (sem lock): escrito uma única vez, cedo no boot, antes de
/// qualquer outra thread poder chamar [`new_area`] — mesmo precedente de
/// `kernel::scheduler::CURRENT_KERNEL_STACK_TOP`.
static mut TEMPLATE: FxSaveArea = FxSaveArea([0u8; 512]);

/// Habilita `FXSAVE`/`FXRSTOR`/SSE a nível de `CR0`/`CR4` e gera o template
/// de estado inicial válido usado por [`new_area`].
///
/// # Safety
/// Deve ser chamada uma única vez, cedo no boot (antes de
/// `kernel::scheduler::register_boot_thread`, que é o primeiro ponto em que
/// uma área de FPU é alocada).
pub unsafe fn init() {
    unsafe {
        let mut cr0: u64;
        core::arch::asm!("mov {0}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        cr0 &= !(1u64 << 2); // EM = 0: desabilita emulação de FPU por software
        cr0 |= 1u64 << 1; // MP = 1: WAIT/FWAIT respeitam TS
        core::arch::asm!("mov cr0, {0}", in(reg) cr0, options(nostack, preserves_flags));

        let mut cr4: u64;
        core::arch::asm!("mov {0}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= (1u64 << 9) | (1u64 << 10); // OSFXSR, OSXMMEXCPT
        core::arch::asm!("mov cr4, {0}", in(reg) cr4, options(nostack, preserves_flags));

        TEMPLATE = generate_template();
    }
}

/// Aloca a área de FPU de uma nova thread, inicializada como cópia do
/// template gerado por [`init`] — nunca zerada crua. Alocada via
/// `Box::leak` (heap do kernel, acessível a partir de qualquer espaço de
/// endereçamento — vive na metade superior compartilhada), nunca liberada.
/// Devolve o endereço utilizável diretamente por `FXSAVE`/`FXRSTOR`.
pub fn new_area() -> u64 {
    let copy = FxSaveArea(unsafe { TEMPLATE.0 });
    let leaked: &'static mut FxSaveArea = Box::leak(Box::new(copy));
    leaked as *mut FxSaveArea as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_template_has_the_default_mxcsr_value() {
        /// Deslocamento, dentro da área de 512 bytes do formato legado de
        /// `FXSAVE`/`FXRSTOR`, do campo `MXCSR` (Intel SDM, Vol. 1, "FXSAVE
        /// Area") — só usado por este teste.
        const MXCSR_OFFSET: usize = 24;
        let area = generate_template();
        let mxcsr = u32::from_le_bytes(area.0[MXCSR_OFFSET..MXCSR_OFFSET + 4].try_into().unwrap());
        assert_eq!(mxcsr, MXCSR_DEFAULT);
    }

    #[test]
    fn fxsave_area_has_the_size_and_alignment_fxsave_requires() {
        assert_eq!(core::mem::size_of::<FxSaveArea>(), 512);
        assert_eq!(core::mem::align_of::<FxSaveArea>(), 16);
    }
}
