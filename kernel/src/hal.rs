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

/// `BlockDevice` simulado por um buffer em memória, usado por testes de host
/// de qualquer módulo que precise de um disco de blocos sem tocar hardware
/// (ex.: `kernel::gpt`, `kernel::fat32`) — mesmo papel de
/// `kernel::phys::tests::SimulatedFrameStore`. Blocos de 512 bytes, fora do
/// intervalo alocado na construção é um erro (`BlockError::DeviceError`),
/// nunca um bloco "mágico" zerado implícito.
#[cfg(test)]
pub(crate) mod test_support {
    extern crate alloc;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{BlockDevice, BlockError};

    pub(crate) const SIMULATED_BLOCK_SIZE: usize = 512;

    pub(crate) struct SimulatedBlockDevice {
        blocks: Vec<[u8; SIMULATED_BLOCK_SIZE]>,
    }

    impl SimulatedBlockDevice {
        /// Cria um disco simulado com `num_blocks` blocos, todos zerados.
        pub(crate) fn new(num_blocks: usize) -> Self {
            Self { blocks: vec![[0u8; SIMULATED_BLOCK_SIZE]; num_blocks] }
        }

        /// Acesso direto (fora do trait `BlockDevice`) a um bloco, para
        /// montar dados de teste byte a byte sem precisar passar por
        /// `write_block`.
        pub(crate) fn block_mut(&mut self, lba: u64) -> &mut [u8; SIMULATED_BLOCK_SIZE] {
            &mut self.blocks[lba as usize]
        }
    }

    impl BlockDevice for SimulatedBlockDevice {
        fn block_size(&self) -> usize {
            SIMULATED_BLOCK_SIZE
        }

        fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
            if buf.len() != SIMULATED_BLOCK_SIZE {
                return Err(BlockError::WrongBufferSize);
            }
            let block = self.blocks.get(lba as usize).ok_or(BlockError::DeviceError)?;
            buf.copy_from_slice(block);
            Ok(())
        }

        fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError> {
            if buf.len() != SIMULATED_BLOCK_SIZE {
                return Err(BlockError::WrongBufferSize);
            }
            let block = self.blocks.get_mut(lba as usize).ok_or(BlockError::DeviceError)?;
            block.copy_from_slice(buf);
            Ok(())
        }
    }
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
