//! PIC 8259 legado — remapeado e completamente mascarado na inicialização,
//! já que o kernel usa exclusivamente o Local APIC a partir desta change
//! (ver design.md — "PIC 8259 legado desabilitado").

use crate::port::outb;

const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const ICW1_INIT_ICW4: u8 = 0x11;
const ICW4_8086_MODE: u8 = 0x01;

/// Vetores para onde o PIC é remapeado antes de ser mascarado — apenas para
/// não colidir com as exceções de CPU (vetores 0-31) caso alguma IRQ escape
/// antes da máscara ser aplicada.
const PIC1_VECTOR_OFFSET: u8 = 0x20;
const PIC2_VECTOR_OFFSET: u8 = 0x28;

const MASK_ALL_IRQS: u8 = 0xFF;

/// Remapeia o PIC 8259 (master + slave) e então mascara todas as suas IRQs.
pub fn disable() {
    unsafe {
        outb(PIC1_COMMAND, ICW1_INIT_ICW4);
        outb(PIC2_COMMAND, ICW1_INIT_ICW4);
        outb(PIC1_DATA, PIC1_VECTOR_OFFSET);
        outb(PIC2_DATA, PIC2_VECTOR_OFFSET);
        outb(PIC1_DATA, 0x04); // informa ao master que há um slave na IRQ2
        outb(PIC2_DATA, 0x02); // informa ao slave sua identidade de cascata
        outb(PIC1_DATA, ICW4_8086_MODE);
        outb(PIC2_DATA, ICW4_8086_MODE);

        outb(PIC1_DATA, MASK_ALL_IRQS);
        outb(PIC2_DATA, MASK_ALL_IRQS);
    }
}
