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

/// Porta de seleção e porta de dados do IMCR (Interrupt Mode Control
/// Register) — presente no chipset (não no próprio 8259), controla se as
/// IRQs ISA são eletricamente roteadas para o PIC ou para o I/O APIC.
const IMCR_SELECT_PORT: u16 = 0x22;
const IMCR_DATA_PORT: u16 = 0x23;
const IMCR_SELECT_REGISTER: u8 = 0x70;
/// Modo APIC: desconecta a saída INTR do PIC da CPU, entregando as IRQs
/// exclusivamente ao I/O APIC.
const IMCR_APIC_MODE: u8 = 0x01;

/// Remapeia o PIC 8259 (master + slave), mascara todas as suas IRQs, e
/// comuta o IMCR para modo APIC.
///
/// Achado durante a implementação da change `kernel-keyboard`: mascarar o
/// PIC (`MASK_ALL_IRQS` abaixo) não basta para que uma IRQ externa roteada
/// pelo I/O APIC (`kernel::ioapic`) seja entregue de forma confiável — sem
/// comutar o IMCR para modo APIC, a IRQ1 (teclado) era entregue corretamente
/// só na primeira vez, e nunca mais depois disso. O timer nunca expôs esse
/// problema porque é gerado internamente pelo próprio Local APIC (LVT),
/// sem depender do IMCR nem do I/O APIC — só agora, com o primeiro
/// dispositivo externo do kernel, o IMCR precisou ser tocado pela primeira
/// vez.
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

        outb(IMCR_SELECT_PORT, IMCR_SELECT_REGISTER);
        outb(IMCR_DATA_PORT, IMCR_APIC_MODE);
    }
}
