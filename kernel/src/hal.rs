//! Primeira abstração da camada de HAL (hardware abstraction layer) do
//! kernel — ver `design.md` da change `kernel-framebuffer-console`.

/// Qualquer dispositivo que aceita texto como saída — independe de qual
/// dispositivo físico específico o implementa (console serial, console de
/// framebuffer, etc.). Sem métodos próprios nesta primeira versão:
/// `core::fmt::Write` já cobre tudo que as specs atuais exigem.
pub trait Console: core::fmt::Write {}

/// Qualquer dispositivo que produz eventos de tecla — independe de qual
/// dispositivo físico específico o implementa (hoje só o teclado PS/2,
/// `kernel::keyboard::Ps2Keyboard`). Único método, não-bloqueante: o kernel
/// ainda não tem nenhuma primitiva de bloqueio/espera de thread, então
/// "nenhum evento pendente" é representado por `None`, não por esperar —
/// ver `design.md` da change `kernel-keyboard`.
pub trait KeyboardDevice {
    fn poll_event(&mut self) -> Option<crate::keyboard::KeyEvent>;
}

/// Motivo pelo qual uma operação de bloco falhou — ver `design.md` da
/// change `kernel-storage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// O buffer passado não tem exatamente `block_size()` bytes.
    WrongBufferSize,
    /// O dispositivo reportou um erro ao executar o comando.
    DeviceError,
}

/// Qualquer dispositivo de armazenamento em blocos, endereçado por índice
/// (LBA) — independe de qual controlador físico específico o implementa
/// (hoje só AHCI, `kernel::storage::AhciDisk`). Um `buf` de tamanho
/// diferente de `block_size()` é sempre um erro explícito (`BlockError`),
/// nunca uma operação parcial silenciosa — ver `design.md` da change
/// `kernel-storage`.
pub trait BlockDevice {
    fn block_size(&self) -> usize;
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError>;
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError>;
}

/// Qualquer placa de rede que envia/recebe quadros Ethernet brutos —
/// independe de qual controlador físico específico o implementa (hoje só
/// a placa e1000, `kernel::network::E1000Nic`). `poll_frame` é
/// não-bloqueante, mesmo espírito de `KeyboardDevice::poll_event` — ver
/// `design.md` da change `kernel-network`.
pub trait NetworkDevice {
    fn mac_address(&self) -> [u8; 6];
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), crate::network::NetworkError>;
    fn poll_frame(&mut self, buf: &mut [u8]) -> Option<usize>;
}
