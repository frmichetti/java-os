//! Demonstração de verificação do driver de armazenamento — análoga às
//! demos anteriores (`kernel::keyboard_demo`), mas uma operação única, não
//! um fluxo contínuo de eventos: lê o primeiro setor do disco de boot e
//! confirma a assinatura de inicialização, provando o mecanismo ponta a
//! ponta sem depender de nenhum sistema de arquivos — ver design.md da
//! change `kernel-storage`.

/// Offset, dentro do primeiro setor, dos dois bytes da assinatura de
/// inicialização (`0x55AA`, little-endian: `0x55` no byte 510, `0xAA` no 511).
const BOOT_SIGNATURE_OFFSET: usize = 510;
const BOOT_SIGNATURE: [u8; 2] = [0x55, 0xAA];

/// Lê o LBA 0 do disco já inicializado (`kernel::storage::init`) e reporta
/// se a assinatura de inicialização foi encontrada — só leitura, nunca
/// escreve no disco de boot.
pub fn run() {
    let mut buf = [0u8; crate::storage::BLOCK_SIZE];
    match crate::storage::read_block(0, &mut buf) {
        Ok(()) => {
            if buf[BOOT_SIGNATURE_OFFSET..BOOT_SIGNATURE_OFFSET + 2] == BOOT_SIGNATURE {
                crate::serial_println!(
                    "armazenamento: setor de boot lido, assinatura 0x{:02x}{:02x} confirmada",
                    buf[BOOT_SIGNATURE_OFFSET],
                    buf[BOOT_SIGNATURE_OFFSET + 1]
                );
            } else {
                crate::serial_println!(
                    "armazenamento: setor de boot lido, mas a assinatura esperada (0x55AA) nao foi encontrada (bytes 510-511: 0x{:02x}{:02x})",
                    buf[BOOT_SIGNATURE_OFFSET],
                    buf[BOOT_SIGNATURE_OFFSET + 1]
                );
            }
        }
        Err(e) => {
            crate::serial_println!("armazenamento: falha ao ler o setor de boot: {:?}", e);
        }
    }
}
