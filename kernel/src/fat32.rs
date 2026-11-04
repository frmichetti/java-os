//! Sistema de arquivos FAT32, somente leitura, nomes curtos 8.3 — ver spec
//! `kernel-filesystem` e `design.md` da change `kernel-fat32-filesystem`.
//!
//! Mesmo padrão de separação de `kernel::vmm`/`kernel::gpt`: parsing puro do
//! setor de inicialização (BPB), das entradas de diretório e das entradas da
//! FAT (sem nenhum acesso a `BlockDevice`), e uma camada real
//! (`Fat32Filesystem`) genérica sobre `hal::BlockDevice`.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::hal::BlockDevice;

/// Assinatura de inicialização obrigatória nos últimos 2 bytes de um setor
/// de boot válido.
const BOOT_SIGNATURE: [u8; 2] = [0x55, 0xAA];

/// Valor obrigatório do campo `FilSysType` (8 bytes, offset 82) para um
/// sistema de arquivos FAT32 (ao contrário de FAT12/FAT16, que usam outros
/// valores nesse campo, ou o deixam com outro conteúdo).
const FAT32_FILESYSTEM_TYPE: &[u8; 8] = b"FAT32   ";

/// Atributo (`Attr`, offset 11 de uma entrada de diretório) que marca uma
/// entrada como parte de um nome longo VFAT — combinação fixa de
/// `ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID`. Ignorada
/// nesta change (ver design.md, Non-Goals: nomes longos VFAT).
const ATTR_LONG_NAME: u8 = 0x0F;
const ATTR_DIRECTORY: u8 = 0x10;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Fat32Error {
    /// O setor de inicialização não tem a assinatura `0x55 0xAA`, ou o campo
    /// `FilSysType` não é `"FAT32   "`.
    NotFat32,
    /// `bpb.bytes_per_sector` não bate com o tamanho de bloco real do
    /// dispositivo — a aritmética de setor desta implementação assume que
    /// são iguais.
    UnsupportedSectorSize,
    /// Algum componente do caminho (intermediário ou final) não corresponde
    /// a nenhuma entrada existente no diretório correspondente.
    NotFound,
    /// O caminho corresponde a um diretório onde um arquivo era esperado.
    NotAFile,
    /// Um componente intermediário do caminho corresponde a um arquivo, não
    /// um diretório.
    NotADirectory,
    /// Uma leitura de bloco falhou ao consultar o disco.
    ReadFailed,
}

// ---------------------------------------------------------------------
// Camada pura: BPB (setor de inicialização).
// ---------------------------------------------------------------------

/// Campos do setor de inicialização (BPB) de FAT32 relevantes para navegar
/// pelo sistema de arquivos. Lógica pura, sem nenhum acesso a `BlockDevice`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Bpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sector_count: u16,
    pub num_fats: u8,
    pub fat_size_32: u32,
    pub root_cluster: u32,
}

impl Bpb {
    /// Interpreta `buf` (um setor de 512 bytes, o setor de inicialização da
    /// partição) como uma BPB de FAT32 — cobre "Partição encontrada não é um
    /// FAT32 válido" da spec `kernel-filesystem`.
    pub fn parse(buf: &[u8; 512]) -> Result<Self, Fat32Error> {
        if buf[510..512] != BOOT_SIGNATURE {
            return Err(Fat32Error::NotFat32);
        }
        if &buf[82..90] != FAT32_FILESYSTEM_TYPE {
            return Err(Fat32Error::NotFat32);
        }
        Ok(Self {
            bytes_per_sector: u16::from_le_bytes(buf[11..13].try_into().unwrap()),
            sectors_per_cluster: buf[13],
            reserved_sector_count: u16::from_le_bytes(buf[14..16].try_into().unwrap()),
            num_fats: buf[16],
            fat_size_32: u32::from_le_bytes(buf[36..40].try_into().unwrap()),
            root_cluster: u32::from_le_bytes(buf[44..48].try_into().unwrap()),
        })
    }

    /// Primeiro setor (relativo ao início da partição) ocupado pela FAT.
    pub fn first_fat_sector(&self) -> u64 {
        self.reserved_sector_count as u64
    }

    /// Primeiro setor (relativo ao início da partição) da área de dados
    /// (onde o cluster 2 — convencionalmente o primeiro cluster de dados —
    /// começa).
    pub fn first_data_sector(&self) -> u64 {
        self.first_fat_sector() + self.num_fats as u64 * self.fat_size_32 as u64
    }

    /// Primeiro setor (relativo ao início da partição) do cluster `cluster`
    /// (`cluster >= 2`).
    pub fn cluster_to_sector(&self, cluster: u32) -> u64 {
        self.first_data_sector() + (cluster as u64 - 2) * self.sectors_per_cluster as u64
    }

    /// Setor (relativo ao início da partição) e deslocamento dentro desse
    /// setor onde a entrada da FAT de `cluster` está — cada entrada ocupa 4
    /// bytes.
    pub fn fat_entry_location(&self, cluster: u32) -> (u64, usize) {
        let fat_offset = cluster as u64 * 4;
        let sector = self.first_fat_sector() + fat_offset / self.bytes_per_sector as u64;
        let offset = (fat_offset % self.bytes_per_sector as u64) as usize;
        (sector, offset)
    }
}

// ---------------------------------------------------------------------
// Camada pura: entradas de diretório.
// ---------------------------------------------------------------------

/// Uma entrada de diretório comum (arquivo ou subdiretório), já decodificada.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_directory: bool,
    pub first_cluster: u32,
    pub file_size: u32,
}

/// Resultado de interpretar uma entrada de 32 bytes de um diretório FAT32.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum DirEntryKind {
    /// Primeiro byte `0x00` — não há mais entradas neste diretório a partir
    /// daqui.
    EndOfDirectory,
    /// Primeiro byte `0xE5` — entrada livre (apagada), ignorada.
    Free,
    /// Atributo `0x0F` — parte de um nome longo VFAT, ignorada (ver
    /// design.md, Non-Goals).
    LongNamePart,
    /// Uma entrada de arquivo ou diretório comum.
    Entry(DirEntry),
}

impl DirEntryKind {
    /// Interpreta `buf` (32 bytes) como uma entrada de diretório FAT32.
    pub fn parse(buf: &[u8; 32]) -> Self {
        if buf[0] == 0x00 {
            return DirEntryKind::EndOfDirectory;
        }
        if buf[0] == 0xE5 {
            return DirEntryKind::Free;
        }
        let attr = buf[11];
        if attr == ATTR_LONG_NAME {
            return DirEntryKind::LongNamePart;
        }

        let name_raw = &buf[0..8];
        let ext_raw = &buf[8..11];
        let name = core::str::from_utf8(name_raw).unwrap_or("").trim_end();
        let ext = core::str::from_utf8(ext_raw).unwrap_or("").trim_end();
        let mut full_name = String::from(name);
        if !ext.is_empty() {
            full_name.push('.');
            full_name.push_str(ext);
        }

        let first_cluster_hi = u16::from_le_bytes(buf[20..22].try_into().unwrap()) as u32;
        let first_cluster_lo = u16::from_le_bytes(buf[26..28].try_into().unwrap()) as u32;
        let first_cluster = (first_cluster_hi << 16) | first_cluster_lo;
        let file_size = u32::from_le_bytes(buf[28..32].try_into().unwrap());

        DirEntryKind::Entry(DirEntry {
            name: full_name,
            is_directory: attr & ATTR_DIRECTORY != 0,
            first_cluster,
            file_size,
        })
    }
}

/// Compara um nome curto 8.3 já decodificado (`entry_name`) com um
/// componente de caminho fornecido pelo chamador (`component`),
/// insensivelmente a maiúsculas/minúsculas — cobre "Correspondência de Nomes
/// Curtos Sem Distinção de Maiúsculas/Minúsculas" da spec.
pub fn short_name_matches(entry_name: &str, component: &str) -> bool {
    entry_name.eq_ignore_ascii_case(component)
}

// ---------------------------------------------------------------------
// Camada pura: entradas da FAT (File Allocation Table).
// ---------------------------------------------------------------------

/// Máscara os 4 bits altos (reservados) de uma entrada bruta da FAT32 — só
/// os 28 bits baixos são significativos.
pub fn fat_entry_value(raw: u32) -> u32 {
    raw & 0x0FFF_FFFF
}

/// `true` se `value` (já mascarado por [`fat_entry_value`]) marca o fim de
/// uma cadeia de clusters.
pub fn is_end_of_chain(value: u32) -> bool {
    value >= 0x0FFF_FFF8
}

// ---------------------------------------------------------------------
// Camada real: liga o parsing puro acima a um `hal::BlockDevice`.
// ---------------------------------------------------------------------

/// Sistema de arquivos FAT32 montado sobre uma partição — ver spec
/// `kernel-filesystem`. Genérica sobre `hal::BlockDevice` em cada operação
/// (não guarda o dispositivo), mesmo padrão de `kernel::gpt::find_efi_system_partition`
/// — testável em host com um disco simulado em memória.
pub struct Fat32Filesystem {
    bpb: Bpb,
    partition_start_lba: u64,
}

impl Fat32Filesystem {
    /// Monta a partição que começa em `partition_start_lba`: lê e valida seu
    /// setor de inicialização — cobre "Montagem do Sistema de Arquivos via
    /// Tabela de Partição GPT" (a parte de validação do FAT32 em si) da spec.
    pub fn mount<D: BlockDevice>(device: &mut D, partition_start_lba: u64) -> Result<Self, Fat32Error> {
        let block_size = device.block_size();
        let mut buf = alloc::vec![0u8; block_size];
        device.read_block(partition_start_lba, &mut buf).map_err(|_| Fat32Error::ReadFailed)?;

        let mut boot_sector = [0u8; 512];
        let copy_len = buf.len().min(512);
        boot_sector[..copy_len].copy_from_slice(&buf[..copy_len]);
        let bpb = Bpb::parse(&boot_sector)?;

        if bpb.bytes_per_sector as usize != block_size {
            return Err(Fat32Error::UnsupportedSectorSize);
        }

        Ok(Self { bpb, partition_start_lba })
    }

    /// Lê o conteúdo completo do arquivo em `path` (caminho absoluto,
    /// componentes separados por `/`) — cobre "Leitura de Arquivo por
    /// Caminho Absoluto" da spec.
    pub fn read_file<D: BlockDevice>(&self, device: &mut D, path: &str) -> Result<Vec<u8>, Fat32Error> {
        let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        let Some((&last, intermediate)) = components.split_last() else {
            return Err(Fat32Error::NotAFile);
        };

        let mut current_cluster = self.bpb.root_cluster;
        for component in intermediate {
            let entry = self.find_in_directory(device, current_cluster, component)?;
            if !entry.is_directory {
                return Err(Fat32Error::NotADirectory);
            }
            current_cluster = entry.first_cluster;
        }

        let entry = self.find_in_directory(device, current_cluster, last)?;
        if entry.is_directory {
            return Err(Fat32Error::NotAFile);
        }
        self.read_cluster_chain(device, entry.first_cluster, entry.file_size as usize)
    }

    /// Lê a entrada da FAT do cluster `cluster`.
    fn read_fat_entry<D: BlockDevice>(&self, device: &mut D, cluster: u32) -> Result<u32, Fat32Error> {
        let block_size = device.block_size();
        let (sector, offset) = self.bpb.fat_entry_location(cluster);
        let mut buf = alloc::vec![0u8; block_size];
        device.read_block(self.partition_start_lba + sector, &mut buf).map_err(|_| Fat32Error::ReadFailed)?;
        let raw = u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        Ok(fat_entry_value(raw))
    }

    /// Procura, na cadeia de clusters do diretório `dir_cluster`, uma
    /// entrada cujo nome curto bate com `name` (sem distinção de
    /// maiúsculas/minúsculas) — cobre os cenários de erro de "Leitura de
    /// Arquivo por Caminho Absoluto" da spec.
    fn find_in_directory<D: BlockDevice>(
        &self,
        device: &mut D,
        dir_cluster: u32,
        name: &str,
    ) -> Result<DirEntry, Fat32Error> {
        let block_size = device.block_size();
        let mut buf = alloc::vec![0u8; block_size];
        let mut cluster = dir_cluster;

        loop {
            let first_sector = self.bpb.cluster_to_sector(cluster);
            for s in 0..self.bpb.sectors_per_cluster as u64 {
                device
                    .read_block(self.partition_start_lba + first_sector + s, &mut buf)
                    .map_err(|_| Fat32Error::ReadFailed)?;

                for chunk_start in (0..buf.len()).step_by(32) {
                    let mut entry_buf = [0u8; 32];
                    entry_buf.copy_from_slice(&buf[chunk_start..chunk_start + 32]);
                    match DirEntryKind::parse(&entry_buf) {
                        DirEntryKind::EndOfDirectory => return Err(Fat32Error::NotFound),
                        DirEntryKind::Free | DirEntryKind::LongNamePart => continue,
                        DirEntryKind::Entry(entry) => {
                            if short_name_matches(&entry.name, name) {
                                return Ok(entry);
                            }
                        }
                    }
                }
            }

            let next = self.read_fat_entry(device, cluster)?;
            if is_end_of_chain(next) {
                return Err(Fat32Error::NotFound);
            }
            cluster = next;
        }
    }

    /// Lê `size` bytes a partir do início da cadeia de clusters que começa
    /// em `first_cluster`, seguindo a FAT até acumular `size` bytes.
    fn read_cluster_chain<D: BlockDevice>(
        &self,
        device: &mut D,
        first_cluster: u32,
        size: usize,
    ) -> Result<Vec<u8>, Fat32Error> {
        let mut result = Vec::with_capacity(size);
        if size == 0 {
            return Ok(result);
        }

        let block_size = device.block_size();
        let mut buf = alloc::vec![0u8; block_size];
        let mut cluster = first_cluster;

        loop {
            let first_sector = self.bpb.cluster_to_sector(cluster);
            for s in 0..self.bpb.sectors_per_cluster as u64 {
                if result.len() >= size {
                    break;
                }
                device
                    .read_block(self.partition_start_lba + first_sector + s, &mut buf)
                    .map_err(|_| Fat32Error::ReadFailed)?;
                let remaining = size - result.len();
                let take = remaining.min(buf.len());
                result.extend_from_slice(&buf[..take]);
            }

            if result.len() >= size {
                return Ok(result);
            }

            let next = self.read_fat_entry(device, cluster)?;
            if is_end_of_chain(next) {
                return Ok(result);
            }
            cluster = next;
        }
    }
}

/// Motivo pelo qual montar o sistema de arquivos da partição de boot falhou
/// — ver [`mount_boot_partition`].
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MountError {
    /// O driver de armazenamento (`kernel::storage`) não está inicializado
    /// (nenhum disco encontrado em boot).
    NoDisk,
    /// Falha ao localizar a partição EFI System na tabela GPT do disco.
    Gpt(crate::gpt::GptError),
    /// A partição EFI System existe, mas não é um FAT32 válido, ou a
    /// operação de arquivo falhou.
    Fat32(Fat32Error),
}

/// Monta o sistema de arquivos FAT32 da partição de boot (disco global de
/// `kernel::storage`): localiza a partição EFI System via `kernel::gpt` e
/// monta essa partição como FAT32 — função de conveniência usada pelo demo e
/// por uma futura exposição via *syscall*.
pub fn mount_boot_partition() -> Result<Fat32Filesystem, MountError> {
    crate::storage::with_disk(|disk| {
        let partition_lba = crate::gpt::find_efi_system_partition(disk).map_err(MountError::Gpt)?;
        Fat32Filesystem::mount(disk, partition_lba).map_err(MountError::Fat32)
    })
    .ok_or(MountError::NoDisk)?
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------
    // Bpb (tarefa 2.1)
    // -------------------------------------------------------------

    fn build_bpb(
        bytes_per_sector: u16,
        sectors_per_cluster: u8,
        reserved_sector_count: u16,
        num_fats: u8,
        fat_size_32: u32,
        root_cluster: u32,
    ) -> [u8; 512] {
        let mut buf = [0u8; 512];
        buf[11..13].copy_from_slice(&bytes_per_sector.to_le_bytes());
        buf[13] = sectors_per_cluster;
        buf[14..16].copy_from_slice(&reserved_sector_count.to_le_bytes());
        buf[16] = num_fats;
        buf[36..40].copy_from_slice(&fat_size_32.to_le_bytes());
        buf[44..48].copy_from_slice(&root_cluster.to_le_bytes());
        buf[82..90].copy_from_slice(FAT32_FILESYSTEM_TYPE);
        buf[510..512].copy_from_slice(&BOOT_SIGNATURE);
        buf
    }

    #[test]
    fn valid_fat32_boot_sector_is_parsed_with_correct_derived_values() {
        let buf = build_bpb(512, 4, 32, 2, 100, 2);
        let bpb = Bpb::parse(&buf).expect("setor de boot valido deveria ser aceito");
        assert_eq!(bpb.bytes_per_sector, 512);
        assert_eq!(bpb.sectors_per_cluster, 4);
        assert_eq!(bpb.reserved_sector_count, 32);
        assert_eq!(bpb.num_fats, 2);
        assert_eq!(bpb.fat_size_32, 100);
        assert_eq!(bpb.root_cluster, 2);

        // Calculo manual: first_fat_sector = 32; first_data_sector = 32 + 2*100 = 232;
        // cluster 2 (primeiro cluster de dados) comeca exatamente em first_data_sector.
        assert_eq!(bpb.first_fat_sector(), 32);
        assert_eq!(bpb.first_data_sector(), 232);
        assert_eq!(bpb.cluster_to_sector(2), 232);
        assert_eq!(bpb.cluster_to_sector(3), 236); // + 1 cluster (4 setores)

        // Entrada da FAT do cluster 10: offset = 40 bytes, dentro do primeiro setor da FAT.
        assert_eq!(bpb.fat_entry_location(10), (32, 40));
    }

    #[test]
    fn boot_sector_without_signature_is_rejected() {
        let mut buf = build_bpb(512, 4, 32, 2, 100, 2);
        buf[510] = 0x00;
        assert_eq!(Bpb::parse(&buf), Err(Fat32Error::NotFat32));
    }

    #[test]
    fn boot_sector_without_fat32_filesystem_type_is_rejected() {
        let mut buf = build_bpb(512, 4, 32, 2, 100, 2);
        buf[82..90].copy_from_slice(b"FAT16   ");
        assert_eq!(Bpb::parse(&buf), Err(Fat32Error::NotFat32));
    }

    // -------------------------------------------------------------
    // DirEntryKind (tarefa 2.2)
    // -------------------------------------------------------------

    fn build_dir_entry(name: &[u8; 8], ext: &[u8; 3], attr: u8, first_cluster: u32, file_size: u32) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..8].copy_from_slice(name);
        buf[8..11].copy_from_slice(ext);
        buf[11] = attr;
        buf[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
        buf[26..28].copy_from_slice(&((first_cluster & 0xffff) as u16).to_le_bytes());
        buf[28..32].copy_from_slice(&file_size.to_le_bytes());
        buf
    }

    #[test]
    fn valid_file_entry_decodes_name_attributes_cluster_and_size() {
        let buf = build_dir_entry(b"KERNEL  ", b"   ", 0x20, 0x0001_0203, 4096);
        match DirEntryKind::parse(&buf) {
            DirEntryKind::Entry(entry) => {
                assert_eq!(entry.name, "KERNEL");
                assert!(!entry.is_directory);
                assert_eq!(entry.first_cluster, 0x0001_0203);
                assert_eq!(entry.file_size, 4096);
            }
            other => panic!("esperava Entry, obteve {other:?}"),
        }
    }

    #[test]
    fn directory_entry_sets_the_directory_flag() {
        let buf = build_dir_entry(b"BOOT    ", b"   ", ATTR_DIRECTORY, 5, 0);
        match DirEntryKind::parse(&buf) {
            DirEntryKind::Entry(entry) => {
                assert_eq!(entry.name, "BOOT");
                assert!(entry.is_directory);
            }
            other => panic!("esperava Entry, obteve {other:?}"),
        }
    }

    #[test]
    fn entry_with_extension_combines_name_and_extension_with_a_dot() {
        let buf = build_dir_entry(b"BOOTX64 ", b"EFI", 0x20, 7, 1024);
        match DirEntryKind::parse(&buf) {
            DirEntryKind::Entry(entry) => assert_eq!(entry.name, "BOOTX64.EFI"),
            other => panic!("esperava Entry, obteve {other:?}"),
        }
    }

    #[test]
    fn first_byte_zero_is_end_of_directory() {
        let mut buf = [0u8; 32];
        buf[0] = 0x00;
        assert_eq!(DirEntryKind::parse(&buf), DirEntryKind::EndOfDirectory);
    }

    #[test]
    fn first_byte_0xe5_is_free() {
        let mut buf = [0u8; 32];
        buf[0] = 0xE5;
        assert_eq!(DirEntryKind::parse(&buf), DirEntryKind::Free);
    }

    #[test]
    fn attribute_0x0f_is_a_long_name_part() {
        let buf = build_dir_entry(b"ANYTHING", b"ANY", ATTR_LONG_NAME, 0, 0);
        assert_eq!(DirEntryKind::parse(&buf), DirEntryKind::LongNamePart);
    }

    // -------------------------------------------------------------
    // FAT (tarefa 2.3)
    // -------------------------------------------------------------

    #[test]
    fn fat_entry_value_masks_the_high_reserved_bits() {
        assert_eq!(fat_entry_value(0xF000_0005), 0x0000_0005);
    }

    #[test]
    fn values_at_or_above_the_threshold_are_end_of_chain() {
        assert!(is_end_of_chain(0x0FFF_FFF8));
        assert!(is_end_of_chain(0x0FFF_FFFF));
    }

    #[test]
    fn a_normal_cluster_value_is_not_end_of_chain() {
        assert!(!is_end_of_chain(5));
        assert!(!is_end_of_chain(0x0FFF_FFF7)); // cluster defeituoso, nao e fim de cadeia
    }

    // -------------------------------------------------------------
    // short_name_matches (tarefa 2.4)
    // -------------------------------------------------------------

    #[test]
    fn short_name_matches_is_case_insensitive() {
        assert!(short_name_matches("KERNEL", "kernel"));
        assert!(short_name_matches("BOOTX64.EFI", "bootx64.efi"));
        assert!(short_name_matches("Boot", "BOOT"));
    }

    #[test]
    fn different_names_do_not_match() {
        assert!(!short_name_matches("KERNEL", "kernel2"));
        assert!(!short_name_matches("BOOT", "BOOTX64"));
    }

    // -------------------------------------------------------------
    // Fat32Filesystem (tarefas 3.1-3.3): camada real sobre um
    // `BlockDevice` simulado, montando uma particao FAT32 sintetica:
    //
    //   setor 0: BPB (bytes_per_sector=512, sectors_per_cluster=1,
    //            reserved_sector_count=1, num_fats=1, fat_size_32=1,
    //            root_cluster=2)
    //   setor 1: FAT (1 setor, 128 entradas de 4 bytes)
    //   setor 2: cluster 2 = diretorio raiz -> "BOOT" (dir, cluster 3)
    //   setor 3: cluster 3 = diretorio BOOT -> "KERNEL" (arquivo, cluster 4,
    //            700 bytes, 2 clusters) e "SMALL.TXT" (arquivo, cluster 6,
    //            10 bytes, 1 cluster)
    //   setor 4: cluster 4 = primeiros 512 bytes de KERNEL (0xAA)
    //   setor 5: cluster 5 = ultimos 188 bytes de KERNEL (0xBB) + resto
    //   setor 6: cluster 6 = conteudo de SMALL.TXT ("HELLOWORLD")
    use crate::hal::test_support::SimulatedBlockDevice;

    fn write_dir_entry(buf: &mut [u8], offset: usize, name: &[u8; 8], ext: &[u8; 3], attr: u8, cluster: u32, size: u32) {
        buf[offset..offset + 8].copy_from_slice(name);
        buf[offset + 8..offset + 11].copy_from_slice(ext);
        buf[offset + 11] = attr;
        buf[offset + 20..offset + 22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
        buf[offset + 26..offset + 28].copy_from_slice(&((cluster & 0xffff) as u16).to_le_bytes());
        buf[offset + 28..offset + 32].copy_from_slice(&size.to_le_bytes());
    }

    fn write_fat_entry(buf: &mut [u8], cluster: u32, value: u32) {
        let offset = cluster as usize * 4;
        buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    const END_OF_CHAIN: u32 = 0x0FFF_FFFF;

    fn synthetic_disk() -> SimulatedBlockDevice {
        let mut disk = SimulatedBlockDevice::new(16);

        *disk.block_mut(0) = build_bpb(512, 1, 1, 1, 1, 2);

        {
            let fat = disk.block_mut(1);
            write_fat_entry(fat, 2, END_OF_CHAIN);
            write_fat_entry(fat, 3, END_OF_CHAIN);
            write_fat_entry(fat, 4, 5);
            write_fat_entry(fat, 5, END_OF_CHAIN);
            write_fat_entry(fat, 6, END_OF_CHAIN);
        }

        {
            let root = disk.block_mut(2);
            write_dir_entry(root, 0, b"BOOT    ", b"   ", ATTR_DIRECTORY, 3, 0);
        }

        {
            let boot_dir = disk.block_mut(3);
            write_dir_entry(boot_dir, 0, b"KERNEL  ", b"   ", 0x20, 4, 700);
            write_dir_entry(boot_dir, 32, b"SMALL   ", b"TXT", 0x20, 6, 10);
        }

        *disk.block_mut(4) = [0xAAu8; 512];

        {
            let cluster5 = disk.block_mut(5);
            cluster5[0..188].copy_from_slice(&[0xBBu8; 188]);
        }

        {
            let cluster6 = disk.block_mut(6);
            cluster6[0..10].copy_from_slice(b"HELLOWORLD");
        }

        disk
    }

    #[test]
    fn mounting_a_valid_synthetic_fat32_partition_succeeds() {
        let mut disk = synthetic_disk();
        assert!(Fat32Filesystem::mount(&mut disk, 0).is_ok());
    }

    #[test]
    fn mounting_an_invalid_boot_sector_is_rejected() {
        let mut disk = SimulatedBlockDevice::new(2);
        assert_eq!(Fat32Filesystem::mount(&mut disk, 0).err(), Some(Fat32Error::NotFat32));
    }

    #[test]
    fn mounting_a_filesystem_with_a_mismatched_sector_size_is_rejected() {
        let mut disk = SimulatedBlockDevice::new(2);
        // bytes_per_sector (1024) nao bate com o tamanho de bloco real do
        // `SimulatedBlockDevice` (512, fixo).
        *disk.block_mut(0) = build_bpb(1024, 1, 1, 1, 1, 2);
        assert_eq!(Fat32Filesystem::mount(&mut disk, 0).err(), Some(Fat32Error::UnsupportedSectorSize));
    }

    #[test]
    fn reading_a_single_cluster_file_returns_its_exact_content() {
        let mut disk = synthetic_disk();
        let fs = Fat32Filesystem::mount(&mut disk, 0).unwrap();
        let content = fs.read_file(&mut disk, "/BOOT/SMALL.TXT").unwrap();
        assert_eq!(content, b"HELLOWORLD".to_vec());
    }

    #[test]
    fn reading_a_multi_cluster_file_returns_all_its_content_in_order() {
        let mut disk = synthetic_disk();
        let fs = Fat32Filesystem::mount(&mut disk, 0).unwrap();
        let content = fs.read_file(&mut disk, "/BOOT/KERNEL").unwrap();

        let mut expected = alloc::vec![0xAAu8; 512];
        expected.extend_from_slice(&[0xBBu8; 188]);
        assert_eq!(content.len(), 700);
        assert_eq!(content, expected);
    }

    #[test]
    fn reading_a_case_insensitive_path_finds_the_same_file() {
        let mut disk = synthetic_disk();
        let fs = Fat32Filesystem::mount(&mut disk, 0).unwrap();
        let content = fs.read_file(&mut disk, "/boot/small.txt").unwrap();
        assert_eq!(content, b"HELLOWORLD".to_vec());
    }

    #[test]
    fn reading_a_nonexistent_path_returns_not_found() {
        let mut disk = synthetic_disk();
        let fs = Fat32Filesystem::mount(&mut disk, 0).unwrap();
        assert_eq!(fs.read_file(&mut disk, "/boot/missing.txt"), Err(Fat32Error::NotFound));
    }

    #[test]
    fn reading_a_directory_as_a_file_returns_not_a_file() {
        let mut disk = synthetic_disk();
        let fs = Fat32Filesystem::mount(&mut disk, 0).unwrap();
        assert_eq!(fs.read_file(&mut disk, "/boot"), Err(Fat32Error::NotAFile));
    }

    #[test]
    fn an_intermediate_path_component_that_is_a_file_returns_not_a_directory() {
        let mut disk = synthetic_disk();
        let fs = Fat32Filesystem::mount(&mut disk, 0).unwrap();
        assert_eq!(fs.read_file(&mut disk, "/boot/kernel/anything"), Err(Fat32Error::NotADirectory));
    }
}
