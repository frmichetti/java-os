//! Driver mínimo de I/O APIC: roteamento de uma IRQ externa específica para
//! um vetor da IDT entregue ao Local APIC — ver `design.md` da change
//! `kernel-keyboard`.

use crate::paging::{MapError, Mapper, PageFlags};
use crate::phys::{FrameAllocator, FrameStore};

/// Endereço físico MMIO fixo do I/O APIC — o padrão de qualquer plataforma
/// PC-compatível (e o valor usado pelo I/O APIC emulado de QEMU,
/// VirtualBox e VMware, os únicos alvos atuais do projeto). Sem parsing de
/// ACPI/MADT — ver design.md, "I/O APIC: endereço MMIO fixo". Precisa ser
/// acessado através da HHDM (ver [`ioapic_base_virt`]): nenhuma página do
/// kernel mapeia esse endereço físico diretamente como virtual.
const IOAPIC_BASE_PHYS: u64 = 0xFEC0_0000;

/// Endereço virtual (via HHDM) onde o I/O APIC pode ser acessado. Achado
/// durante a implementação: escrever direto em `IOAPIC_BASE_PHYS` como se
/// fosse um endereço virtual causa um page fault (endereço não mapeado) —
/// ao contrário do framebuffer do Limine, que já vem como um ponteiro HHDM
/// pronto, o I/O APIC é um endereço físico "cru" que o próprio kernel
/// escolheu (ver design.md), e precisa da mesma tradução já usada pelo
/// alocador de frames físicos e pela paginação (`kernel::hhdm::phys_to_virt`).
fn ioapic_base_virt() -> usize {
    crate::hhdm::phys_to_virt(crate::hhdm::offset(), IOAPIC_BASE_PHYS) as usize
}

/// Mapeia a página MMIO do I/O APIC no endereço virtual da HHDM
/// correspondente a `IOAPIC_BASE_PHYS` — precisa ser chamada antes de
/// qualquer uso de [`route_irq`]. Achado durante a implementação: a HHDM do
/// Limine só garante cobrir memória usável/reclamável pelo bootloader, não
/// qualquer endereço físico arbitrário (ver design.md, "I/O APIC: endereço
/// MMIO fixo"); o kernel precisa mapear essa página ele mesmo, com o mesmo
/// `Mapper` já usado por `kernel::heap`/`kernel::stack`.
pub fn init<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<(), MapError> {
    let virt = ioapic_base_virt() as u64;
    Mapper::new(frame_allocator).map(virt, IOAPIC_BASE_PHYS, PageFlags::KERNEL_RW, false)
}

/// Offset do registrador de seleção (`IOREGSEL`): escrever aqui seleciona
/// qual registrador interno do I/O APIC a próxima leitura/escrita em
/// `IOWIN` vai acessar.
const IOREGSEL_OFFSET: usize = 0x00;
/// Offset da janela de dados (`IOWIN`): lê/escreve o registrador
/// selecionado em `IOREGSEL`.
const IOWIN_OFFSET: usize = 0x10;

/// Índice do primeiro registrador (dword baixo) da tabela de redireção —
/// cada IRQ ocupa dois registradores consecutivos (baixo, depois alto) a
/// partir daqui.
const REDIRECTION_TABLE_BASE: u32 = 0x10;

/// Monta os dois dwords (baixo, depois alto) de uma entrada da tabela de
/// redireção do I/O APIC para `vector`, com a interrupção mascarada
/// conforme `masked`, entregue em modo "Fixed"/destino físico/borda/
/// polaridade ativa-alta (os valores fixos decididos em design.md — todos
/// zero nos bits correspondentes) para o Local APIC `destination_apic_id`.
/// Função pura, testável sem hardware.
fn redirection_entry(vector: u8, masked: bool, destination_apic_id: u32) -> (u32, u32) {
    let mut low = vector as u32;
    if masked {
        low |= 1 << 16;
    }
    // Modo de entrega "Fixed" (bits 8-10 = 0), modo de destino físico (bit
    // 11 = 0), polaridade ativa-alta (bit 13 = 0), borda (bit 15 = 0) — já
    // satisfeitos por `low` não ter nenhum desses bits ligados.
    let high = (destination_apic_id & 0xff) << 24;
    (low, high)
}

/// Escreve `value` no registrador `index` do I/O APIC, via `IOREGSEL`/`IOWIN`.
///
/// # Safety
/// `IOAPIC_BASE_PHYS` deve apontar para um I/O APIC real mapeado nesse
/// endereço (ver design.md — hipótese assumida para as plataformas-alvo
/// atuais), e `kernel::hhdm::init` já deve ter sido chamada.
unsafe fn write_register(index: u32, value: u32) {
    unsafe {
        let base = ioapic_base_virt();
        let regsel = (base + IOREGSEL_OFFSET) as *mut u32;
        let iowin = (base + IOWIN_OFFSET) as *mut u32;
        regsel.write_volatile(index);
        iowin.write_volatile(value);
    }
}

/// Roteia a IRQ ISA `irq` para `vector`, entregue ao Local APIC
/// `destination_apic_id`, desmascarada — ver design.md, "Entrada de
/// redireção".
///
/// # Safety
/// Deve ser chamada depois de `apic::init()` (Local APIC já inicializado) e
/// com a IDT já carregada e o handler de `vector` já instalado — caso
/// contrário, a IRQ roteada chegaria a um vetor sem handler específico
/// (tratado pelo handler padrão, que só dá EOI sem processar o dispositivo).
pub unsafe fn route_irq(irq: u8, vector: u8, destination_apic_id: u32) {
    let (low, high) = redirection_entry(vector, false, destination_apic_id);
    let index = REDIRECTION_TABLE_BASE + 2 * irq as u32;
    unsafe {
        write_register(index + 1, high);
        write_register(index, low);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_the_configured_vector_in_the_low_dword() {
        let (low, _) = redirection_entry(0x21, false, 0);
        assert_eq!(low & 0xff, 0x21);
    }

    #[test]
    fn mask_bit_reflects_the_masked_parameter() {
        let (low_masked, _) = redirection_entry(0x21, true, 0);
        let (low_unmasked, _) = redirection_entry(0x21, false, 0);
        assert_ne!(low_masked & (1 << 16), 0, "bit de mascara deveria estar ligado");
        assert_eq!(low_unmasked & (1 << 16), 0, "bit de mascara deveria estar desligado");
    }

    #[test]
    fn delivery_mode_destination_mode_polarity_and_trigger_mode_are_the_fixed_decided_values() {
        let (low, _) = redirection_entry(0x21, false, 0);
        assert_eq!(low & (0b111 << 8), 0, "modo de entrega deveria ser Fixed (000)");
        assert_eq!(low & (1 << 11), 0, "modo de destino deveria ser fisico");
        assert_eq!(low & (1 << 13), 0, "polaridade deveria ser ativa-alta");
        assert_eq!(low & (1 << 15), 0, "modo de disparo deveria ser por borda");
    }

    #[test]
    fn destination_apic_id_appears_in_the_expected_bits_of_the_high_dword() {
        let (_, high) = redirection_entry(0x21, false, 0x05);
        assert_eq!(high, 0x05 << 24);
    }

    #[test]
    fn destination_apic_id_is_truncated_to_eight_bits() {
        let (_, high) = redirection_entry(0x21, false, 0x1_23);
        assert_eq!(high, 0x23 << 24, "deveria manter so o byte menos significativo do ID");
    }
}
