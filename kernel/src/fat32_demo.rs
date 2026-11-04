//! Demonstração de verificação de `kernel::fat32`/`kernel::gpt` — análoga a
//! `kernel::storage_demo`: uma operação única, não um fluxo contínuo. Monta
//! o sistema de arquivos FAT32 da partição de boot e lê `/EFI/BOOT/BOOTX64.EFI`
//! (presente em qualquer imagem de boot UEFI, com nome curto 8.3 válido
//! nativamente — ver design.md da change `kernel-fat32-filesystem`),
//! confirmando que seu conteúdo começa com a assinatura `"MZ"` de um
//! executável PE32+/UEFI.

const EFI_EXECUTABLE_PATH: &str = "/EFI/BOOT/BOOTX64.EFI";

/// Assinatura `"MZ"` (bytes `0x4D 0x5A`) obrigatória no início de qualquer
/// executável PE32+/UEFI.
const MZ_SIGNATURE: [u8; 2] = [0x4D, 0x5A];

/// Monta a partição de boot e lê [`EFI_EXECUTABLE_PATH`], confirmando a
/// assinatura `"MZ"` — só leitura, nunca escreve no disco de boot.
pub fn run() {
    let fs = match crate::fat32::mount_boot_partition() {
        Ok(fs) => fs,
        Err(e) => {
            crate::serial_println!("fat32: falha ao montar a particao de boot: {:?}", e);
            return;
        }
    };

    let content = crate::storage::with_disk(|disk| fs.read_file(disk, EFI_EXECUTABLE_PATH));
    match content {
        Some(Ok(bytes)) => {
            if bytes.len() >= 2 && bytes[0..2] == MZ_SIGNATURE {
                crate::serial_println!(
                    "fat32: {} lido ({} bytes), assinatura MZ confirmada - PASSOU",
                    EFI_EXECUTABLE_PATH,
                    bytes.len()
                );
            } else {
                crate::serial_println!(
                    "fat32: {} lido ({} bytes), mas a assinatura MZ esperada nao foi encontrada",
                    EFI_EXECUTABLE_PATH,
                    bytes.len()
                );
            }
        }
        Some(Err(e)) => {
            crate::serial_println!("fat32: falha ao ler {}: {:?}", EFI_EXECUTABLE_PATH, e);
        }
        None => {
            crate::serial_println!("fat32: disco de armazenamento indisponivel, nao foi possivel ler {}", EFI_EXECUTABLE_PATH);
        }
    }
}
