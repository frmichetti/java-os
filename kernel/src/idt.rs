//! IDT própria do kernel e handlers de exceção de CPU — ver `design.md` da
//! change `kernel-interrupts-timer`.

use crate::gdt::{DescriptorTablePointer, KERNEL_CODE_SELECTOR};

const GATE_TYPE_INTERRUPT: u8 = 0xE;

/// Uma entrada da IDT (gate descriptor), no formato de 16 bytes do x86_64.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    pub const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// `ist_index` é 0 para "nenhuma stack dedicada" (usa a stack corrente),
    /// ou 1-7 para usar a entrada correspondente de IST do TSS.
    pub fn new(handler_addr: u64, selector: u16, ist_index: u8, dpl: u8) -> Self {
        Self {
            offset_low: (handler_addr & 0xffff) as u16,
            selector,
            ist: ist_index & 0x7,
            type_attr: 0x80 | ((dpl & 0x3) << 5) | GATE_TYPE_INTERRUPT,
            offset_mid: ((handler_addr >> 16) & 0xffff) as u16,
            offset_high: ((handler_addr >> 32) & 0xffff_ffff) as u32,
            reserved: 0,
        }
    }

    pub fn offset(&self) -> u64 {
        (self.offset_low as u64) | ((self.offset_mid as u64) << 16) | ((self.offset_high as u64) << 32)
    }

    pub fn selector(&self) -> u16 {
        self.selector
    }

    pub fn ist(&self) -> u8 {
        self.ist & 0x7
    }

    pub fn is_present(&self) -> bool {
        self.type_attr & 0x80 != 0
    }
}

#[repr(C)]
pub struct Idt {
    entries: [IdtEntry; 256],
}

impl Idt {
    pub const fn new() -> Self {
        Self { entries: [IdtEntry::missing(); 256] }
    }

    pub fn set_handler(&mut self, vector: usize, handler_addr: u64, ist_index: u8) {
        self.entries[vector] = IdtEntry::new(handler_addr, KERNEL_CODE_SELECTOR, ist_index, 0);
    }
}

static mut IDT: Idt = Idt::new();

/// Instala um handler em um vetor específico da IDT estática do kernel.
/// Usado por outros módulos (ex.: `timer`) que precisam registrar seus
/// próprios handlers. Não entrega a IDT à CPU — isso é feito por [`load`].
pub fn set_handler(vector: usize, handler_addr: u64, ist_index: u8) {
    unsafe {
        (*core::ptr::addr_of_mut!(IDT)).set_handler(vector, handler_addr, ist_index);
    }
}

const VECTOR_DOUBLE_FAULT: usize = 8;
const VECTOR_GENERAL_PROTECTION_FAULT: usize = 13;
const VECTOR_PAGE_FAULT: usize = 14;

/// Frame que a CPU empilha ao entregar uma interrupção/exceção, na ordem em
/// que a ABI `extern "x86-interrupt"` do Rust espera recebê-lo.
#[repr(C)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

fn read_cr2() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Reporta uma exceção de CPU pelo canal de boot e entra em halt — cobre
/// "Tratamento de Exceções de CPU" da spec `kernel-interrupts`.
extern "x86-interrupt" fn page_fault_handler(frame: InterruptStackFrame, error_code: u64) {
    crate::serial_println_emergency!(
        "EXCECAO #PF (page fault): endereco=0x{:x} codigo_erro=0x{:x} rip=0x{:x}",
        read_cr2(),
        error_code,
        frame.instruction_pointer,
    );
    crate::hlt_loop();
}

extern "x86-interrupt" fn general_protection_fault_handler(frame: InterruptStackFrame, error_code: u64) {
    crate::serial_println_emergency!(
        "EXCECAO #GP (general protection fault): codigo_erro=0x{:x} rip=0x{:x}",
        error_code, frame.instruction_pointer,
    );
    crate::hlt_loop();
}

/// Usa a stack dedicada de IST1 (ver `gdt::DOUBLE_FAULT_IST_INDEX`), para
/// continuar funcionando mesmo que a causa do double fault seja um estouro
/// da stack corrente do kernel — cobre "Double Fault com Stack Dedicada".
extern "x86-interrupt" fn double_fault_handler(frame: InterruptStackFrame, error_code: u64) -> ! {
    crate::serial_println_emergency!(
        "EXCECAO #DF (double fault): codigo_erro=0x{:x} rip=0x{:x}",
        error_code, frame.instruction_pointer,
    );
    crate::hlt_loop();
}

/// Primeiro vetor de IRQ de hardware (depois das 32 exceções de CPU). O
/// handler padrão cobre esta faixa inteira, incluindo o vetor de "spurious
/// interrupt" do Local APIC (`apic::SPURIOUS_VECTOR`).
const FIRST_HARDWARE_VECTOR: usize = 32;

/// Reconhece qualquer interrupção de hardware que não tenha um handler mais
/// específico instalado, via EOI do Local APIC, e retorna — cobre
/// "Tratamento Padrão de Interrupções de Hardware" da spec `kernel-interrupts`.
/// Não precisa saber qual vetor foi entregue: o EOI do x2APIC não recebe
/// parâmetro de vetor.
extern "x86-interrupt" fn default_interrupt_handler(_frame: InterruptStackFrame) {
    crate::apic::send_eoi();
}

fn populate_exception_handlers(idt: &mut Idt) {
    idt.set_handler(
        VECTOR_DOUBLE_FAULT,
        double_fault_handler as *const () as u64,
        crate::gdt::DOUBLE_FAULT_IST_INDEX as u8,
    );
    idt.set_handler(VECTOR_GENERAL_PROTECTION_FAULT, general_protection_fault_handler as *const () as u64, 0);
    // IST dedicada (não 0): necessária para que a exceção continue sendo
    // entregue mesmo quando a stack corrente já estourou para dentro da
    // guard page do kernel — ver `design.md` da change
    // `kernel-stack-guard-page`, "Page fault também precisa de stack
    // dedicada (IST)".
    idt.set_handler(
        VECTOR_PAGE_FAULT,
        page_fault_handler as *const () as u64,
        crate::gdt::PAGE_FAULT_IST_INDEX as u8,
    );
}

fn populate_default_handlers(idt: &mut Idt) {
    for vector in FIRST_HARDWARE_VECTOR..256 {
        idt.set_handler(vector, default_interrupt_handler as *const () as u64, 0);
    }
}

/// Instala os handlers das exceções de CPU essenciais na IDT estática do
/// kernel. Não entrega a IDT à CPU — isso é feito por [`load`].
pub fn install_exception_handlers() {
    unsafe { populate_exception_handlers(&mut *core::ptr::addr_of_mut!(IDT)) };
}

/// Instala o handler padrão em todos os vetores de IRQ de hardware
/// (32-255). Deve ser chamada antes de instalar qualquer handler específico
/// de IRQ (ex.: o timer, na tarefa 4.1), já que `set_handler` sobrescreve.
pub fn install_default_handlers() {
    unsafe { populate_default_handlers(&mut *core::ptr::addr_of_mut!(IDT)) };
}

/// # Safety
/// Deve ser chamada depois de `gdt::init()` (os seletores/IST referenciados
/// pelas entradas da IDT dependem do GDT/TSS já estarem carregados).
pub unsafe fn load() {
    unsafe {
        let idt_ptr = DescriptorTablePointer {
            limit: (core::mem::size_of::<Idt>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u64,
        };
        core::arch::asm!("lidt [{0}]", in(reg) &idt_ptr, options(readonly, nostack, preserves_flags));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_entry_is_not_present() {
        assert!(!IdtEntry::missing().is_present());
    }

    #[test]
    fn new_entry_round_trips_handler_address() {
        let addr: u64 = 0xffff_ffff_8001_2345;
        let entry = IdtEntry::new(addr, KERNEL_CODE_SELECTOR, 0, 0);
        assert_eq!(entry.offset(), addr);
        assert_eq!(entry.selector(), KERNEL_CODE_SELECTOR);
        assert!(entry.is_present());
    }

    #[test]
    fn new_entry_encodes_ist_index() {
        let entry = IdtEntry::new(0x1000, KERNEL_CODE_SELECTOR, 1, 0);
        assert_eq!(entry.ist(), 1);
    }

    #[test]
    fn ist_index_is_masked_to_three_bits() {
        let entry = IdtEntry::new(0x1000, KERNEL_CODE_SELECTOR, 0xff, 0);
        assert_eq!(entry.ist(), 0x7);
    }

    #[test]
    fn exception_handlers_are_installed_at_correct_vectors_with_correct_ist() {
        let mut idt = Idt::new();
        populate_exception_handlers(&mut idt);

        assert!(idt.entries[VECTOR_DOUBLE_FAULT].is_present());
        assert_eq!(idt.entries[VECTOR_DOUBLE_FAULT].ist(), crate::gdt::DOUBLE_FAULT_IST_INDEX as u8);
        assert_eq!(idt.entries[VECTOR_DOUBLE_FAULT].offset(), double_fault_handler as *const () as u64);

        assert!(idt.entries[VECTOR_GENERAL_PROTECTION_FAULT].is_present());
        assert_eq!(idt.entries[VECTOR_GENERAL_PROTECTION_FAULT].ist(), 0);
        assert_eq!(
            idt.entries[VECTOR_GENERAL_PROTECTION_FAULT].offset(),
            general_protection_fault_handler as *const () as u64
        );

        assert!(idt.entries[VECTOR_PAGE_FAULT].is_present());
        assert_eq!(idt.entries[VECTOR_PAGE_FAULT].ist(), crate::gdt::PAGE_FAULT_IST_INDEX as u8);
        assert_eq!(idt.entries[VECTOR_PAGE_FAULT].offset(), page_fault_handler as *const () as u64);
    }

    #[test]
    fn default_handler_covers_the_full_hardware_vector_range() {
        let mut idt = Idt::new();
        populate_default_handlers(&mut idt);

        assert!(idt.entries[FIRST_HARDWARE_VECTOR].is_present());
        assert!(idt.entries[255].is_present());
        // Vetores de exceção de CPU não recebem o handler padrão.
        assert!(!idt.entries[0].is_present());
        assert!(!idt.entries[VECTOR_PAGE_FAULT].is_present());
    }
}
