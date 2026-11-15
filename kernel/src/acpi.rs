//! Localização e interpretação das tabelas ACPI entregues pelo bootloader
//! (RSDP, RSDT/XSDT, MADT) — ver spec `kernel-acpi` e `design.md` da change
//! `kernel-acpi-madt`. Descobre a topologia real de interrupção do hardware
//! (endereço do I/O APIC, substituições de IRQ legada) em vez de assumir os
//! valores fixos corretos só para QEMU/VirtualBox/VMware.
//!
//! Dividido no mesmo padrão de `kernel::heap`/`kernel::paging`: lógica pura
//! de validação/decodificação (testável em host, com bytes construídos à
//! mão), separada da orquestração que lê memória física real através do
//! RSDP entregue pelo bootloader (só verificável em boot real).

extern crate alloc;

use core::fmt::Write as _;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::sync::Spinlock;

/// Soma de todos os bytes, módulo 256, deve dar zero — a regra de
/// integridade que toda tabela ACPI (RSDP, RSDT/XSDT, MADT) usa. Função
/// pura, testável sem hardware.
fn checksum_is_valid(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b)) == 0
}

/// Tamanho do cabeçalho genérico de tabela ACPI (`SDTHeader`): assinatura
/// de 4 bytes, comprimento total (`u32`), e os demais campos (revisão,
/// soma de verificação, OEM ID/Table ID/Revision, Creator ID/Revision) até
/// completar 36 bytes.
const SDT_HEADER_LEN: usize = 36;

/// Cabeçalho genérico de uma tabela ACPI já lida (RSDT/XSDT/MADT
/// compartilham o mesmo formato de cabeçalho) — vista sobre um `&[u8]` já
/// em mãos, nunca lê memória por conta própria. Função pura, testável sem
/// hardware.
struct SdtHeader<'a> {
    bytes: &'a [u8],
}

impl<'a> SdtHeader<'a> {
    /// `bytes` deve conter ao menos o cabeçalho (36 bytes) — o comprimento
    /// total declarado no cabeçalho é então usado para delimitar a tabela
    /// inteira dentro de `bytes`, que pode conter mais bytes do que a
    /// tabela (ex.: um buffer de tamanho máximo lido de forma conservadora).
    /// Devolve `None` se `bytes` for menor que o cabeçalho, ou menor que o
    /// comprimento declarado.
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < SDT_HEADER_LEN {
            return None;
        }
        let declared_len = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
        if declared_len < SDT_HEADER_LEN || bytes.len() < declared_len {
            return None;
        }
        Some(Self { bytes: &bytes[..declared_len] })
    }

    /// Lê só o campo de comprimento total (bytes 4..8) de um cabeçalho —
    /// exige apenas os primeiros 8 bytes, não a tabela inteira. Usada para
    /// descobrir quanto reler antes de validar a tabela inteira via
    /// [`SdtHeader::parse`] (que, ao contrário desta função, exige que
    /// `bytes` já contenha a tabela inteira — uma leitura só do cabeçalho,
    /// mais curta que o comprimento real declarado, nunca passaria nessa
    /// checagem). Função pura, testável sem hardware.
    fn peek_declared_length(bytes: &[u8]) -> Option<usize> {
        if bytes.len() < 8 {
            return None;
        }
        Some(u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize)
    }

    fn signature(&self) -> &'a [u8] {
        &self.bytes[0..4]
    }

    /// A tabela inteira (cabeçalho incluído) — a soma de verificação ACPI
    /// cobre todos os bytes, não só o cabeçalho.
    fn whole_table(&self) -> &'a [u8] {
        self.bytes
    }

    /// Os bytes específicos desta tabela, depois do cabeçalho genérico de
    /// 36 bytes.
    fn body(&self) -> &'a [u8] {
        &self.bytes[SDT_HEADER_LEN..]
    }
}

// ---------------------------------------------------------------------
// RSDP (Root System Description Pointer).
// ---------------------------------------------------------------------

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";
const RSDP_V1_LEN: usize = 20;
const RSDP_V2_LEN: usize = 36;

/// Valida a estrutura RSDP de revisão 0/1 (os primeiros 20 bytes, sempre
/// presentes mesmo em ACPI 1.0) e devolve `(revisão, endereço físico da
/// RSDT)`. Função pura, testável sem hardware — `bytes` já deve ter sido
/// lido de algum lugar (boot real: via o ponteiro entregue pelo
/// bootloader).
fn parse_rsdp_v1(bytes: &[u8]) -> Option<(u8, u32)> {
    if bytes.len() < RSDP_V1_LEN {
        return None;
    }
    if &bytes[0..8] != RSDP_SIGNATURE {
        return None;
    }
    if !checksum_is_valid(&bytes[0..RSDP_V1_LEN]) {
        return None;
    }
    let revision = bytes[15];
    let rsdt_address = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    Some((revision, rsdt_address))
}

/// Valida a extensão ACPI 2.0+ da estrutura RSDP (os bytes 20..36,
/// presentes só quando a revisão validada por [`parse_rsdp_v1`] for >= 2) e
/// devolve o endereço físico da XSDT, ou `None` se o campo for zero (sem
/// XSDT disponível, mesmo em ACPI 2.0+) ou a soma de verificação estendida
/// (bytes 0..36) não bater. Função pura, testável sem hardware.
fn parse_rsdp_v2_extension(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < RSDP_V2_LEN {
        return None;
    }
    if !checksum_is_valid(&bytes[0..RSDP_V2_LEN]) {
        return None;
    }
    let xsdt_address = u64::from_le_bytes(bytes[24..32].try_into().ok()?);
    if xsdt_address == 0 {
        return None;
    }
    Some(xsdt_address)
}

// ---------------------------------------------------------------------
// Entradas da MADT (`Multiple APIC Description Table`, assinatura `"APIC"`).
// ---------------------------------------------------------------------

const MADT_SIGNATURE: &[u8; 4] = b"APIC";

/// Tipo de entrada "I/O APIC" dentro da MADT.
const MADT_ENTRY_TYPE_IOAPIC: u8 = 1;
/// Tipo de entrada "Interrupt Source Override" dentro da MADT.
const MADT_ENTRY_TYPE_INTERRUPT_SOURCE_OVERRIDE: u8 = 2;

/// Decodifica uma entrada de tipo 1 (I/O APIC) da MADT — `entry` é os bytes
/// da entrada inteira, começando no byte de tipo. Devolve o endereço MMIO
/// físico declarado. Função pura, testável sem hardware.
fn decode_ioapic_entry(entry: &[u8]) -> Option<u32> {
    if entry.len() < 12 || entry[0] != MADT_ENTRY_TYPE_IOAPIC {
        return None;
    }
    Some(u32::from_le_bytes(entry[4..8].try_into().ok()?))
}

/// Decodifica uma entrada de tipo 2 (substituição de origem de interrupção)
/// da MADT — `entry` é os bytes da entrada inteira, começando no byte de
/// tipo. Devolve `(IRQ ISA de origem, GSI de destino)`. Função pura,
/// testável sem hardware.
fn decode_interrupt_source_override_entry(entry: &[u8]) -> Option<(u8, u32)> {
    if entry.len() < 10 || entry[0] != MADT_ENTRY_TYPE_INTERRUPT_SOURCE_OVERRIDE {
        return None;
    }
    let source_irq = entry[3];
    let gsi = u32::from_le_bytes(entry[4..8].try_into().ok()?);
    Some((source_irq, gsi))
}

/// Resultado de percorrer todas as entradas de uma MADT válida: o endereço
/// MMIO físico da primeira entrada de I/O APIC encontrada (`None` se
/// nenhuma existir), e o GSI de destino de cada substituição de origem de
/// interrupção declarada, indexado pela IRQ ISA de origem (`None` nessa
/// posição = nenhuma substituição declarada para essa IRQ).
struct MadtEntries {
    ioapic_address: Option<u32>,
    isa_irq_overrides: [Option<u32>; 16],
}

/// Percorre as entradas de uma MADT já validada (`body` = os bytes da MADT
/// depois do cabeçalho de 36 bytes **e** dos 8 bytes específicos da MADT —
/// endereço do Local APIC + *flags* —, ou seja, só a lista de entradas de
/// tamanho variável) extraindo o endereço do primeiro I/O APIC e todas as
/// substituições de origem de interrupção. Uma entrada de tipo ou
/// comprimento desconhecido/inválido é ignorada, não um erro — o laço
/// avança pelo comprimento declarado de cada entrada, nunca assume um
/// tamanho fixo. Função pura, testável sem hardware.
fn walk_madt_entries(body: &[u8]) -> MadtEntries {
    let mut ioapic_address = None;
    let mut isa_irq_overrides = [None; 16];

    let mut offset = 0;
    while offset + 2 <= body.len() {
        let entry_type = body[offset];
        let entry_len = body[offset + 1] as usize;
        if entry_len < 2 || offset + entry_len > body.len() {
            // Comprimento inválido/inconsistente: para de confiar no
            // restante da tabela, em vez de arriscar um laço infinito ou
            // ler fora dos limites.
            break;
        }
        let entry = &body[offset..offset + entry_len];

        match entry_type {
            MADT_ENTRY_TYPE_IOAPIC => {
                if ioapic_address.is_none() {
                    ioapic_address = decode_ioapic_entry(entry);
                }
            }
            MADT_ENTRY_TYPE_INTERRUPT_SOURCE_OVERRIDE => {
                if let Some((source_irq, gsi)) = decode_interrupt_source_override_entry(entry) {
                    if let Some(slot) = isa_irq_overrides.get_mut(source_irq as usize) {
                        *slot = Some(gsi);
                    }
                }
            }
            _ => {}
        }

        offset += entry_len;
    }

    MadtEntries { ioapic_address, isa_irq_overrides }
}

// ---------------------------------------------------------------------
// Estado global e API pública.
// ---------------------------------------------------------------------

/// `0` = nenhum I/O APIC descoberto via ACPI (nenhum endereço MMIO real é
/// zero).
static IOAPIC_ADDRESS: AtomicU64 = AtomicU64::new(0);
static ISA_IRQ_OVERRIDES: Spinlock<[Option<u32>; 16]> = Spinlock::new([None; 16]);

/// Tamanho máximo de tabela ACPI que esta implementação lê — generoso o
/// bastante para qualquer MADT real (algumas dezenas de entradas), sem
/// arriscar uma leitura absurda a partir de um campo de comprimento
/// corrompido.
const MAX_TABLE_LEN: usize = 64 * 1024;

/// Lê `max_len` bytes a partir do endereço físico `phys_addr`, através da
/// HHDM — usada para ler o cabeçalho (para descobrir o comprimento real) e
/// depois a tabela inteira. Só verificável em boot real (lê memória física
/// real) — mesma categoria de `Mapper::map`/`AddressSpace::new`.
///
/// # Safety
/// `phys_addr` deve apontar para memória realmente mapeada pela HHDM, e
/// `kernel::hhdm::init` já deve ter sido chamada.
unsafe fn read_phys_bytes(phys_addr: u64, max_len: usize) -> &'static [u8] {
    let virt = crate::hhdm::phys_to_virt(crate::hhdm::offset(), phys_addr) as *const u8;
    unsafe { core::slice::from_raw_parts(virt, max_len) }
}

/// Lê e valida a tabela ACPI em `phys_addr`: lê o cabeçalho primeiro para
/// descobrir o comprimento real declarado, depois relê exatamente esse
/// tanto e confirma a soma de verificação sobre a tabela inteira. Devolve
/// os bytes validados da tabela inteira, ou `None` se o cabeçalho for
/// inválido ou a soma de verificação não bater. Só verificável em boot
/// real.
///
/// # Safety
/// Mesma exigência de [`read_phys_bytes`].
unsafe fn read_validated_table(phys_addr: u64) -> Option<&'static [u8]> {
    let probe = unsafe { read_phys_bytes(phys_addr, SDT_HEADER_LEN) };
    let declared_len = SdtHeader::peek_declared_length(probe)?.min(MAX_TABLE_LEN);
    let full = unsafe { read_phys_bytes(phys_addr, declared_len) };
    let header = SdtHeader::parse(full)?;
    if !checksum_is_valid(header.whole_table()) {
        return None;
    }
    Some(header.whole_table())
}

/// Localiza a MADT a partir de uma RSDT/XSDT já validada: percorre suas
/// entradas (ponteiros físicos de 32 bits na RSDT, de 64 bits na XSDT),
/// lendo e validando cada tabela apontada até encontrar a assinatura
/// `"APIC"`. Só verificável em boot real.
///
/// # Safety
/// Mesma exigência de [`read_phys_bytes`].
unsafe fn find_madt(sdt_body: &[u8], entries_are_64_bit: bool) -> Option<&'static [u8]> {
    let entry_size = if entries_are_64_bit { 8 } else { 4 };
    for entry in sdt_body.chunks_exact(entry_size) {
        let table_phys_addr =
            if entries_are_64_bit { u64::from_le_bytes(entry.try_into().ok()?) } else { u32::from_le_bytes(entry.try_into().ok()?) as u64 };
        if let Some(table) = unsafe { read_validated_table(table_phys_addr) } {
            if table.get(0..4) == Some(MADT_SIGNATURE.as_slice()) {
                return Some(table);
            }
        }
    }
    None
}

/// Localiza e interpreta as tabelas ACPI entregues pelo bootloader, a
/// partir do ponteiro RSDP (já virtual, pronto para uso direto — ver
/// design.md, "RSDP já vem como ponteiro virtual") — `None` se o
/// bootloader não entregou nenhum. Popula o estado global consultado por
/// [`ioapic_address`]/[`resolve_isa_irq_gsi`]. Nunca entra em pânico nem
/// trava em nenhuma falha de validação — só reporta um diagnóstico e deixa
/// o estado global refletindo "nada descoberto". Deve ser chamada uma
/// única vez, depois de `kernel::hhdm::init`, antes de
/// `kernel::apic::init`/`kernel::ioapic::init`/`kernel::keyboard::init`.
///
/// # Safety
/// `rsdp_virt_ptr`, se `Some`, deve ser o ponteiro RSDP de verdade entregue
/// pelo bootloader (já virtual — ver design.md), e `kernel::hhdm::init` já
/// deve ter sido chamada.
pub unsafe fn init(rsdp_virt_ptr: Option<*const u8>) {
    let Some(rsdp_ptr) = rsdp_virt_ptr else {
        crate::serial_println!("acpi: RSDP nao fornecido pelo bootloader - topologia via ACPI indisponivel");
        return;
    };

    let v1_bytes = unsafe { core::slice::from_raw_parts(rsdp_ptr, RSDP_V1_LEN) };
    let Some((revision, rsdt_address)) = parse_rsdp_v1(v1_bytes) else {
        crate::serial_println!(
            "acpi: RSDP invalido (assinatura ou soma de verificacao) - topologia via ACPI indisponivel"
        );
        return;
    };

    let xsdt_address = if revision >= 2 {
        let v2_bytes = unsafe { core::slice::from_raw_parts(rsdp_ptr, RSDP_V2_LEN) };
        parse_rsdp_v2_extension(v2_bytes)
    } else {
        None
    };

    let (sdt_phys_addr, entries_are_64_bit) =
        if let Some(xsdt) = xsdt_address { (xsdt, true) } else { (rsdt_address as u64, false) };

    let Some(sdt_table) = (unsafe { read_validated_table(sdt_phys_addr) }) else {
        crate::serial_println!("acpi: RSDT/XSDT ausente ou invalida - topologia via ACPI indisponivel");
        return;
    };
    let sdt_header = SdtHeader { bytes: sdt_table };

    let Some(madt) = (unsafe { find_madt(sdt_header.body(), entries_are_64_bit) }) else {
        crate::serial_println!("acpi: MADT nao encontrada ou invalida - topologia via ACPI indisponivel");
        return;
    };

    // Corpo específico da MADT: 36 bytes de cabeçalho genérico + 8 bytes
    // (endereço do Local APIC + flags, não usados aqui — ver design.md,
    // Non-Goals) antes da lista de entradas de tamanho variável.
    let Some(entries_body) = madt.get(SDT_HEADER_LEN + 8..) else {
        crate::serial_println!("acpi: MADT menor que o esperado - topologia via ACPI indisponivel");
        return;
    };
    let parsed = walk_madt_entries(entries_body);

    if let Some(address) = parsed.ioapic_address {
        IOAPIC_ADDRESS.store(address as u64, Ordering::Relaxed);
    }
    *ISA_IRQ_OVERRIDES.lock() = parsed.isa_irq_overrides;

    // `StackBuf`, não `alloc::format!`: `kernel::acpi::init` roda antes do
    // heap estar pronto (`kernel::heap::init`, bem mais adiante no boot) —
    // achado real durante a implementação, ver tasks.md.
    let override_count = parsed.isa_irq_overrides.iter().filter(|o| o.is_some()).count();
    let mut ioapic_desc = crate::fmtbuf::StackBuf::<32>::new();
    match parsed.ioapic_address {
        Some(addr) => {
            let _ = write!(ioapic_desc, "em 0x{addr:x}");
        }
        None => {
            let _ = write!(ioapic_desc, "nao declarado");
        }
    }
    crate::serial_println!(
        "acpi: MADT valida - I/O APIC {} - {} substituicao(oes) de IRQ legada",
        ioapic_desc.as_str(),
        override_count
    );
}

/// Endereço MMIO físico do I/O APIC descoberto via ACPI/MADT, ou `None` se
/// nenhum foi descoberto (ACPI ausente/inválido, ou MADT sem entrada de I/O
/// APIC) — nesse caso, o chamador (`kernel::ioapic::init`) usa seu próprio
/// endereço de contingência.
pub fn ioapic_address() -> Option<u64> {
    let value = IOAPIC_ADDRESS.load(Ordering::Relaxed);
    if value == 0 {
        None
    } else {
        Some(value)
    }
}

/// `GSI` (`Global System Interrupt`) real ao qual a IRQ ISA legada `irq`
/// (0-15) está conectada — o `GSI` declarado por uma substituição de
/// origem de interrupção da MADT, se existir, ou o próprio número da IRQ
/// caso contrário (identidade — mesma semântica do próprio padrão ACPI,
/// não uma contingência do kernel). Nunca `Option`: `irq >= 16` (fora da
/// faixa ISA) também devolve identidade, já que não há como ter uma
/// substituição para ela.
pub fn resolve_isa_irq_gsi(irq: u8) -> u32 {
    if let Some(&Some(gsi)) = ISA_IRQ_OVERRIDES.lock().get(irq as usize) {
        return gsi;
    }
    irq as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_valid_accepts_a_buffer_whose_bytes_sum_to_zero_mod_256() {
        // 0x01 + 0xff = 0x100, que módulo 256 é zero.
        assert!(checksum_is_valid(&[0x01, 0xff]));
    }

    #[test]
    fn checksum_is_valid_rejects_a_buffer_whose_bytes_do_not_sum_to_zero() {
        assert!(!checksum_is_valid(&[0x01, 0x02]));
    }

    #[test]
    fn checksum_is_valid_accepts_an_empty_buffer() {
        assert!(checksum_is_valid(&[]));
    }

    fn make_sdt_header(signature: &[u8; 4], total_len: usize) -> alloc::vec::Vec<u8> {
        let mut bytes = alloc::vec![0u8; total_len];
        bytes[0..4].copy_from_slice(signature);
        bytes[4..8].copy_from_slice(&(total_len as u32).to_le_bytes());
        bytes
    }

    #[test]
    fn sdt_header_parses_signature_and_declared_length() {
        let bytes = make_sdt_header(b"TEST", 40);
        let header = SdtHeader::parse(&bytes).expect("cabecalho valido deveria ser aceito");
        assert_eq!(header.signature(), b"TEST");
        assert_eq!(header.whole_table().len(), 40);
    }

    #[test]
    fn sdt_header_rejects_a_buffer_smaller_than_the_header() {
        assert!(SdtHeader::parse(&[0u8; 10]).is_none());
    }

    #[test]
    fn peek_declared_length_reads_the_length_field_even_from_a_short_probe_buffer() {
        // Achado real: uma leitura "de espreita" (só os primeiros 36 bytes,
        // para descobrir o comprimento real antes de reler a tabela
        // inteira) é mais curta que o comprimento declarado em qualquer
        // tabela real não-trivial (ex.: uma XSDT de 84 bytes) —
        // `peek_declared_length` precisa aceitar isso, ao contrário de
        // `SdtHeader::parse` (que exige a tabela inteira já em mãos).
        let full_table = make_sdt_header(b"XSDT", 84);
        let probe = &full_table[..SDT_HEADER_LEN];
        assert_eq!(SdtHeader::peek_declared_length(probe), Some(84));
        // Confirma a própria causa do bug original: `SdtHeader::parse`
        // sobre esse mesmo probe rejeita, porque 36 < 84.
        assert!(SdtHeader::parse(probe).is_none());
    }

    #[test]
    fn peek_declared_length_rejects_a_buffer_shorter_than_the_length_field_itself() {
        assert!(SdtHeader::peek_declared_length(&[0u8; 6]).is_none());
    }

    #[test]
    fn sdt_header_rejects_a_declared_length_longer_than_the_buffer() {
        let mut bytes = make_sdt_header(b"TEST", 40);
        bytes.truncate(30); // buffer físico menor que o comprimento declarado
        assert!(SdtHeader::parse(&bytes).is_none());
    }

    #[test]
    fn sdt_header_ignores_extra_bytes_beyond_the_declared_length() {
        let mut bytes = make_sdt_header(b"TEST", 40);
        bytes.extend_from_slice(&[0xaa; 10]); // buffer maior que o necessário
        let header = SdtHeader::parse(&bytes).expect("deveria aceitar, ignorando os bytes extras");
        assert_eq!(header.whole_table().len(), 40);
    }

    fn make_valid_rsdp_v1(revision: u8, rsdt_address: u32) -> alloc::vec::Vec<u8> {
        let mut bytes = alloc::vec![0u8; RSDP_V1_LEN];
        bytes[0..8].copy_from_slice(RSDP_SIGNATURE);
        bytes[15] = revision;
        bytes[16..20].copy_from_slice(&rsdt_address.to_le_bytes());
        let sum: u8 = bytes[..RSDP_V1_LEN].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        bytes[8] = 0u8.wrapping_sub(sum);
        bytes
    }

    #[test]
    fn parse_rsdp_v1_accepts_a_valid_structure() {
        let bytes = make_valid_rsdp_v1(0, 0x1234_5000);
        let (revision, rsdt_address) = parse_rsdp_v1(&bytes).expect("RSDP valido deveria ser aceito");
        assert_eq!(revision, 0);
        assert_eq!(rsdt_address, 0x1234_5000);
    }

    #[test]
    fn parse_rsdp_v1_rejects_wrong_signature() {
        let mut bytes = make_valid_rsdp_v1(0, 0x1000);
        bytes[0] = b'X';
        assert!(parse_rsdp_v1(&bytes).is_none());
    }

    #[test]
    fn parse_rsdp_v1_rejects_bad_checksum() {
        let mut bytes = make_valid_rsdp_v1(0, 0x1000);
        bytes[8] = bytes[8].wrapping_add(1);
        assert!(parse_rsdp_v1(&bytes).is_none());
    }

    fn make_valid_rsdp_v2(rsdt_address: u32, xsdt_address: u64) -> alloc::vec::Vec<u8> {
        let mut bytes = make_valid_rsdp_v1(2, rsdt_address);
        bytes.resize(RSDP_V2_LEN, 0);
        bytes[20..24].copy_from_slice(&(RSDP_V2_LEN as u32).to_le_bytes());
        bytes[24..32].copy_from_slice(&xsdt_address.to_le_bytes());
        let sum: u8 = bytes[..RSDP_V2_LEN].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        bytes[32] = 0u8.wrapping_sub(sum);
        bytes
    }

    #[test]
    fn parse_rsdp_v2_extension_accepts_a_valid_structure() {
        let bytes = make_valid_rsdp_v2(0x1000, 0xdead_beef_0000);
        let xsdt_address = parse_rsdp_v2_extension(&bytes).expect("extensao v2 valida deveria ser aceita");
        assert_eq!(xsdt_address, 0xdead_beef_0000);
    }

    #[test]
    fn parse_rsdp_v2_extension_rejects_zero_address_as_unavailable() {
        let bytes = make_valid_rsdp_v2(0x1000, 0);
        assert!(parse_rsdp_v2_extension(&bytes).is_none());
    }

    #[test]
    fn parse_rsdp_v2_extension_rejects_bad_extended_checksum() {
        let mut bytes = make_valid_rsdp_v2(0x1000, 0xabc);
        bytes[32] = bytes[32].wrapping_add(1);
        assert!(parse_rsdp_v2_extension(&bytes).is_none());
    }

    fn make_ioapic_entry(id: u8, address: u32, gsi_base: u32) -> alloc::vec::Vec<u8> {
        let mut entry = alloc::vec![0u8; 12];
        entry[0] = MADT_ENTRY_TYPE_IOAPIC;
        entry[1] = 12;
        entry[2] = id;
        entry[4..8].copy_from_slice(&address.to_le_bytes());
        entry[8..12].copy_from_slice(&gsi_base.to_le_bytes());
        entry
    }

    #[test]
    fn decode_ioapic_entry_extracts_the_mmio_address() {
        let entry = make_ioapic_entry(0, 0xfec0_0000, 0);
        assert_eq!(decode_ioapic_entry(&entry), Some(0xfec0_0000));
    }

    #[test]
    fn decode_ioapic_entry_rejects_wrong_type() {
        let mut entry = make_ioapic_entry(0, 0xfec0_0000, 0);
        entry[0] = MADT_ENTRY_TYPE_INTERRUPT_SOURCE_OVERRIDE;
        assert_eq!(decode_ioapic_entry(&entry), None);
    }

    #[test]
    fn decode_ioapic_entry_rejects_truncated_entry() {
        let entry = make_ioapic_entry(0, 0xfec0_0000, 0);
        assert_eq!(decode_ioapic_entry(&entry[..8]), None);
    }

    fn make_interrupt_source_override_entry(source_irq: u8, gsi: u32) -> alloc::vec::Vec<u8> {
        let mut entry = alloc::vec![0u8; 10];
        entry[0] = MADT_ENTRY_TYPE_INTERRUPT_SOURCE_OVERRIDE;
        entry[1] = 10;
        entry[3] = source_irq;
        entry[4..8].copy_from_slice(&gsi.to_le_bytes());
        entry
    }

    #[test]
    fn decode_interrupt_source_override_entry_extracts_irq_and_gsi() {
        let entry = make_interrupt_source_override_entry(1, 9);
        assert_eq!(decode_interrupt_source_override_entry(&entry), Some((1, 9)));
    }

    #[test]
    fn decode_interrupt_source_override_entry_rejects_wrong_type() {
        let mut entry = make_interrupt_source_override_entry(1, 9);
        entry[0] = MADT_ENTRY_TYPE_IOAPIC;
        assert_eq!(decode_interrupt_source_override_entry(&entry), None);
    }

    #[test]
    fn walk_madt_entries_finds_ioapic_and_overrides_among_mixed_entries() {
        let mut body = alloc::vec::Vec::new();
        body.extend(make_ioapic_entry(0, 0xfec0_0000, 0));
        body.extend(make_interrupt_source_override_entry(1, 9));
        // Entrada de tipo desconhecido (ex.: Local APIC, tipo 0) — deve ser
        // ignorada sem quebrar o laço.
        body.extend([0u8, 8, 0, 0, 0, 0, 0, 0]);
        body.extend(make_interrupt_source_override_entry(0, 2));

        let parsed = walk_madt_entries(&body);
        assert_eq!(parsed.ioapic_address, Some(0xfec0_0000));
        assert_eq!(parsed.isa_irq_overrides[1], Some(9));
        assert_eq!(parsed.isa_irq_overrides[0], Some(2));
        assert_eq!(parsed.isa_irq_overrides[2], None);
    }

    #[test]
    fn walk_madt_entries_uses_the_first_ioapic_entry_when_more_than_one_exists() {
        let mut body = alloc::vec::Vec::new();
        body.extend(make_ioapic_entry(0, 0xfec0_0000, 0));
        body.extend(make_ioapic_entry(1, 0xfec0_1000, 24));

        let parsed = walk_madt_entries(&body);
        assert_eq!(parsed.ioapic_address, Some(0xfec0_0000));
    }

    #[test]
    fn walk_madt_entries_stops_on_an_inconsistent_length_without_panicking() {
        let mut body = alloc::vec::Vec::new();
        body.extend(make_ioapic_entry(0, 0xfec0_0000, 0));
        // Comprimento declarado maior que o restante do buffer.
        body.extend([2u8, 200]);

        let parsed = walk_madt_entries(&body);
        assert_eq!(parsed.ioapic_address, Some(0xfec0_0000));
    }

    #[test]
    fn resolve_isa_irq_gsi_identity_maps_when_no_override_is_registered() {
        *ISA_IRQ_OVERRIDES.lock() = [None; 16];
        assert_eq!(resolve_isa_irq_gsi(1), 1);
    }

    #[test]
    fn resolve_isa_irq_gsi_uses_the_registered_override() {
        let mut overrides = [None; 16];
        overrides[1] = Some(9);
        *ISA_IRQ_OVERRIDES.lock() = overrides;
        assert_eq!(resolve_isa_irq_gsi(1), 9);
        assert_eq!(resolve_isa_irq_gsi(2), 2);
        *ISA_IRQ_OVERRIDES.lock() = [None; 16];
    }

    #[test]
    fn resolve_isa_irq_gsi_identity_maps_an_out_of_range_irq() {
        assert_eq!(resolve_isa_irq_gsi(200), 200);
    }

    #[test]
    fn ioapic_address_is_none_when_nothing_was_discovered() {
        IOAPIC_ADDRESS.store(0, Ordering::Relaxed);
        assert_eq!(ioapic_address(), None);
    }

    #[test]
    fn ioapic_address_reflects_the_discovered_value() {
        IOAPIC_ADDRESS.store(0xfec0_0000, Ordering::Relaxed);
        assert_eq!(ioapic_address(), Some(0xfec0_0000));
        IOAPIC_ADDRESS.store(0, Ordering::Relaxed);
    }
}
