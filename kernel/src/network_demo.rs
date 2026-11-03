//! Demonstração de verificação do driver de rede — análoga a
//! `kernel::storage_demo`: uma operação única, não um fluxo contínuo de
//! eventos. Envia um quadro Ethernet de teste simples (sem nenhum
//! protocolo de verdade) e reporta se a placa aceitou o envio, provando o
//! mecanismo ponta a ponta — ver `design.md` da change `kernel-network`.

/// EtherType reservado pelo IEEE para uso experimental/local (nunca
/// atribuído a um protocolo real) — usado só para marcar este quadro como
/// um teste, sem a pretensão de implementar nenhum protocolo.
const TEST_ETHERTYPE: [u8; 2] = [0x88, 0xB5];
const BROADCAST_MAC: [u8; 6] = [0xff; 6];

/// Monta um quadro Ethernet de teste (destino broadcast, origem `src_mac`,
/// EtherType de teste, preenchido até o tamanho mínimo de 64 bytes) e
/// envia via `kernel::network::send_frame`, reportando o resultado pelos
/// consoles existentes.
pub fn run() {
    let Some(src_mac) = crate::network::mac_address() else {
        crate::serial_println!("rede: placa nao inicializada, demonstracao pulada");
        return;
    };

    let mut frame = [0u8; crate::network::MIN_FRAME_SIZE];
    frame[0..6].copy_from_slice(&BROADCAST_MAC);
    frame[6..12].copy_from_slice(&src_mac);
    frame[12..14].copy_from_slice(&TEST_ETHERTYPE);
    // bytes 14..64 (carga util + preenchimento) permanecem zerados.

    match crate::network::send_frame(&frame) {
        Ok(()) => {
            crate::serial_println!(
                "rede: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, quadro de teste de {} bytes aceito pela placa",
                src_mac[0], src_mac[1], src_mac[2], src_mac[3], src_mac[4], src_mac[5],
                frame.len()
            );
        }
        Err(e) => {
            crate::serial_println!("rede: falha ao enviar o quadro de teste: {:?}", e);
        }
    }
}
