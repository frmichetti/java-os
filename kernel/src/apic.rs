//! Local APIC (modo x2APIC) e o timer do APIC — ver `design.md` da change
//! `kernel-interrupts-timer`. Esta change exige x2APIC; não há fallback
//! para xAPIC/MMIO (ver Decisões em design.md).

const CPUID_LEAF_FEATURES: u32 = 1;
const CPUID_ECX_X2APIC_BIT: u32 = 1 << 21;

/// Interpreta o bit de suporte a x2APIC em `ECX` da folha 1 do `CPUID`.
/// Função pura, testável sem hardware.
pub fn ecx_reports_x2apic_support(ecx: u32) -> bool {
    ecx & CPUID_ECX_X2APIC_BIT != 0
}

fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let eax_out: u32;
    let ebx_out: u32;
    let ecx_out: u32;
    let edx_out: u32;
    unsafe {
        core::arch::asm!(
            "mov {ebx_tmp:e}, ebx",
            "cpuid",
            "xchg {ebx_tmp:e}, ebx",
            ebx_tmp = out(reg) ebx_out,
            inout("eax") leaf => eax_out,
            inout("ecx") 0u32 => ecx_out,
            out("edx") edx_out,
            options(nostack, preserves_flags),
        );
    }
    (eax_out, ebx_out, ecx_out, edx_out)
}

/// `true` se a CPU atual suporta x2APIC.
pub fn supports_x2apic() -> bool {
    let (_, _, ecx, _) = cpuid(CPUID_LEAF_FEATURES);
    ecx_reports_x2apic_support(ecx)
}

pub(crate) unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
    }
    ((hi as u64) << 32) | lo as u64
}

pub(crate) unsafe fn wrmsr(msr: u32, value: u64) {
    let lo = value as u32;
    let hi = (value >> 32) as u32;
    unsafe {
        core::arch::asm!("wrmsr", in("ecx") msr, in("eax") lo, in("edx") hi, options(nomem, nostack, preserves_flags));
    }
}

const IA32_APIC_BASE_MSR: u32 = 0x1B;
const APIC_BASE_GLOBAL_ENABLE: u64 = 1 << 11;
const APIC_BASE_X2APIC_ENABLE: u64 = 1 << 10;

const MSR_X2APIC_SIVR: u32 = 0x80F;
const MSR_X2APIC_EOI: u32 = 0x80B;

pub(crate) const MSR_X2APIC_LVT_TIMER: u32 = 0x832;
pub(crate) const MSR_X2APIC_INITIAL_COUNT: u32 = 0x838;
pub(crate) const MSR_X2APIC_CURRENT_COUNT: u32 = 0x839;
pub(crate) const MSR_X2APIC_DIVIDE_CONFIG: u32 = 0x83E;

/// Vetor usado para a "spurious interrupt" do Local APIC — um vetor alto,
/// fora da faixa usada por exceções de CPU ou pelo timer.
pub const SPURIOUS_VECTOR: u8 = 0xFF;
const APIC_SOFTWARE_ENABLE: u64 = 1 << 8;

/// Habilita o Local APIC em modo x2APIC (via `IA32_APIC_BASE`) e configura o
/// Spurious Interrupt Vector Register. Desabilita o PIC 8259 legado antes,
/// para que nenhuma IRQ chegue por um canal que o kernel não trata mais.
///
/// # Safety
/// Deve ser chamada depois de confirmar [`supports_x2apic`] e depois de a
/// IDT já estar carregada.
pub unsafe fn init() {
    crate::pic::disable();

    unsafe {
        let base = rdmsr(IA32_APIC_BASE_MSR);
        wrmsr(IA32_APIC_BASE_MSR, base | APIC_BASE_GLOBAL_ENABLE | APIC_BASE_X2APIC_ENABLE);

        let sivr = (SPURIOUS_VECTOR as u64) | APIC_SOFTWARE_ENABLE;
        wrmsr(MSR_X2APIC_SIVR, sivr);
    }
}

/// Sinaliza o fim do tratamento da interrupção corrente ao Local APIC.
pub fn send_eoi() {
    unsafe { wrmsr(MSR_X2APIC_EOI, 0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_x2apic_bit_set() {
        assert!(ecx_reports_x2apic_support(1 << 21));
    }

    #[test]
    fn detects_x2apic_bit_absent() {
        assert!(!ecx_reports_x2apic_support(0));
    }

    #[test]
    fn ignores_unrelated_bits() {
        let ecx_with_other_features = 0xffff_ffff & !(1u32 << 21);
        assert!(!ecx_reports_x2apic_support(ecx_with_other_features));
    }
}
