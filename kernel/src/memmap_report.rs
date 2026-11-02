use core::fmt::{self, Write};

use limine::memmap::{self, Entry};

fn region_type_name(entry: &Entry) -> &'static str {
    match entry.type_ {
        memmap::MEMMAP_USABLE => "usavel",
        memmap::MEMMAP_RESERVED => "reservada",
        memmap::MEMMAP_ACPI_RECLAIMABLE => "ACPI reclamavel",
        memmap::MEMMAP_ACPI_NVS => "ACPI NVS",
        memmap::MEMMAP_BAD_MEMORY => "memoria defeituosa",
        memmap::MEMMAP_BOOTLOADER_RECLAIMABLE => "reclamavel pelo bootloader",
        memmap::MEMMAP_EXECUTABLE_AND_MODULES => "kernel/modulos",
        memmap::MEMMAP_FRAMEBUFFER => "framebuffer",
        memmap::MEMMAP_MAPPED_RESERVED => "mapeada reservada",
        _ => "desconhecida",
    }
}

/// Escreve o relatório do mapa de memória em `out`.
///
/// Função pura (sem I/O de hardware), testável fora do QEMU — ver cenários
/// "Mapa de memória é exibido no boot" e "Mapa de memória indisponível" em
/// `specs/kernel-boot/spec.md`.
pub fn write_memory_map<W: Write>(out: &mut W, entries: Option<&[&Entry]>) -> fmt::Result {
    let Some(entries) = entries else {
        return writeln!(
            out,
            "mapa de memoria: AUSENTE (bootloader nao respondeu ao memmap request)"
        );
    };

    let mut usable_total: u64 = 0;
    for entry in entries {
        writeln!(
            out,
            "  base=0x{:016x} tamanho=0x{:016x} tipo={}",
            entry.base,
            entry.length,
            region_type_name(entry)
        )?;
        if entry.type_ == memmap::MEMMAP_USABLE {
            usable_total += entry.length;
        }
    }
    writeln!(out, "memoria usavel total: {} bytes", usable_total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_absence_when_no_memmap() {
        let mut out = String::new();
        write_memory_map(&mut out, None).unwrap();
        assert!(out.contains("AUSENTE"));
    }

    #[test]
    fn does_not_fabricate_values_when_absent() {
        let mut out = String::new();
        write_memory_map(&mut out, None).unwrap();
        assert!(!out.contains("base=0x0000000000000000"));
    }

    #[test]
    fn reports_entries_and_usable_total_when_present() {
        let data = [
            Entry { base: 0x1000, length: 0x2000, type_: memmap::MEMMAP_USABLE },
            Entry { base: 0x5000, length: 0x1000, type_: memmap::MEMMAP_RESERVED },
        ];
        let refs: [&Entry; 2] = [&data[0], &data[1]];
        let mut out = String::new();
        write_memory_map(&mut out, Some(&refs)).unwrap();

        assert!(out.contains("base=0x0000000000001000"));
        assert!(out.contains("tipo=usavel"));
        assert!(out.contains("tipo=reservada"));
        assert!(out.contains("memoria usavel total: 8192 bytes"));
    }
}
