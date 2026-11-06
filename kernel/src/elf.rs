//! Parsing e carregamento de executáveis ELF64 estáticos — ver spec
//! `kernel-elf-loader` e `design.md` da change `kernel-elf-loader`.
//!
//! Mesmo padrão de separação de `kernel::gpt`/`kernel::fat32`: parsing puro
//! do cabeçalho e dos program headers (`ElfHeader`/`ProgramHeader`, sem
//! nenhum acesso a memória do processo destino) e funções auxiliares puras
//! de mapeamento (`segment_page_range`/`segment_page_flags`/
//! `page_ranges_overlap`), testáveis com `cargo test -p kernel --lib` sem
//! hardware. O mapeamento real de um segmento (`load_segment`, que caminha
//! tabelas de página via o alias HHDM) e a função de alto nível que lê o
//! arquivo e cria o processo (`kernel::scheduler::spawn_from_elf`) só são
//! verificáveis em boot real — ver design.md, "`load_and_spawn` vive em
//! `kernel::scheduler`".

extern crate alloc;

use alloc::vec::Vec;

use crate::hhdm;
use crate::paging::{Mapper, PageFlags, PAGE_SIZE};
use crate::phys::{FrameAllocator, FrameStore};

/// Assinatura obrigatória nos primeiros 4 bytes de um arquivo ELF válido.
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// `EI_CLASS` (byte 4 do cabeçalho) para ELF64 — único suportado.
const ELFCLASS64: u8 = 2;
/// `EI_DATA` (byte 5 do cabeçalho) para little-endian — único suportado.
const ELFDATA2LSB: u8 = 1;
/// `e_type` para um executável estático — único suportado (sem PIE/`ET_DYN`,
/// ver design.md, Non-Goals).
const ET_EXEC: u16 = 2;
/// `e_machine` para x86-64 — única arquitetura suportada.
const EM_X86_64: u16 = 62;

/// `p_type` de um segmento carregável.
const PT_LOAD: u32 = 1;
/// Bit de `p_flags` que marca um segmento gravável.
const PF_W: u32 = 2;
/// Bit de `p_flags` que marca um segmento executável.
const PF_X: u32 = 1;

/// Tamanho, em bytes, de um cabeçalho ELF64 (`Elf64_Ehdr`).
const EHDR_SIZE: usize = 64;
/// Tamanho, em bytes, de um program header ELF64 (`Elf64_Phdr`) nos campos
/// que esta implementação lê.
const PHDR_SIZE: usize = 56;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ElfError {
    /// O buffer é menor que um cabeçalho ELF64, ou não começa com a
    /// assinatura `0x7F 'E' 'L' 'F'`.
    NotElf,
    /// Cabeçalho ELF válido, mas de classe, endianness, arquitetura ou tipo
    /// não suportados (só ELFCLASS64/ELFDATA2LSB/EM_X86_64/ET_EXEC).
    UnsupportedExecutable,
    /// A tabela de program headers (`e_phoff`/`e_phnum`/`e_phentsize`)
    /// aponta, ao menos em parte, para fora do buffer do arquivo.
    ProgramHeaderTableOutOfBounds,
    /// Um segmento `PT_LOAD` declara `p_offset`/`p_filesz` fora do buffer do
    /// arquivo, ou `p_memsz` menor que `p_filesz` (segmento malformado).
    InvalidSegment,
    /// Dois segmentos `PT_LOAD` ocupam, ao menos em parte, a mesma página de
    /// memória.
    OverlappingSegments,
    /// Falha ao mapear uma página de um segmento (sem memória física
    /// disponível, ou colisão de mapeamento não detectada pela checagem de
    /// sobreposição).
    MapFailed,
    /// O disco de armazenamento global não está inicializado — ver
    /// `kernel::storage::with_disk`.
    NoDisk,
    /// Falha ao ler o arquivo do executável do sistema de arquivos
    /// (caminho não encontrado, não é um arquivo, ou falha de leitura de
    /// disco).
    FileRead(crate::fat32::Fat32Error),
    /// Falha ao criar o processo (espaço de endereçamento ou stacks) a
    /// partir do executável já validado e mapeado.
    Process(crate::process::ProcessError),
}

/// Campos do cabeçalho ELF64 relevantes para carregar um executável
/// estático. Lógica pura, sem nenhum acesso a memória do processo destino.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ElfHeader {
    pub entry: u64,
    pub phoff: u64,
    pub phentsize: u16,
    pub phnum: u16,
}

impl ElfHeader {
    /// Interpreta `buf` (o conteúdo completo do arquivo) como um cabeçalho
    /// ELF64 de executável estático — cobre "Validação do Cabeçalho e da
    /// Arquitetura do Executável" da spec `kernel-elf-loader`. Nunca indexa
    /// `buf` fora dos limites: um buffer curto demais é `ElfError::NotElf`.
    pub fn parse(buf: &[u8]) -> Result<Self, ElfError> {
        let header = buf.get(0..EHDR_SIZE).ok_or(ElfError::NotElf)?;

        if header[0..4] != ELF_MAGIC {
            return Err(ElfError::NotElf);
        }
        if header[4] != ELFCLASS64 || header[5] != ELFDATA2LSB {
            return Err(ElfError::UnsupportedExecutable);
        }

        let e_type = u16::from_le_bytes(header[16..18].try_into().unwrap());
        let e_machine = u16::from_le_bytes(header[18..20].try_into().unwrap());
        if e_type != ET_EXEC || e_machine != EM_X86_64 {
            return Err(ElfError::UnsupportedExecutable);
        }

        let entry = u64::from_le_bytes(header[24..32].try_into().unwrap());
        let phoff = u64::from_le_bytes(header[32..40].try_into().unwrap());
        let phentsize = u16::from_le_bytes(header[54..56].try_into().unwrap());
        let phnum = u16::from_le_bytes(header[56..58].try_into().unwrap());

        Ok(Self { entry, phoff, phentsize, phnum })
    }
}

/// Campos de um program header `PT_LOAD` relevantes para mapear um segmento
/// carregável. Lógica pura, sem nenhum acesso a memória do processo destino.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ProgramHeader {
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
}

/// Devolve, na ordem em que aparecem na tabela, os program headers do tipo
/// `PT_LOAD` descritos por `header` dentro de `buf` — cobre "Mapeamento dos
/// Segmentos Carregáveis com Permissões Derivadas do Executável" da spec.
/// Rejeita com `ElfError` qualquer offset/tamanho (da tabela em si ou de um
/// segmento individual) que exceda o tamanho real de `buf`, ou um segmento
/// com `p_memsz < p_filesz`, sem nunca indexar `buf` fora dos limites.
pub fn load_program_headers(buf: &[u8], header: &ElfHeader) -> Result<Vec<ProgramHeader>, ElfError> {
    let phentsize = header.phentsize as usize;
    let phnum = header.phnum as usize;
    let table_len = phentsize.checked_mul(phnum).ok_or(ElfError::ProgramHeaderTableOutOfBounds)?;
    let table_start = usize::try_from(header.phoff).map_err(|_| ElfError::ProgramHeaderTableOutOfBounds)?;
    let table_end = table_start.checked_add(table_len).ok_or(ElfError::ProgramHeaderTableOutOfBounds)?;
    let table = buf.get(table_start..table_end).ok_or(ElfError::ProgramHeaderTableOutOfBounds)?;

    let mut result = Vec::new();
    for entry in table.chunks_exact(phentsize) {
        let raw = entry.get(0..PHDR_SIZE).ok_or(ElfError::ProgramHeaderTableOutOfBounds)?;
        let p_type = u32::from_le_bytes(raw[0..4].try_into().unwrap());
        if p_type != PT_LOAD {
            continue;
        }

        let p_flags = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        let p_offset = u64::from_le_bytes(raw[8..16].try_into().unwrap());
        let p_vaddr = u64::from_le_bytes(raw[16..24].try_into().unwrap());
        let p_filesz = u64::from_le_bytes(raw[32..40].try_into().unwrap());
        let p_memsz = u64::from_le_bytes(raw[40..48].try_into().unwrap());

        if p_memsz < p_filesz {
            return Err(ElfError::InvalidSegment);
        }
        let seg_start = usize::try_from(p_offset).map_err(|_| ElfError::InvalidSegment)?;
        let seg_len = usize::try_from(p_filesz).map_err(|_| ElfError::InvalidSegment)?;
        let seg_end = seg_start.checked_add(seg_len).ok_or(ElfError::InvalidSegment)?;
        if seg_end > buf.len() {
            return Err(ElfError::InvalidSegment);
        }

        result.push(ProgramHeader { flags: p_flags, offset: p_offset, vaddr: p_vaddr, filesz: p_filesz, memsz: p_memsz });
    }
    Ok(result)
}

/// Primeira página e página-limite (exclusiva) cobertas por um segmento com
/// endereço virtual `vaddr` e tamanho em memória `memsz` — intervalo
/// alinhado para baixo/para cima em `PAGE_SIZE` (mesmo padrão de
/// `kernel::phys::align_up`/`align_down`), pronto para iterar em passos de
/// página com `Mapper::map_new`.
pub fn segment_page_range(vaddr: u64, memsz: u64) -> (u64, u64) {
    let start = vaddr & !(PAGE_SIZE - 1);
    let end = (vaddr + memsz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    (start, end)
}

/// `PageFlags` para as páginas de um segmento, derivada de `p_flags`
/// (`PF_W`→`writable`, `PF_X`→`executable`). Sempre `user_accessible`: um
/// segmento de executável só existe em processos de modo usuário — ver spec
/// `kernel-user-mode`.
pub fn segment_page_flags(p_flags: u32) -> PageFlags {
    PageFlags { writable: p_flags & PF_W != 0, user_accessible: true, executable: p_flags & PF_X != 0 }
}

/// `true` se os intervalos de página `[a.0, a.1)` e `[b.0, b.1)` (como
/// devolvidos por [`segment_page_range`]) se sobrepõem em ao menos uma
/// página.
pub fn page_ranges_overlap(a: (u64, u64), b: (u64, u64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Checa, par a par, se algum dos intervalos de página de `segments` se
/// sobrepõe a outro — cobre o cenário de erro "Segmentos carregáveis se
/// sobrepõem" da spec. Chamada antes de mapear qualquer coisa, para nunca
/// deixar um processo parcialmente mapeado quando o executável é malformado
/// dessa forma.
fn check_no_overlap(segments: &[ProgramHeader]) -> Result<(), ElfError> {
    for (i, a) in segments.iter().enumerate() {
        let a_range = segment_page_range(a.vaddr, a.memsz);
        for b in &segments[i + 1..] {
            let b_range = segment_page_range(b.vaddr, b.memsz);
            if page_ranges_overlap(a_range, b_range) {
                return Err(ElfError::OverlappingSegments);
            }
        }
    }
    Ok(())
}

/// Mapeia e copia um segmento `PT_LOAD` já validado para a address space de
/// `root`, usando `frame_allocator` para as páginas novas. `file` é o
/// conteúdo completo do executável (o mesmo buffer já usado por
/// [`ElfHeader::parse`]/[`load_program_headers`]).
///
/// Cada página nova é explicitamente zerada via o alias HHDM do frame recém
/// mapeado antes de qualquer cópia — `Mapper::map_new` não zera o frame que
/// aloca (achado desta change, ver design.md, "Zeragem explícita do BSS").
/// Isso cobre tanto o preenchimento de BSS (`p_memsz > p_filesz`) quanto a
/// porção de uma página final que ultrapassa `p_filesz`.
///
/// Deve ser chamada só depois de [`check_no_overlap`] ter confirmado que
/// nenhum segmento deste executável colide com outro.
fn load_segment<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    root: u64,
    file: &[u8],
    segment: &ProgramHeader,
) -> Result<(), ElfError> {
    let (start, end) = segment_page_range(segment.vaddr, segment.memsz);
    let flags = segment_page_flags(segment.flags);
    let file_backed_end = segment.vaddr + segment.filesz;
    let hhdm_offset = hhdm::offset();

    let mut mapper = Mapper::for_root(frame_allocator, root);
    let mut virt = start;
    while virt < end {
        let frame = mapper.map_new(virt, flags, false).map_err(|_| ElfError::MapFailed)?;
        let dest = hhdm::phys_to_virt(hhdm_offset, frame) as *mut u8;
        unsafe { core::ptr::write_bytes(dest, 0, PAGE_SIZE as usize) };

        let page_end = virt + PAGE_SIZE;
        let copy_from = segment.vaddr.max(virt);
        let copy_to = file_backed_end.min(page_end);
        if copy_to > copy_from {
            let len = (copy_to - copy_from) as usize;
            let file_start = (segment.offset + (copy_from - segment.vaddr)) as usize;
            let page_offset = (copy_from - virt) as usize;
            let src = &file[file_start..file_start + len];
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dest.add(page_offset), len) };
        }

        virt = page_end;
    }
    Ok(())
}

/// Mapeia todos os segmentos `PT_LOAD` de `segments` (já extraídos por
/// [`load_program_headers`]) na address space de `root`, checando ausência
/// de sobreposição antes de mapear qualquer um deles — cobre "Mapeamento dos
/// Segmentos Carregáveis..." e "Segmentos carregáveis se sobrepõem" da spec.
/// Usada por `kernel::scheduler::spawn_from_elf` (ver design.md,
/// "`load_and_spawn` vive em `kernel::scheduler`").
pub fn load_segments<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    root: u64,
    file: &[u8],
    segments: &[ProgramHeader],
) -> Result<(), ElfError> {
    check_no_overlap(segments)?;
    for segment in segments {
        load_segment(frame_allocator, root, file, segment)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EI_CLASS: usize = 4;
    const EI_DATA: usize = 5;

    /// Monta um cabeçalho ELF64 válido (`ET_EXEC`/`EM_X86_64`) com `entry`,
    /// `phoff`, `phentsize`, `phnum` dados, em um buffer do tamanho pedido
    /// (preenchido com zeros além do cabeçalho).
    fn build_header(entry: u64, phoff: u64, phentsize: u16, phnum: u16, total_len: usize) -> Vec<u8> {
        let mut buf = alloc::vec![0u8; total_len.max(EHDR_SIZE)];
        buf[0..4].copy_from_slice(&ELF_MAGIC);
        buf[EI_CLASS] = ELFCLASS64;
        buf[EI_DATA] = ELFDATA2LSB;
        buf[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
        buf[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        buf[24..32].copy_from_slice(&entry.to_le_bytes());
        buf[32..40].copy_from_slice(&phoff.to_le_bytes());
        buf[54..56].copy_from_slice(&phentsize.to_le_bytes());
        buf[56..58].copy_from_slice(&phnum.to_le_bytes());
        buf
    }

    fn write_phdr(buf: &mut [u8], offset: usize, p_type: u32, p_flags: u32, p_offset: u64, p_vaddr: u64, p_filesz: u64, p_memsz: u64) {
        buf[offset..offset + 4].copy_from_slice(&p_type.to_le_bytes());
        buf[offset + 4..offset + 8].copy_from_slice(&p_flags.to_le_bytes());
        buf[offset + 8..offset + 16].copy_from_slice(&p_offset.to_le_bytes());
        buf[offset + 16..offset + 24].copy_from_slice(&p_vaddr.to_le_bytes());
        buf[offset + 32..offset + 40].copy_from_slice(&p_filesz.to_le_bytes());
        buf[offset + 40..offset + 48].copy_from_slice(&p_memsz.to_le_bytes());
    }

    // -------------------------------------------------------------
    // ElfHeader::parse (tarefa 1.2)
    // -------------------------------------------------------------

    #[test]
    fn valid_header_is_parsed() {
        let buf = build_header(0x1000, EHDR_SIZE as u64, PHDR_SIZE as u16, 1, EHDR_SIZE + PHDR_SIZE);
        let header = ElfHeader::parse(&buf).expect("cabecalho valido deveria ser aceito");
        assert_eq!(header.entry, 0x1000);
        assert_eq!(header.phoff, EHDR_SIZE as u64);
        assert_eq!(header.phentsize, PHDR_SIZE as u16);
        assert_eq!(header.phnum, 1);
    }

    #[test]
    fn buffer_shorter_than_header_is_rejected() {
        let buf = [0u8; EHDR_SIZE - 1];
        assert_eq!(ElfHeader::parse(&buf), Err(ElfError::NotElf));
    }

    #[test]
    fn wrong_magic_is_rejected() {
        let mut buf = build_header(0, 0, 0, 0, EHDR_SIZE);
        buf[0] = 0x00;
        assert_eq!(ElfHeader::parse(&buf), Err(ElfError::NotElf));
    }

    #[test]
    fn wrong_class_is_rejected() {
        let mut buf = build_header(0, 0, 0, 0, EHDR_SIZE);
        buf[EI_CLASS] = 1; // ELFCLASS32
        assert_eq!(ElfHeader::parse(&buf), Err(ElfError::UnsupportedExecutable));
    }

    #[test]
    fn wrong_endianness_is_rejected() {
        let mut buf = build_header(0, 0, 0, 0, EHDR_SIZE);
        buf[EI_DATA] = 2; // ELFDATA2MSB
        assert_eq!(ElfHeader::parse(&buf), Err(ElfError::UnsupportedExecutable));
    }

    #[test]
    fn wrong_machine_is_rejected() {
        let mut buf = build_header(0, 0, 0, 0, EHDR_SIZE);
        buf[18..20].copy_from_slice(&0u16.to_le_bytes()); // EM_NONE
        assert_eq!(ElfHeader::parse(&buf), Err(ElfError::UnsupportedExecutable));
    }

    #[test]
    fn wrong_type_is_rejected() {
        let mut buf = build_header(0, 0, 0, 0, EHDR_SIZE);
        buf[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        assert_eq!(ElfHeader::parse(&buf), Err(ElfError::UnsupportedExecutable));
    }

    // -------------------------------------------------------------
    // load_program_headers (tarefa 1.3)
    // -------------------------------------------------------------

    #[test]
    fn single_pt_load_segment_is_extracted() {
        let phoff = EHDR_SIZE as u64;
        let mut buf = build_header(0, phoff, PHDR_SIZE as u16, 1, EHDR_SIZE + PHDR_SIZE);
        write_phdr(&mut buf, EHDR_SIZE, PT_LOAD, PF_X, 0, 0x1000, 16, 16);
        let header = ElfHeader::parse(&buf).unwrap();

        let segments = load_program_headers(&buf, &header).expect("segmentos validos deveriam ser aceitos");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].vaddr, 0x1000);
        assert_eq!(segments[0].flags, PF_X);
    }

    #[test]
    fn non_pt_load_entries_are_ignored() {
        let phoff = EHDR_SIZE as u64;
        let mut buf = build_header(0, phoff, PHDR_SIZE as u16, 2, EHDR_SIZE + 2 * PHDR_SIZE);
        write_phdr(&mut buf, EHDR_SIZE, 6 /* PT_PHDR */, 0, 0, 0, 0, 0);
        write_phdr(&mut buf, EHDR_SIZE + PHDR_SIZE, PT_LOAD, PF_W, 0, 0x2000, 8, 8);
        let header = ElfHeader::parse(&buf).unwrap();

        let segments = load_program_headers(&buf, &header).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].vaddr, 0x2000);
    }

    #[test]
    fn program_header_table_out_of_bounds_is_rejected() {
        let buf = build_header(0, EHDR_SIZE as u64, PHDR_SIZE as u16, 5, EHDR_SIZE); // declara 5 entradas, buffer so tem o cabecalho
        let header = ElfHeader::parse(&buf).unwrap();
        assert_eq!(load_program_headers(&buf, &header), Err(ElfError::ProgramHeaderTableOutOfBounds));
    }

    #[test]
    fn segment_offset_past_end_of_file_is_rejected() {
        let phoff = EHDR_SIZE as u64;
        let mut buf = build_header(0, phoff, PHDR_SIZE as u16, 1, EHDR_SIZE + PHDR_SIZE);
        write_phdr(&mut buf, EHDR_SIZE, PT_LOAD, PF_R_FLAG, 1000, 0x1000, 16, 16);
        let header = ElfHeader::parse(&buf).unwrap();
        assert_eq!(load_program_headers(&buf, &header), Err(ElfError::InvalidSegment));
    }

    #[test]
    fn memsz_smaller_than_filesz_is_rejected() {
        let phoff = EHDR_SIZE as u64;
        let mut buf = build_header(0, phoff, PHDR_SIZE as u16, 1, EHDR_SIZE + PHDR_SIZE);
        write_phdr(&mut buf, EHDR_SIZE, PT_LOAD, PF_R_FLAG, 0, 0x1000, 32, 16);
        let header = ElfHeader::parse(&buf).unwrap();
        assert_eq!(load_program_headers(&buf, &header), Err(ElfError::InvalidSegment));
    }

    const PF_R_FLAG: u32 = 4;

    // -------------------------------------------------------------
    // segment_page_range / segment_page_flags / page_ranges_overlap (tarefa 2.1)
    // -------------------------------------------------------------

    #[test]
    fn page_range_for_segment_within_a_single_page() {
        assert_eq!(segment_page_range(0x1000, 16), (0x1000, 0x2000));
    }

    #[test]
    fn page_range_truncates_down_and_rounds_up() {
        // vaddr no meio de uma pagina, estendendo-se ate o meio da proxima.
        assert_eq!(segment_page_range(0x1800, 0x1000), (0x1000, 0x3000));
    }

    #[test]
    fn page_range_exactly_at_page_boundaries_does_not_add_extra_page() {
        assert_eq!(segment_page_range(0x1000, 0x1000), (0x1000, 0x2000));
    }

    #[test]
    fn page_flags_read_execute_segment() {
        let flags = segment_page_flags(PF_R_FLAG | PF_X);
        assert!(!flags.writable);
        assert!(flags.executable);
        assert!(flags.user_accessible);
    }

    #[test]
    fn page_flags_read_write_segment() {
        let flags = segment_page_flags(PF_R_FLAG | PF_W);
        assert!(flags.writable);
        assert!(!flags.executable);
    }

    #[test]
    fn page_flags_read_only_segment() {
        let flags = segment_page_flags(PF_R_FLAG);
        assert!(!flags.writable);
        assert!(!flags.executable);
    }

    #[test]
    fn disjoint_ranges_do_not_overlap() {
        assert!(!page_ranges_overlap((0x1000, 0x2000), (0x2000, 0x3000)));
    }

    #[test]
    fn overlapping_ranges_are_detected() {
        assert!(page_ranges_overlap((0x1000, 0x3000), (0x2000, 0x4000)));
    }

    #[test]
    fn identical_ranges_overlap() {
        assert!(page_ranges_overlap((0x1000, 0x2000), (0x1000, 0x2000)));
    }

    // -------------------------------------------------------------
    // check_no_overlap (tarefa 2.3)
    // -------------------------------------------------------------

    fn segment(vaddr: u64, memsz: u64) -> ProgramHeader {
        ProgramHeader { flags: PF_R_FLAG, offset: 0, vaddr, memsz, filesz: 0 }
    }

    #[test]
    fn non_overlapping_segments_are_accepted() {
        let segments = [segment(0x1000, 0x1000), segment(0x2000, 0x1000)];
        assert_eq!(check_no_overlap(&segments), Ok(()));
    }

    #[test]
    fn overlapping_segments_are_rejected_before_any_mapping() {
        let segments = [segment(0x1000, 0x2000), segment(0x1800, 0x1000)];
        assert_eq!(check_no_overlap(&segments), Err(ElfError::OverlappingSegments));
    }
}
