//! Carregamento de bibliotecas compartilhadas ELF64 (`ET_DYN`) — ver spec
//! `kernel-dynamic-linker` e `design.md` da change `kernel-dynamic-linker`.
//!
//! Mesmo padrão de separação do resto do kernel (`kernel::gpt`/
//! `kernel::fat32`, `kernel::elf`): parsing puro da seção dinâmica e da
//! tabela de símbolos/relocações (sem nenhum acesso a memória do processo
//! destino, testável com `cargo test -p kernel --lib` sem hardware) e uma
//! camada real (`load_library`) que mapeia segmentos e escreve relocações,
//! só verificável em boot real. Reaproveita `kernel::elf` para o
//! cabeçalho/`program headers`/mapeamento de segmentos em vez de duplicar
//! esse parsing — ver design.md, "Reaproveitar `kernel::elf`...".

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::elf::{ElfHeader, ProgramHeader};
use crate::phys::{FrameAllocator, FrameStore};
use crate::sync::Spinlock;

/// Tamanho, em bytes, de um program header ELF64 nos campos lidos aqui —
/// mesmo valor de `kernel::elf::PHDR_SIZE`, duplicado deliberadamente (ver
/// design.md, "Localizar o segmento `PT_DYNAMIC`") para não alargar a API
/// pública de `kernel::elf` por conta de um único chamador.
const PHDR_SIZE: usize = 56;
/// `p_type` do segmento que descreve a tabela `.dynamic`.
const PT_DYNAMIC: u32 = 2;

/// Tamanho, em bytes, de uma entrada `Elf64_Dyn` (`d_tag`/`d_val`).
const DYN_ENTRY_SIZE: usize = 16;
/// Tamanho, em bytes, de uma entrada `Elf64_Sym`.
const SYM_ENTRY_SIZE: usize = 24;
/// Tamanho, em bytes, de uma entrada `Elf64_Rela` (com `r_addend` explícito
/// — formato usado por este linker para `DT_RELA`/`DT_JMPREL`).
const RELA_ENTRY_SIZE: usize = 24;

// Tags de `Elf64_Dyn` relevantes — ver design.md para o inventário completo.
const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_PLTRELSZ: u64 = 2;
const DT_HASH: u64 = 4;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_RELA: u64 = 7;
const DT_RELASZ: u64 = 8;
const DT_STRSZ: u64 = 10;
const DT_PLTREL: u64 = 20;
const DT_JMPREL: u64 = 23;

/// Tipos de relocação x86-64 suportados — ver design.md, "Resolução de
/// relocação: fórmula uniforme".
const R_X86_64_64: u32 = 1;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DylibError {
    /// O ELF não é válido, ou é válido mas não `ET_DYN`, ou de arquitetura
    /// não suportada — devolvido por `kernel::elf::ElfHeader::parse` /
    /// checagem de tipo.
    UnsupportedLibrary,
    /// A tabela de program headers aponta, ao menos em parte, para fora dos
    /// limites do arquivo (mesma checagem de `kernel::elf`, duplicada aqui
    /// para `find_dynamic_segment`).
    ProgramHeaderTableOutOfBounds,
    /// Nenhum program header do tipo `PT_DYNAMIC` foi encontrado — este
    /// `ET_DYN` não tem seção dinâmica (malformado para os fins deste
    /// linker).
    MissingDynamicSegment,
    /// O segmento `PT_DYNAMIC` aponta para fora dos limites do arquivo.
    DynamicSegmentOutOfBounds,
    /// Uma tag obrigatória (`DT_STRTAB`/`DT_SYMTAB`/`DT_HASH`/`DT_STRSZ`)
    /// está ausente da tabela `.dynamic`.
    MissingRequiredTag,
    /// `DT_PLTREL` está presente com um valor diferente de `DT_RELA` — só
    /// relocações de PLT no formato RELA (com `addend` explícito) são
    /// suportadas.
    UnsupportedPltRelType,
    /// Um endereço virtual (`DT_STRTAB`/`DT_SYMTAB`/`DT_HASH`/`DT_RELA`/
    /// `DT_JMPREL`, ou o `r_offset`/`st_name` de uma entrada) não cai dentro
    /// de nenhum segmento `PT_LOAD` do arquivo, ou o deslocamento no
    /// arquivo resultante excede o tamanho do buffer.
    AddressOutOfBounds,
    /// Uma string C dentro de `.dynstr` não tem um terminador `\0` antes do
    /// fim do buffer.
    UnterminatedString,
    /// O tipo de uma relocação não é um dos suportados (ver design.md,
    /// Non-Goals — TLS e outros tipos não são implementados).
    UnsupportedRelocationType(u32),
    /// Uma relocação referencia um símbolo que não foi encontrado em
    /// nenhuma biblioteca já carregada no processo.
    UnresolvedSymbol,
    /// O disco de armazenamento global não está inicializado — ver
    /// `kernel::storage::with_disk`.
    NoDisk,
    /// Falha ao ler o arquivo da biblioteca do sistema de arquivos (ou de
    /// uma dependência `DT_NEEDED`) — caminho não encontrado, não é um
    /// arquivo, ou falha de leitura de disco.
    FileRead(crate::fat32::Fat32Error),
    /// Reaproveita `kernel::elf` para o cabeçalho/`program headers`/
    /// mapeamento de segmentos — qualquer falha desses passos (formato
    /// inválido, segmento malformado/sobreposto, falta de memória) é
    /// propagada aqui.
    ElfParsing(crate::elf::ElfError),
    /// Um caminho já está sendo carregado na mesma árvore de dependências
    /// desta chamada (`DT_NEEDED` circular: A depende de B que depende de
    /// A) — rejeitado antes de causar recursão infinita.
    CircularDependency,
    /// Os segmentos `PT_LOAD` da biblioteca (depois do rebase) excedem o
    /// slab de endereço virtual reservado para ela — ver design.md,
    /// "Riscos — slab fixo".
    LibraryTooLarge,
    /// `handle` não corresponde a nenhuma biblioteca carregada no espaço de
    /// endereçamento do chamador.
    InvalidHandle,
    /// Nenhum símbolo com o nome pedido é exportado pela biblioteca
    /// referenciada pelo `handle` — usado por `dlsym` (distinto de
    /// `UnresolvedSymbol`, que é uma falha durante o carregamento/
    /// relocação).
    SymbolNotFound,
}

/// Localiza o primeiro program header do tipo `PT_DYNAMIC` e devolve o
/// intervalo de bytes (`p_offset..p_offset+p_filesz`) que ele ocupa no
/// arquivo — cobre a localização da seção `.dynamic` para "Carregamento de
/// Biblioteca Compartilhada..." da spec. Mesmos limites verificados de
/// `kernel::elf::load_program_headers`, nunca indexa `buf` fora dos limites.
pub(crate) fn find_dynamic_segment(buf: &[u8], header: &ElfHeader) -> Result<(usize, usize), DylibError> {
    let phentsize = header.phentsize as usize;
    let phnum = header.phnum as usize;
    let table_len = phentsize.checked_mul(phnum).ok_or(DylibError::ProgramHeaderTableOutOfBounds)?;
    let table_start = usize::try_from(header.phoff).map_err(|_| DylibError::ProgramHeaderTableOutOfBounds)?;
    let table_end = table_start.checked_add(table_len).ok_or(DylibError::ProgramHeaderTableOutOfBounds)?;
    let table = buf.get(table_start..table_end).ok_or(DylibError::ProgramHeaderTableOutOfBounds)?;

    for entry in table.chunks_exact(phentsize) {
        let raw = entry.get(0..PHDR_SIZE).ok_or(DylibError::ProgramHeaderTableOutOfBounds)?;
        let p_type = u32::from_le_bytes(raw[0..4].try_into().unwrap());
        if p_type != PT_DYNAMIC {
            continue;
        }
        let p_offset = u64::from_le_bytes(raw[8..16].try_into().unwrap());
        let p_filesz = u64::from_le_bytes(raw[32..40].try_into().unwrap());
        let start = usize::try_from(p_offset).map_err(|_| DylibError::DynamicSegmentOutOfBounds)?;
        let len = usize::try_from(p_filesz).map_err(|_| DylibError::DynamicSegmentOutOfBounds)?;
        let end = start.checked_add(len).ok_or(DylibError::DynamicSegmentOutOfBounds)?;
        if end > buf.len() {
            return Err(DylibError::DynamicSegmentOutOfBounds);
        }
        return Ok((start, end));
    }
    Err(DylibError::MissingDynamicSegment)
}

/// Encontra o segmento `PT_LOAD` **original** (sem rebase — `vaddr`
/// relativo à base hipotética 0 de um `ET_DYN`) que contém `vaddr`, e
/// devolve o deslocamento correspondente no arquivo. Necessário porque os
/// valores de `DT_STRTAB`/`DT_SYMTAB`/`DT_HASH`/`DT_RELA`/`DT_JMPREL` (e os
/// campos que referenciam endereço dentro de `.dynsym`/relocações) são
/// endereços virtuais de link-time, não deslocamentos de arquivo — mas o
/// parsing desta change lê tudo do buffer do arquivo em memória do kernel,
/// nunca de memória mapeada.
pub(crate) fn vaddr_to_file_offset(segments: &[ProgramHeader], vaddr: u64) -> Option<usize> {
    for segment in segments {
        let file_backed_end = segment.vaddr + segment.filesz;
        if vaddr >= segment.vaddr && vaddr < file_backed_end {
            let offset_in_segment = vaddr - segment.vaddr;
            return usize::try_from(segment.offset + offset_in_segment).ok();
        }
    }
    None
}

fn vaddr_range_to_file_range(segments: &[ProgramHeader], vaddr: u64, len: u64) -> Result<(usize, usize), DylibError> {
    let start = vaddr_to_file_offset(segments, vaddr).ok_or(DylibError::AddressOutOfBounds)?;
    let len = usize::try_from(len).map_err(|_| DylibError::AddressOutOfBounds)?;
    let end = start.checked_add(len).ok_or(DylibError::AddressOutOfBounds)?;
    Ok((start, end))
}

/// Campos da tabela `.dynamic` relevantes para carregar uma biblioteca —
/// endereços virtuais (link-time, base 0), ainda não traduzidos para
/// deslocamento de arquivo. Lógica pura, sem nenhum acesso a memória do
/// processo destino.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct DynamicInfo {
    /// Deslocamentos em `.dynstr` de cada nome declarado via `DT_NEEDED`.
    pub needed_name_offsets: Vec<u64>,
    pub strtab_vaddr: u64,
    pub strsz: u64,
    pub symtab_vaddr: u64,
    pub hash_vaddr: u64,
    /// `(vaddr, tamanho_em_bytes)` de `DT_RELA`/`DT_RELASZ`, se presentes.
    pub rela: Option<(u64, u64)>,
    /// `(vaddr, tamanho_em_bytes)` de `DT_JMPREL`/`DT_PLTRELSZ`, se
    /// presentes. `DT_PLTREL` já validado como `DT_RELA` nesse caso.
    pub jmprel: Option<(u64, u64)>,
}

/// Interpreta `buf[dyn_start..dyn_end]` (o intervalo devolvido por
/// [`find_dynamic_segment`]) como uma tabela `.dynamic` (entradas de 16
/// bytes, `d_tag`/`d_val`, terminada em `DT_NULL`) — cobre "Carregamento de
/// Biblioteca Compartilhada..." da spec. Rejeita com `DylibError` qualquer
/// tag obrigatória ausente (`DT_STRTAB`/`DT_SYMTAB`/`DT_HASH`/`DT_STRSZ`) e
/// qualquer `DT_PLTREL` diferente de `DT_RELA`.
pub(crate) fn parse_dynamic_section(buf: &[u8], dyn_start: usize, dyn_end: usize) -> Result<DynamicInfo, DylibError> {
    let table = buf.get(dyn_start..dyn_end).ok_or(DylibError::DynamicSegmentOutOfBounds)?;

    let mut needed_name_offsets = Vec::new();
    let mut strtab_vaddr = None;
    let mut strsz = None;
    let mut symtab_vaddr = None;
    let mut hash_vaddr = None;
    let mut rela_vaddr = None;
    let mut relasz = None;
    let mut jmprel_vaddr = None;
    let mut pltrelsz = None;
    let mut pltrel = None;

    for entry in table.chunks_exact(DYN_ENTRY_SIZE) {
        let d_tag = u64::from_le_bytes(entry[0..8].try_into().unwrap());
        let d_val = u64::from_le_bytes(entry[8..16].try_into().unwrap());
        match d_tag {
            DT_NULL => break,
            DT_NEEDED => needed_name_offsets.push(d_val),
            DT_STRTAB => strtab_vaddr = Some(d_val),
            DT_STRSZ => strsz = Some(d_val),
            DT_SYMTAB => symtab_vaddr = Some(d_val),
            DT_HASH => hash_vaddr = Some(d_val),
            DT_RELA => rela_vaddr = Some(d_val),
            DT_RELASZ => relasz = Some(d_val),
            DT_JMPREL => jmprel_vaddr = Some(d_val),
            DT_PLTRELSZ => pltrelsz = Some(d_val),
            DT_PLTREL => pltrel = Some(d_val),
            _ => {}
        }
    }

    if let Some(pltrel) = pltrel {
        if pltrel != DT_RELA {
            return Err(DylibError::UnsupportedPltRelType);
        }
    }

    let rela = match (rela_vaddr, relasz) {
        (Some(v), Some(sz)) => Some((v, sz)),
        _ => None,
    };
    let jmprel = match (jmprel_vaddr, pltrelsz) {
        (Some(v), Some(sz)) => Some((v, sz)),
        _ => None,
    };

    Ok(DynamicInfo {
        needed_name_offsets,
        strtab_vaddr: strtab_vaddr.ok_or(DylibError::MissingRequiredTag)?,
        strsz: strsz.ok_or(DylibError::MissingRequiredTag)?,
        symtab_vaddr: symtab_vaddr.ok_or(DylibError::MissingRequiredTag)?,
        hash_vaddr: hash_vaddr.ok_or(DylibError::MissingRequiredTag)?,
        rela,
        jmprel,
    })
}

/// Lê uma string C (terminada em `\0`) dentro de `dynstr` a partir de
/// `offset` — cobre a resolução de nomes de `DT_NEEDED`/símbolos. Rejeita
/// com `DylibError::UnterminatedString` se nenhum `\0` for encontrado antes
/// do fim de `dynstr`, nunca lê além dele.
pub(crate) fn cstr_at(dynstr: &[u8], offset: u32) -> Result<&str, DylibError> {
    let start = offset as usize;
    let rest = dynstr.get(start..).ok_or(DylibError::AddressOutOfBounds)?;
    let len = rest.iter().position(|&b| b == 0).ok_or(DylibError::UnterminatedString)?;
    core::str::from_utf8(&rest[..len]).map_err(|_| DylibError::UnterminatedString)
}

/// Resolve cada deslocamento de [`DynamicInfo::needed_name_offsets`] para o
/// nome declarado em `DT_NEEDED` — cobre "Carregamento de Dependências
/// Declaradas..." da spec.
pub(crate) fn resolve_needed_names(dynstr: &[u8], info: &DynamicInfo) -> Result<Vec<String>, DylibError> {
    info.needed_name_offsets
        .iter()
        .map(|&off| {
            let off = u32::try_from(off).map_err(|_| DylibError::AddressOutOfBounds)?;
            cstr_at(dynstr, off).map(String::from)
        })
        .collect()
}

/// Um símbolo de `.dynsym`, ainda não resolvido a um nome (`st_name` é só o
/// deslocamento em `.dynstr`) nem a um endereço final (`st_value` é
/// relativo à base 0, antes de somar a base de carga real).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) struct DynSymbol {
    pub st_name: u32,
    pub st_value: u64,
    pub st_shndx: u16,
}

/// Lê os dois primeiros `u32` de `DT_HASH` (`nbucket`, `nchain`) e devolve
/// `nchain` — por invariante da especificação ELF, igual ao número de
/// entradas de `.dynsym`. Usado só para dimensionar a tabela de símbolos,
/// nunca para busca por hash de verdade (buckets/chains são ignorados) —
/// ver design.md, "Contagem de símbolos via `DT_HASH`". Uma biblioteca
/// linkada só com `DT_GNU_HASH` (sem `DT_HASH` clássico) já teria sido
/// rejeitada antes (`DynamicInfo::hash_vaddr` é obrigatório).
pub(crate) fn symbol_count_from_hash(buf: &[u8], hash_file_offset: usize) -> Result<u32, DylibError> {
    let header = buf.get(hash_file_offset..hash_file_offset + 8).ok_or(DylibError::AddressOutOfBounds)?;
    let nchain = u32::from_le_bytes(header[4..8].try_into().unwrap());
    Ok(nchain)
}

/// Interpreta `count` entradas `Elf64_Sym` (24 bytes cada) a partir de
/// `symtab_file_offset` — cobre "Mapeamento dos Segmentos..."/resolução de
/// símbolo da spec. Rejeita com `DylibError` qualquer leitura que
/// ultrapasse os limites de `buf`, nunca indexa fora deles.
pub(crate) fn parse_dynsym(buf: &[u8], symtab_file_offset: usize, count: u32) -> Result<Vec<DynSymbol>, DylibError> {
    let total_len = (count as usize).checked_mul(SYM_ENTRY_SIZE).ok_or(DylibError::AddressOutOfBounds)?;
    let end = symtab_file_offset.checked_add(total_len).ok_or(DylibError::AddressOutOfBounds)?;
    let table = buf.get(symtab_file_offset..end).ok_or(DylibError::AddressOutOfBounds)?;

    let mut symbols = Vec::with_capacity(count as usize);
    for entry in table.chunks_exact(SYM_ENTRY_SIZE) {
        let st_name = u32::from_le_bytes(entry[0..4].try_into().unwrap());
        let st_shndx = u16::from_le_bytes(entry[6..8].try_into().unwrap());
        let st_value = u64::from_le_bytes(entry[8..16].try_into().unwrap());
        symbols.push(DynSymbol { st_name, st_value, st_shndx });
    }
    Ok(symbols)
}

/// Extrai, de `symbols`, os que são "exportados" por esta biblioteca —
/// `st_name != 0` (tem nome) e `st_shndx != 0`/`SHN_UNDEF` (definido aqui,
/// não é uma referência externa) — com o nome resolvido via `dynstr` e o
/// endereço final somado a `base`. Não distingue vinculação/visibilidade
/// (`GLOBAL`/`WEAK`/`LOCAL`, `DEFAULT`/`HIDDEN`) — ver design.md,
/// "Exportação de símbolo: qualquer entrada nomeada e definida".
pub(crate) fn exported_symbols(dynstr: &[u8], symbols: &[DynSymbol], base: u64) -> Result<Vec<(String, u64)>, DylibError> {
    symbols
        .iter()
        .filter(|s| s.st_name != 0 && s.st_shndx != 0)
        .map(|s| cstr_at(dynstr, s.st_name).map(|name| (String::from(name), s.st_value + base)))
        .collect()
}

/// Uma relocação `Elf64_Rela` já decomposta: `r_info` separado em tipo de
/// relocação e índice de símbolo (`.dynsym`).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) struct Relocation {
    pub offset: u64,
    pub reloc_type: u32,
    pub symbol_index: u32,
    pub addend: i64,
}

/// Interpreta `buf[file_start..file_end]` como uma tabela de relocações
/// `Elf64_Rela` (entradas de 24 bytes: `r_offset`, `r_info`, `r_addend`) —
/// usada tanto para `DT_RELA` quanto para `DT_JMPREL` (ambas no mesmo
/// formato, já que `DT_PLTREL` é validado como `DT_RELA` em
/// [`parse_dynamic_section`]). Rejeita com `DylibError` qualquer leitura
/// fora dos limites de `buf`.
pub(crate) fn parse_relocations(buf: &[u8], file_start: usize, file_end: usize) -> Result<Vec<Relocation>, DylibError> {
    let table = buf.get(file_start..file_end).ok_or(DylibError::AddressOutOfBounds)?;
    if table.len() % RELA_ENTRY_SIZE != 0 {
        return Err(DylibError::AddressOutOfBounds);
    }

    let mut relocations = Vec::with_capacity(table.len() / RELA_ENTRY_SIZE);
    for entry in table.chunks_exact(RELA_ENTRY_SIZE) {
        let r_offset = u64::from_le_bytes(entry[0..8].try_into().unwrap());
        let r_info = u64::from_le_bytes(entry[8..16].try_into().unwrap());
        let r_addend = i64::from_le_bytes(entry[16..24].try_into().unwrap());
        let reloc_type = (r_info & 0xffff_ffff) as u32;
        let symbol_index = (r_info >> 32) as u32;
        relocations.push(Relocation { offset: r_offset, reloc_type, symbol_index, addend: r_addend });
    }
    Ok(relocations)
}

/// Resolve o valor final de uma relocação já parseada — cobre "Resolução e
/// Aplicação de Relocações ao Carregar" da spec. `R_X86_64_RELATIVE` não
/// consulta `resolve_symbol` (relativo só à base de carga); os demais tipos
/// suportados (`GLOB_DAT`/`JUMP_SLOT`/`R_X86_64_64`) resolvem o símbolo pelo
/// índice e somam o `addend` — ver design.md, "Resolução de relocação:
/// fórmula uniforme". Qualquer outro tipo é rejeitado explicitamente, nunca
/// ignorado silenciosamente.
pub(crate) fn resolve_relocation_value(
    reloc: &Relocation,
    base: u64,
    resolve_symbol: impl Fn(u32) -> Option<u64>,
) -> Result<u64, DylibError> {
    match reloc.reloc_type {
        R_X86_64_RELATIVE => Ok(base.wrapping_add(reloc.addend as u64)),
        R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT | R_X86_64_64 => {
            let symbol_value = resolve_symbol(reloc.symbol_index).ok_or(DylibError::UnresolvedSymbol)?;
            Ok(symbol_value.wrapping_add(reloc.addend as u64))
        }
        other => Err(DylibError::UnsupportedRelocationType(other)),
    }
}

/// Endereço de base da primeira biblioteca carregada em qualquer espaço de
/// endereçamento — distinto das demais faixas fixas já usadas pelo kernel
/// (`USER_DEMO_CODE_VIRT`/`...STACK_VIRT` etc. em `kernel::scheduler`, todas
/// abaixo de `0x0000_6000_0000_0000`).
const DYLIB_BASE_START: u64 = 0x0000_6000_0000_0000;
/// Tamanho fixo reservado para cada biblioteca carregada (bump allocator
/// sem reclamação — ver design.md, "Riscos — slab fixo"). Generoso para as
/// bibliotecas de teste desta change; revisitar quando uma biblioteca real
/// e grande precisar carregar.
const DYLIB_SLAB_SIZE: u64 = 16 * 1024 * 1024;

/// Uma biblioteca já carregada num espaço de endereçamento: seu endereço de
/// base e os símbolos que exporta (nome já resolvido, endereço final já
/// somado à base).
pub(crate) struct LoadedLibrary {
    pub base: u64,
    pub symbols: Vec<(String, u64)>,
}

/// Bibliotecas carregadas e o próximo endereço de base livre para um único
/// espaço de endereçamento (processo) — ver design.md, "Estado por espaço
/// de endereçamento via `current_root()`".
pub(crate) struct ProcessDylibState {
    pub libraries: Vec<LoadedLibrary>,
    next_base: u64,
}

impl ProcessDylibState {
    fn new() -> Self {
        Self { libraries: Vec::new(), next_base: DYLIB_BASE_START }
    }
}

/// Tabela global de estado de bibliotecas, chaveada pelo `root` (CR3 físico,
/// `kernel::paging::current_root()`) do espaço de endereçamento chamador —
/// nunca pelo escalonador. Mesmo padrão de "tabela que só cresce" de
/// `kernel::pthread_handles`, com uma camada a mais de chave para isolar
/// processos entre si — cobre "Isolamento de Bibliotecas Carregadas entre
/// Processos Diferentes" da spec.
pub(crate) static DYLIB_STATE: Spinlock<BTreeMap<u64, ProcessDylibState>> = Spinlock::new(BTreeMap::new());

/// Resolve um nome de símbolo contra as bibliotecas já carregadas em
/// `root`, na ordem em que foram carregadas — usada tanto durante a
/// aplicação de relocações (tarefa 4.2) quanto por `dlsym`
/// (`kernel::syscall::sys_dlsym`).
pub(crate) fn resolve_symbol_by_name(root: u64, name: &str) -> Option<u64> {
    let state = DYLIB_STATE.lock();
    let entry = state.get(&root)?;
    entry.libraries.iter().find_map(|lib| lib.symbols.iter().find(|(n, _)| n == name).map(|(_, addr)| *addr))
}

/// Resolve `name` entre os símbolos exportados pela biblioteca `handle` do
/// espaço de endereçamento `root` — cobre "Resolução de Símbolo por Nome a
/// partir de um Handle de Biblioteca" da spec; usada por
/// `kernel::syscall::sys_dlsym`, que colapsa `InvalidHandle`/`SymbolNotFound`
/// no mesmo sentinela `SYSCALL_ERROR`.
pub(crate) fn dlsym(root: u64, handle: u64, name: &str) -> Result<u64, DylibError> {
    let state = DYLIB_STATE.lock();
    let process_state = state.get(&root).ok_or(DylibError::InvalidHandle)?;
    let library = process_state.libraries.get(handle as usize).ok_or(DylibError::InvalidHandle)?;
    library.symbols.iter().find(|(n, _)| n == name).map(|(_, addr)| *addr).ok_or(DylibError::SymbolNotFound)
}

/// Carrega a biblioteca compartilhada ELF64 (`ET_DYN`) em `path` no espaço
/// de endereçamento `root`, carregando recursivamente suas dependências
/// (`DT_NEEDED`, resolvidas em `/lib/<nome>`) antes de aplicar suas próprias
/// relocações — cobre "Carregamento de Biblioteca Compartilhada...",
/// "Carregamento de Dependências Declaradas..." e "Resolução e Aplicação de
/// Relocações..." da spec `kernel-dynamic-linker`. `loading_stack` detecta
/// dependência circular (ver design.md) — o chamador de topo
/// (`kernel::syscall::sys_dlopen`) sempre passa um `Vec` novo e vazio.
///
/// Devolve o índice da biblioteca em `ProcessDylibState::libraries` (usado
/// como `handle` opaco por `dlopen`). Nenhuma falha em qualquer etapa deixa
/// uma biblioteca parcialmente registrada: o `push` final só acontece depois
/// de segmentos mapeados, dependências carregadas e todas as relocações
/// aplicadas com sucesso (tarefa 4.3).
pub(crate) fn load_library<S: FrameStore>(
    frame_allocator: &mut FrameAllocator<S>,
    root: u64,
    fs: &crate::fat32::Fat32Filesystem,
    path: &str,
    loading_stack: &mut Vec<String>,
) -> Result<usize, DylibError> {
    if loading_stack.iter().any(|p| p.as_str() == path) {
        return Err(DylibError::CircularDependency);
    }

    let file = crate::storage::with_disk(|disk| fs.read_file(disk, path))
        .ok_or(DylibError::NoDisk)?
        .map_err(DylibError::FileRead)?;

    let header = ElfHeader::parse(&file).map_err(DylibError::ElfParsing)?;
    if header.e_type != crate::elf::ET_DYN {
        return Err(DylibError::UnsupportedLibrary);
    }
    let segments = crate::elf::load_program_headers(&file, &header).map_err(DylibError::ElfParsing)?;

    let base = {
        let mut state = DYLIB_STATE.lock();
        let entry = state.entry(root).or_insert_with(ProcessDylibState::new);
        let base = entry.next_base;
        entry.next_base += DYLIB_SLAB_SIZE;
        base
    };

    if segments.iter().any(|s| s.vaddr + s.memsz > DYLIB_SLAB_SIZE) {
        return Err(DylibError::LibraryTooLarge);
    }

    let rebased = crate::elf::rebase_segments(&segments, base);
    crate::elf::load_segments(frame_allocator, root, &file, &rebased).map_err(DylibError::ElfParsing)?;

    let (dyn_start, dyn_end) = find_dynamic_segment(&file, &header)?;
    let info = parse_dynamic_section(&file, dyn_start, dyn_end)?;

    let strtab_offset = vaddr_to_file_offset(&segments, info.strtab_vaddr).ok_or(DylibError::AddressOutOfBounds)?;
    let strsz = usize::try_from(info.strsz).map_err(|_| DylibError::AddressOutOfBounds)?;
    let dynstr = file.get(strtab_offset..strtab_offset + strsz).ok_or(DylibError::AddressOutOfBounds)?;

    let hash_offset = vaddr_to_file_offset(&segments, info.hash_vaddr).ok_or(DylibError::AddressOutOfBounds)?;
    let symbol_count = symbol_count_from_hash(&file, hash_offset)?;

    let symtab_offset = vaddr_to_file_offset(&segments, info.symtab_vaddr).ok_or(DylibError::AddressOutOfBounds)?;
    let symbols = parse_dynsym(&file, symtab_offset, symbol_count)?;

    // Dependências primeiro: seus símbolos precisam estar em `DYLIB_STATE`
    // antes de qualquer relocação desta biblioteca ser resolvida.
    let needed_names = resolve_needed_names(dynstr, &info)?;
    loading_stack.push(String::from(path));
    for name in &needed_names {
        let dep_path = format!("/lib/{}", name);
        let result = load_library(frame_allocator, root, fs, &dep_path, loading_stack);
        if result.is_err() {
            loading_stack.pop();
            return result;
        }
    }
    loading_stack.pop();

    // Calculado antes de aplicar relocações (e antes do registro em
    // `DYLIB_STATE`, que só acontece no fim): uma relocação desta própria
    // biblioteca pode referenciar um símbolo que ela mesma exporta (comum
    // mesmo para auto-referências — o compilador emite `R_X86_64_64` em vez
    // de `R_X86_64_RELATIVE` sempre que o símbolo referenciado é `GLOBAL`,
    // já que ele é, em tese, interponível, independente de já estar
    // definido no próprio objeto). Sem isso, `resolve` só enxergaria
    // bibliotecas carregadas *antes* desta, nunca os próprios símbolos —
    // achado ao depurar a verificação em boot real (ver design.md,
    // "Achados durante a implementação").
    let exported = exported_symbols(dynstr, &symbols, base)?;

    let resolve = |symbol_index: u32| -> Option<u64> {
        let symbol = symbols.get(symbol_index as usize)?;
        let name = cstr_at(dynstr, symbol.st_name).ok()?;
        exported
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, addr)| *addr)
            .or_else(|| resolve_symbol_by_name(root, name))
    };

    for (vaddr, size) in [info.rela, info.jmprel].into_iter().flatten() {
        let (start, end) = vaddr_range_to_file_range(&segments, vaddr, size)?;
        let relocations = parse_relocations(&file, start, end)?;
        for reloc in &relocations {
            let value = resolve_relocation_value(reloc, base, resolve)?;
            let dest = (base + reloc.offset) as *mut u64;
            unsafe { core::ptr::write_unaligned(dest, value) };
        }
    }

    let mut state = DYLIB_STATE.lock();
    let entry = state.entry(root).or_insert_with(ProcessDylibState::new);
    entry.libraries.push(LoadedLibrary { base, symbols: exported });
    Ok(entry.libraries.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::ElfHeader;

    const EHDR_SIZE: usize = 64;

    fn build_header(e_type: u16, phoff: u64, phentsize: u16, phnum: u16, total_len: usize) -> Vec<u8> {
        let mut buf = alloc::vec![0u8; total_len.max(EHDR_SIZE)];
        buf[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
        buf[4] = 2; // ELFCLASS64
        buf[5] = 1; // ELFDATA2LSB
        buf[16..18].copy_from_slice(&e_type.to_le_bytes());
        buf[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        buf[32..40].copy_from_slice(&phoff.to_le_bytes());
        buf[54..56].copy_from_slice(&phentsize.to_le_bytes());
        buf[56..58].copy_from_slice(&phnum.to_le_bytes());
        buf
    }

    fn write_phdr(buf: &mut [u8], offset: usize, p_type: u32, p_offset: u64, p_vaddr: u64, p_filesz: u64, p_memsz: u64) {
        buf[offset..offset + 4].copy_from_slice(&p_type.to_le_bytes());
        buf[offset + 8..offset + 16].copy_from_slice(&p_offset.to_le_bytes());
        buf[offset + 16..offset + 24].copy_from_slice(&p_vaddr.to_le_bytes());
        buf[offset + 32..offset + 40].copy_from_slice(&p_filesz.to_le_bytes());
        buf[offset + 40..offset + 48].copy_from_slice(&p_memsz.to_le_bytes());
    }

    fn write_dyn_entry(buf: &mut [u8], offset: usize, tag: u64, val: u64) {
        buf[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        buf[offset + 8..offset + 16].copy_from_slice(&val.to_le_bytes());
    }

    // -------------------------------------------------------------
    // find_dynamic_segment (tarefa 2.2)
    // -------------------------------------------------------------

    #[test]
    fn finds_the_pt_dynamic_segment() {
        const PHDR_SIZE_U16: u16 = 56;
        let phoff = EHDR_SIZE as u64;
        let mut buf = build_header(3, phoff, PHDR_SIZE_U16, 1, EHDR_SIZE + 56 + 32);
        write_phdr(&mut buf, EHDR_SIZE, PT_DYNAMIC, (EHDR_SIZE + 56) as u64, 0x2000, 32, 32);
        let header = ElfHeader::parse(&buf).unwrap();

        let (start, end) = find_dynamic_segment(&buf, &header).expect("PT_DYNAMIC deveria ser encontrado");
        assert_eq!(start, EHDR_SIZE + 56);
        assert_eq!(end, EHDR_SIZE + 56 + 32);
    }

    #[test]
    fn missing_pt_dynamic_is_rejected() {
        const PHDR_SIZE_U16: u16 = 56;
        let phoff = EHDR_SIZE as u64;
        let mut buf = build_header(3, phoff, PHDR_SIZE_U16, 1, EHDR_SIZE + 56);
        write_phdr(&mut buf, EHDR_SIZE, 1 /* PT_LOAD */, 0, 0, 16, 16);
        let header = ElfHeader::parse(&buf).unwrap();

        assert_eq!(find_dynamic_segment(&buf, &header), Err(DylibError::MissingDynamicSegment));
    }

    // -------------------------------------------------------------
    // vaddr_to_file_offset (tarefa 2.3)
    // -------------------------------------------------------------

    #[test]
    fn vaddr_inside_a_segment_resolves_to_its_file_offset() {
        let segments = [ProgramHeader { flags: 0, offset: 0x1000, vaddr: 0x2000, filesz: 0x100, memsz: 0x100 }];
        assert_eq!(vaddr_to_file_offset(&segments, 0x2010), Some(0x1010));
    }

    #[test]
    fn vaddr_outside_every_segment_is_none() {
        let segments = [ProgramHeader { flags: 0, offset: 0x1000, vaddr: 0x2000, filesz: 0x100, memsz: 0x100 }];
        assert_eq!(vaddr_to_file_offset(&segments, 0x3000), None);
    }

    #[test]
    fn vaddr_in_bss_portion_beyond_filesz_is_none() {
        // So a porcao com conteudo real no arquivo e traduzivel - a parte
        // de BSS (alem de filesz) nao existe no arquivo.
        let segments = [ProgramHeader { flags: 0, offset: 0x1000, vaddr: 0x2000, filesz: 0x10, memsz: 0x100 }];
        assert_eq!(vaddr_to_file_offset(&segments, 0x2020), None);
    }

    // -------------------------------------------------------------
    // parse_dynamic_section (tarefa 2.4)
    // -------------------------------------------------------------

    fn build_complete_dynamic_table() -> Vec<u8> {
        let mut buf = alloc::vec![0u8; DYN_ENTRY_SIZE * 8];
        write_dyn_entry(&mut buf, 0 * DYN_ENTRY_SIZE, DT_NEEDED, 5);
        write_dyn_entry(&mut buf, 1 * DYN_ENTRY_SIZE, DT_STRTAB, 0x100);
        write_dyn_entry(&mut buf, 2 * DYN_ENTRY_SIZE, DT_STRSZ, 0x20);
        write_dyn_entry(&mut buf, 3 * DYN_ENTRY_SIZE, DT_SYMTAB, 0x200);
        write_dyn_entry(&mut buf, 4 * DYN_ENTRY_SIZE, DT_HASH, 0x300);
        write_dyn_entry(&mut buf, 5 * DYN_ENTRY_SIZE, DT_RELA, 0x400);
        write_dyn_entry(&mut buf, 6 * DYN_ENTRY_SIZE, DT_RELASZ, 0x18);
        write_dyn_entry(&mut buf, 7 * DYN_ENTRY_SIZE, DT_NULL, 0);
        buf
    }

    #[test]
    fn complete_dynamic_table_is_parsed() {
        let buf = build_complete_dynamic_table();
        let info = parse_dynamic_section(&buf, 0, buf.len()).expect("tabela completa deveria ser aceita");
        assert_eq!(info.needed_name_offsets, alloc::vec![5]);
        assert_eq!(info.strtab_vaddr, 0x100);
        assert_eq!(info.strsz, 0x20);
        assert_eq!(info.symtab_vaddr, 0x200);
        assert_eq!(info.hash_vaddr, 0x300);
        assert_eq!(info.rela, Some((0x400, 0x18)));
        assert_eq!(info.jmprel, None);
    }

    #[test]
    fn missing_required_tag_is_rejected() {
        let mut buf = alloc::vec![0u8; DYN_ENTRY_SIZE * 3];
        write_dyn_entry(&mut buf, 0, DT_STRTAB, 0x100);
        // DT_SYMTAB e DT_HASH ausentes.
        write_dyn_entry(&mut buf, DYN_ENTRY_SIZE, DT_STRSZ, 0x20);
        write_dyn_entry(&mut buf, 2 * DYN_ENTRY_SIZE, DT_NULL, 0);
        assert_eq!(parse_dynamic_section(&buf, 0, buf.len()), Err(DylibError::MissingRequiredTag));
    }

    #[test]
    fn pltrel_different_from_dt_rela_is_rejected() {
        let mut buf = alloc::vec![0u8; DYN_ENTRY_SIZE * 2];
        write_dyn_entry(&mut buf, 0, DT_PLTREL, 17 /* DT_REL, nao suportado */);
        write_dyn_entry(&mut buf, DYN_ENTRY_SIZE, DT_NULL, 0);
        assert_eq!(parse_dynamic_section(&buf, 0, buf.len()), Err(DylibError::UnsupportedPltRelType));
    }

    // -------------------------------------------------------------
    // cstr_at / resolve_needed_names (tarefa 2.5)
    // -------------------------------------------------------------

    #[test]
    fn cstr_at_reads_up_to_the_null_terminator() {
        let dynstr = b"\0libbase.so\0libuser.so\0";
        assert_eq!(cstr_at(dynstr, 1).unwrap(), "libbase.so");
        assert_eq!(cstr_at(dynstr, 12).unwrap(), "libuser.so");
    }

    #[test]
    fn cstr_at_without_terminator_is_rejected() {
        let dynstr = b"sem terminador";
        assert_eq!(cstr_at(dynstr, 0), Err(DylibError::UnterminatedString));
    }

    #[test]
    fn resolve_needed_names_resolves_each_offset() {
        let dynstr = b"\0libbase.so\0";
        let info = DynamicInfo {
            needed_name_offsets: alloc::vec![1],
            strtab_vaddr: 0,
            strsz: 0,
            symtab_vaddr: 0,
            hash_vaddr: 0,
            rela: None,
            jmprel: None,
        };
        let names = resolve_needed_names(dynstr, &info).unwrap();
        assert_eq!(names, alloc::vec![String::from("libbase.so")]);
    }

    // -------------------------------------------------------------
    // symbol_count_from_hash / parse_dynsym (tarefa 2.6)
    // -------------------------------------------------------------

    #[test]
    fn symbol_count_reads_nchain() {
        let mut buf = alloc::vec![0u8; 8];
        buf[0..4].copy_from_slice(&3u32.to_le_bytes()); // nbucket (ignorado)
        buf[4..8].copy_from_slice(&7u32.to_le_bytes()); // nchain
        assert_eq!(symbol_count_from_hash(&buf, 0).unwrap(), 7);
    }

    fn write_sym_entry(buf: &mut [u8], offset: usize, st_name: u32, st_shndx: u16, st_value: u64) {
        buf[offset..offset + 4].copy_from_slice(&st_name.to_le_bytes());
        buf[offset + 6..offset + 8].copy_from_slice(&st_shndx.to_le_bytes());
        buf[offset + 8..offset + 16].copy_from_slice(&st_value.to_le_bytes());
    }

    #[test]
    fn parses_the_declared_count_of_symbols() {
        let mut buf = alloc::vec![0u8; SYM_ENTRY_SIZE * 2];
        write_sym_entry(&mut buf, 0, 0, 0, 0);
        write_sym_entry(&mut buf, SYM_ENTRY_SIZE, 1, 1, 0x1234);
        let symbols = parse_dynsym(&buf, 0, 2).unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[1].st_name, 1);
        assert_eq!(symbols[1].st_value, 0x1234);
    }

    #[test]
    fn truncated_symbol_table_is_rejected() {
        let buf = alloc::vec![0u8; SYM_ENTRY_SIZE]; // so cabe 1 entrada
        assert_eq!(parse_dynsym(&buf, 0, 2), Err(DylibError::AddressOutOfBounds));
    }

    // -------------------------------------------------------------
    // exported_symbols (tarefa 2.7)
    // -------------------------------------------------------------

    #[test]
    fn exported_symbols_includes_only_named_and_defined() {
        let dynstr = b"\0add\0";
        let symbols = [
            DynSymbol { st_name: 0, st_value: 0, st_shndx: 0 },       // sem nome
            DynSymbol { st_name: 1, st_value: 0, st_shndx: 0 },       // referencia externa (SHN_UNDEF)
            DynSymbol { st_name: 1, st_value: 0x50, st_shndx: 1 },    // exportado
        ];
        let exported = exported_symbols(dynstr, &symbols, 0x1000).unwrap();
        assert_eq!(exported, alloc::vec![(String::from("add"), 0x1050)]);
    }

    // -------------------------------------------------------------
    // parse_relocations / resolve_relocation_value (tarefa 3.1/3.2)
    // -------------------------------------------------------------

    fn write_rela_entry(buf: &mut [u8], offset: usize, r_offset: u64, reloc_type: u32, symbol_index: u32, addend: i64) {
        let r_info = ((symbol_index as u64) << 32) | reloc_type as u64;
        buf[offset..offset + 8].copy_from_slice(&r_offset.to_le_bytes());
        buf[offset + 8..offset + 16].copy_from_slice(&r_info.to_le_bytes());
        buf[offset + 16..offset + 24].copy_from_slice(&addend.to_le_bytes());
    }

    #[test]
    fn parses_offset_type_symbol_index_and_addend() {
        let mut buf = alloc::vec![0u8; RELA_ENTRY_SIZE];
        write_rela_entry(&mut buf, 0, 0x3000, R_X86_64_GLOB_DAT, 42, -8);
        let relocs = parse_relocations(&buf, 0, buf.len()).unwrap();
        assert_eq!(relocs.len(), 1);
        assert_eq!(relocs[0].offset, 0x3000);
        assert_eq!(relocs[0].reloc_type, R_X86_64_GLOB_DAT);
        assert_eq!(relocs[0].symbol_index, 42);
        assert_eq!(relocs[0].addend, -8);
    }

    #[test]
    fn relative_relocation_does_not_need_symbol_resolution() {
        let reloc = Relocation { offset: 0, reloc_type: R_X86_64_RELATIVE, symbol_index: 0, addend: 0x20 };
        let value = resolve_relocation_value(&reloc, 0x1_0000, |_| panic!("nao deveria resolver simbolo")).unwrap();
        assert_eq!(value, 0x1_0020);
    }

    #[test]
    fn glob_dat_resolves_symbol_by_index_and_adds_addend() {
        let reloc = Relocation { offset: 0, reloc_type: R_X86_64_GLOB_DAT, symbol_index: 3, addend: 4 };
        let value = resolve_relocation_value(&reloc, 0x1_0000, |idx| if idx == 3 { Some(0x5000) } else { None }).unwrap();
        assert_eq!(value, 0x5004);
    }

    #[test]
    fn jump_slot_resolves_symbol_by_index() {
        let reloc = Relocation { offset: 0, reloc_type: R_X86_64_JUMP_SLOT, symbol_index: 1, addend: 0 };
        let value = resolve_relocation_value(&reloc, 0, |idx| if idx == 1 { Some(0x9000) } else { None }).unwrap();
        assert_eq!(value, 0x9000);
    }

    #[test]
    fn unresolved_symbol_is_an_explicit_error() {
        let reloc = Relocation { offset: 0, reloc_type: R_X86_64_64, symbol_index: 9, addend: 0 };
        assert_eq!(resolve_relocation_value(&reloc, 0, |_| None), Err(DylibError::UnresolvedSymbol));
    }

    #[test]
    fn unsupported_relocation_type_is_rejected() {
        let reloc = Relocation { offset: 0, reloc_type: 0xFF, symbol_index: 0, addend: 0 };
        assert_eq!(resolve_relocation_value(&reloc, 0, |_| None), Err(DylibError::UnsupportedRelocationType(0xFF)));
    }
}
