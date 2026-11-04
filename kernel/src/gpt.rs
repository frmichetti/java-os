//! Parsing da tabela de partição GPT (GUID Partition Table) — localiza a
//! partição "EFI System Partition" (onde este projeto formata seu sistema de
//! arquivos FAT32) sem hardcodar o LBA de início usado pelo `xtask` para
//! montar a imagem de disco. Ver spec `kernel-filesystem` e `design.md` da
//! change `kernel-fat32-filesystem`.
//!
//! Dividido no mesmo padrão de `kernel::vmm`/`kernel::phys`: parsing puro
//! (`GptHeader`/`GptPartitionEntry`, sem nenhum acesso a `BlockDevice`) e uma
//! função de integração (`find_efi_system_partition`) genérica sobre
//! `hal::BlockDevice`, testável em host com um disco simulado.

extern crate alloc;

use crate::hal::BlockDevice;

/// Assinatura obrigatória nos primeiros 8 bytes de um cabeçalho GPT válido.
const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";

/// GUID da partição "EFI System Partition", codificado na ordem mista usual
/// de GUIDs (`C12A7328-F81F-11D2-BA4B-00A0C93EC93B`): os três primeiros
/// campos em *little-endian*, os dois últimos na ordem em que são escritos.
pub const EFI_SYSTEM_PARTITION_GUID: [u8; 16] =
    [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B];

/// Quantos setores da tabela de entradas de partição são varridos à procura
/// de uma partição EFI System — ver design.md, Non-Goals ("GPT totalmente
/// geral"). Generoso o bastante para encontrar a primeira entrada mesmo que
/// não seja exatamente a entrada 0.
const MAX_PARTITION_TABLE_SECTORS_SCANNED: u64 = 4;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum GptError {
    /// O LBA 1 do disco não contém um cabeçalho GPT válido (assinatura
    /// `"EFI PART"` ausente).
    NoGptHeader,
    /// O cabeçalho GPT é válido, mas nenhuma entrada de partição varrida tem
    /// o tipo "EFI System Partition".
    NoEfiSystemPartition,
    /// Uma leitura de bloco falhou ao consultar o disco.
    ReadFailed,
}

/// Campos do cabeçalho GPT (LBA 1) relevantes para localizar a tabela de
/// entradas de partição. Lógica pura, sem nenhum acesso a `BlockDevice`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct GptHeader {
    pub partition_entry_lba: u64,
    pub num_partition_entries: u32,
    pub size_of_partition_entry: u32,
}

impl GptHeader {
    /// Interpreta `buf` (um setor de 512 bytes, LBA 1 do disco) como um
    /// cabeçalho GPT — `None` se a assinatura `"EFI PART"` não bater.
    pub fn parse(buf: &[u8; 512]) -> Option<Self> {
        if &buf[0..8] != GPT_SIGNATURE {
            return None;
        }
        Some(Self {
            partition_entry_lba: u64::from_le_bytes(buf[72..80].try_into().unwrap()),
            num_partition_entries: u32::from_le_bytes(buf[80..84].try_into().unwrap()),
            size_of_partition_entry: u32::from_le_bytes(buf[84..88].try_into().unwrap()),
        })
    }
}

/// Campos de uma entrada da tabela de partição GPT relevantes para localizar
/// a partição EFI System. Lógica pura, sem nenhum acesso a `BlockDevice`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct GptPartitionEntry {
    pub partition_type_guid: [u8; 16],
    pub starting_lba: u64,
}

impl GptPartitionEntry {
    /// Interpreta `buf` (ao menos os primeiros 40 bytes de uma entrada de
    /// `size_of_partition_entry` bytes) como uma entrada de partição GPT.
    pub fn parse(buf: &[u8]) -> Self {
        let mut partition_type_guid = [0u8; 16];
        partition_type_guid.copy_from_slice(&buf[0..16]);
        Self { partition_type_guid, starting_lba: u64::from_le_bytes(buf[32..40].try_into().unwrap()) }
    }

    /// `true` se esta entrada for do tipo "EFI System Partition".
    pub fn is_efi_system_partition(&self) -> bool {
        self.partition_type_guid == EFI_SYSTEM_PARTITION_GUID
    }
}

/// Localiza a primeira partição do tipo "EFI System Partition" na tabela GPT
/// de `device`, devolvendo seu LBA inicial — cobre "Montagem do Sistema de
/// Arquivos via Tabela de Partição GPT" da spec `kernel-filesystem`.
pub fn find_efi_system_partition<D: BlockDevice>(device: &mut D) -> Result<u64, GptError> {
    let block_size = device.block_size();
    let mut buf = alloc::vec![0u8; block_size];

    device.read_block(1, &mut buf).map_err(|_| GptError::ReadFailed)?;
    let mut header_buf = [0u8; 512];
    let copy_len = buf.len().min(512);
    header_buf[..copy_len].copy_from_slice(&buf[..copy_len]);
    let header = GptHeader::parse(&header_buf).ok_or(GptError::NoGptHeader)?;

    let entries_per_sector = block_size / header.size_of_partition_entry.max(1) as usize;
    let mut entries_seen = 0u32;

    for sector_offset in 0..MAX_PARTITION_TABLE_SECTORS_SCANNED {
        if entries_seen >= header.num_partition_entries {
            break;
        }
        let lba = header.partition_entry_lba + sector_offset;
        device.read_block(lba, &mut buf).map_err(|_| GptError::ReadFailed)?;

        for i in 0..entries_per_sector {
            if entries_seen >= header.num_partition_entries {
                break;
            }
            let start = i * header.size_of_partition_entry as usize;
            let end = start + header.size_of_partition_entry as usize;
            if end > buf.len() {
                break;
            }
            let entry = GptPartitionEntry::parse(&buf[start..end]);
            if entry.is_efi_system_partition() {
                return Ok(entry.starting_lba);
            }
            entries_seen += 1;
        }
    }

    Err(GptError::NoEfiSystemPartition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::test_support::SimulatedBlockDevice;

    /// Monta um cabeçalho GPT válido (bytes conhecidos) num buffer de 512
    /// bytes, com `partition_entry_lba`/`num_partition_entries`/
    /// `size_of_partition_entry` dados.
    fn build_header(partition_entry_lba: u64, num_partition_entries: u32, size_of_partition_entry: u32) -> [u8; 512] {
        let mut buf = [0u8; 512];
        buf[0..8].copy_from_slice(GPT_SIGNATURE);
        buf[72..80].copy_from_slice(&partition_entry_lba.to_le_bytes());
        buf[80..84].copy_from_slice(&num_partition_entries.to_le_bytes());
        buf[84..88].copy_from_slice(&size_of_partition_entry.to_le_bytes());
        buf
    }

    fn write_entry(buf: &mut [u8], offset: usize, type_guid: [u8; 16], starting_lba: u64) {
        buf[offset..offset + 16].copy_from_slice(&type_guid);
        buf[offset + 32..offset + 40].copy_from_slice(&starting_lba.to_le_bytes());
    }

    #[test]
    fn header_with_valid_signature_is_parsed() {
        let buf = build_header(2, 128, 128);
        let header = GptHeader::parse(&buf).expect("assinatura valida deveria ser aceita");
        assert_eq!(header.partition_entry_lba, 2);
        assert_eq!(header.num_partition_entries, 128);
        assert_eq!(header.size_of_partition_entry, 128);
    }

    #[test]
    fn header_without_signature_is_rejected() {
        let buf = [0u8; 512];
        assert_eq!(GptHeader::parse(&buf), None);
    }

    #[test]
    fn partition_entry_extracts_guid_and_starting_lba() {
        let mut buf = [0u8; 128];
        write_entry(&mut buf, 0, EFI_SYSTEM_PARTITION_GUID, 2048);
        let entry = GptPartitionEntry::parse(&buf);
        assert_eq!(entry.partition_type_guid, EFI_SYSTEM_PARTITION_GUID);
        assert_eq!(entry.starting_lba, 2048);
        assert!(entry.is_efi_system_partition());
    }

    #[test]
    fn efi_system_partition_guid_matches_the_known_mixed_endian_encoding() {
        // C12A7328-F81F-11D2-BA4B-00A0C93EC93B: Data1/Data2/Data3 em
        // little-endian, Data4 (8 bytes) na ordem em que e escrito.
        let data1: u32 = 0xC12A7328;
        let data2: u16 = 0xF81F;
        let data3: u16 = 0x11D2;
        let data4: [u8; 8] = [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B];

        let mut expected = [0u8; 16];
        expected[0..4].copy_from_slice(&data1.to_le_bytes());
        expected[4..6].copy_from_slice(&data2.to_le_bytes());
        expected[6..8].copy_from_slice(&data3.to_le_bytes());
        expected[8..16].copy_from_slice(&data4);

        assert_eq!(EFI_SYSTEM_PARTITION_GUID, expected);
    }

    fn disk_with_efi_partition(starting_lba: u64) -> SimulatedBlockDevice {
        let mut disk = SimulatedBlockDevice::new(16);
        *disk.block_mut(1) = build_header(2, 4, 128);
        let entries_sector = disk.block_mut(2);
        write_entry(entries_sector, 0, EFI_SYSTEM_PARTITION_GUID, starting_lba);
        disk
    }

    #[test]
    fn finds_the_efi_system_partition_on_a_valid_disk() {
        let mut disk = disk_with_efi_partition(2048);
        assert_eq!(find_efi_system_partition(&mut disk), Ok(2048));
    }

    #[test]
    fn disk_without_a_gpt_header_is_rejected() {
        let mut disk = SimulatedBlockDevice::new(16);
        assert_eq!(find_efi_system_partition(&mut disk), Err(GptError::NoGptHeader));
    }

    #[test]
    fn disk_with_gpt_but_no_efi_system_partition_is_rejected() {
        let mut disk = SimulatedBlockDevice::new(16);
        *disk.block_mut(1) = build_header(2, 1, 128);
        let other_guid = [0xAAu8; 16];
        let entries_sector = disk.block_mut(2);
        write_entry(entries_sector, 0, other_guid, 2048);

        assert_eq!(find_efi_system_partition(&mut disk), Err(GptError::NoEfiSystemPartition));
    }
}
