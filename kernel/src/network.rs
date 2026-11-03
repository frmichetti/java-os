//! Driver de placa de rede Ethernet (Intel 82540EM, `e1000`) — ver
//! `design.md` da change `kernel-network`.

use crate::paging::{MapError, Mapper, PageFlags, PAGE_SIZE};
use crate::phys::{FrameAllocator, FrameStore};
use crate::sync::Spinlock;

/// Tamanho mínimo e máximo de um quadro Ethernet padrão (sem *jumbo
/// frames*) — ver design.md.
pub const MIN_FRAME_SIZE: usize = 64;
pub const MAX_FRAME_SIZE: usize = 1518;

/// Motivo pelo qual o driver de rede não pôde ser inicializado, ou pelo
/// qual uma operação falhou — ver design.md, "indisponibilidade tratada
/// explicitamente".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    /// Nenhuma placa de rede (classe 0x02, subclasse 0x00) foi encontrada
    /// na enumeração do barramento 0.
    NoController,
    /// Falha ao mapear `BAR0` ou ao alocar as estruturas dos anéis de
    /// descritores.
    MapFailed(crate::paging::MapError),
    /// `frame.len()` está fora do intervalo `[MIN_FRAME_SIZE, MAX_FRAME_SIZE]`.
    InvalidFrameSize,
    /// O comando foi emitido mas a placa não confirmou a conclusão.
    DeviceError,
    /// A operação nunca terminou dentro do limite de espera ocupada.
    Timeout,
}

impl From<crate::paging::MapError> for NetworkError {
    fn from(e: crate::paging::MapError) -> Self {
        NetworkError::MapFailed(e)
    }
}

/// Valida que `len` está dentro do intervalo de tamanho de um quadro
/// Ethernet padrão — erro explícito em vez de deixar a placa rejeitar
/// silenciosamente. Função pura, testável sem hardware.
pub fn validate_frame_size(len: usize) -> Result<(), NetworkError> {
    if (MIN_FRAME_SIZE..=MAX_FRAME_SIZE).contains(&len) {
        Ok(())
    } else {
        Err(NetworkError::InvalidFrameSize)
    }
}

// --- Descritor de transmissão legado (16 bytes) ---
/// Bit 0 do byte de comando (`CMD`): fim do pacote (único descritor por
/// quadro, nesta implementação — sempre ligado).
const TX_CMD_EOP: u8 = 1 << 0;
/// Bit 1: insere o FCS (CRC) automaticamente.
const TX_CMD_IFCS: u8 = 1 << 1;
/// Bit 3: reporta status de conclusão (`STA.DD`) quando terminar.
const TX_CMD_RS: u8 = 1 << 3;

/// Monta os 16 bytes de um descritor de transmissão legado do e1000 para um
/// buffer de `length` bytes no endereço físico `buffer_phys` — ver
/// design.md, "Driver e1000". Função pura, testável sem hardware.
pub fn build_tx_descriptor(buffer_phys: u64, length: u16) -> [u8; 16] {
    let mut desc = [0u8; 16];
    desc[0..8].copy_from_slice(&buffer_phys.to_le_bytes());
    desc[8..10].copy_from_slice(&length.to_le_bytes());
    // byte 10 (CSO) permanece 0 — sem checksum offload (Non-Goal).
    desc[11] = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;
    // byte 12 (STA) permanece 0 — ainda não processado pela placa.
    desc
}

// --- Classe PCI de uma placa Ethernet genérica ---
const PCI_CLASS_NETWORK: u8 = 0x02;
const PCI_SUBCLASS_ETHERNET: u8 = 0x00;

/// Offset do registrador de comando no espaço de configuração PCI — bit 0
/// habilita espaço de memória, bit 2 habilita Bus Mastering (necessário
/// para DMA) — ver design.md, "PCI: habilitar Bus Mastering explicitamente".
const PCI_COMMAND_OFFSET: u8 = 0x04;
const PCI_COMMAND_MEMORY_SPACE: u32 = 1 << 0;
const PCI_COMMAND_BUS_MASTER: u32 = 1 << 2;

// --- Registradores do e1000 (offsets em bytes a partir do BAR0) ---
const REG_CTRL: usize = 0x0000;
const REG_RCTL: usize = 0x0100;
const REG_TCTL: usize = 0x0400;
const REG_RDBAL: usize = 0x2800;
const REG_RDBAH: usize = 0x2804;
const REG_RDLEN: usize = 0x2808;
const REG_RDH: usize = 0x2810;
const REG_RDT: usize = 0x2818;
const REG_TDBAL: usize = 0x3800;
const REG_TDBAH: usize = 0x3804;
const REG_TDLEN: usize = 0x3808;
const REG_TDH: usize = 0x3810;
const REG_TDT: usize = 0x3818;
const REG_RAL0: usize = 0x5400;
const REG_RAH0: usize = 0x5404;

const CTRL_FD: u32 = 1 << 0;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_RST: u32 = 1 << 26;

const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
/// Remove o FCS (CRC) do final do quadro recebido antes de armazená-lo no
/// buffer — simplifica o tratamento do comprimento recebido.
const RCTL_SECRC: u32 = 1 << 26;

const TCTL_EN: u32 = 1 << 1;
/// Preenche (*pad*) quadros curtos até o tamanho mínimo — a placa faz isso
/// sozinha em vez do driver precisar preencher manualmente.
const TCTL_PSP: u32 = 1 << 3;

/// Páginas mapeadas de `BAR0` — folgado o bastante para cobrir todos os
/// registradores usados por este driver (o mais alto, `RAH0`, está em
/// `0x5404`).
const BAR0_MAPPED_PAGES: u64 = 8;

const NUM_RX_DESC: usize = 8;
const NUM_TX_DESC: usize = 8;
const DESC_SIZE: usize = 16;
/// Offset, dentro de um descritor de 16 bytes, do byte de status (`STA`
/// para transmissão, `Status` para recepção — mesma posição nos dois).
const DESC_STATUS_OFFSET: usize = 12;
/// Offset, dentro de um descritor de recepção, do comprimento recebido.
const RX_DESC_LENGTH_OFFSET: usize = 8;

const RX_STATUS_DD: u8 = 1 << 0;
const TX_STATUS_DD: u8 = 1 << 0;

/// Limite simples de espera ocupada (reset do dispositivo, conclusão de
/// transmissão) — mesmo padrão já usado em `kernel::storage`.
const OPERATION_TIMEOUT_SPINS: u32 = 50_000_000;

unsafe fn read32(base: *mut u8, offset: usize) -> u32 {
    unsafe { (base.add(offset) as *const u32).read_volatile() }
}

unsafe fn write32(base: *mut u8, offset: usize, value: u32) {
    unsafe { (base.add(offset) as *mut u32).write_volatile(value) }
}

/// Mapeia as páginas MMIO do `BAR0` da placa de rede no endereço virtual da
/// HHDM correspondente — mesmo achado/tratamento do I/O APIC/`ABAR` em
/// changes anteriores: a HHDM do Limine não garante cobrir o endereço
/// físico de um BAR PCI arbitrário.
fn map_bar0<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>, bar0_phys: u64) -> Result<*mut u8, NetworkError> {
    let hhdm_offset = crate::hhdm::offset();
    let virt_base = crate::hhdm::phys_to_virt(hhdm_offset, bar0_phys);
    let mut mapper = Mapper::new(frame_allocator);
    for page in 0..BAR0_MAPPED_PAGES {
        let offset = page * PAGE_SIZE;
        mapper.map(virt_base + offset, bar0_phys + offset, PageFlags::KERNEL_RW, false)?;
    }
    Ok(virt_base as *mut u8)
}

/// Alça para a placa de rede e1000 inicializada — implementa
/// [`crate::hal::NetworkDevice`]. O kernel ainda não é SMP, seguro marcar
/// como `Send`.
pub struct E1000Nic {
    base: *mut u8,
    mac: [u8; 6],
    rx_ring_virt: *mut u8,
    /// (endereço físico, ponteiro virtual) do buffer de cada descritor de
    /// recepção — o buffer em si nunca muda; só seu conteúdo e o status do
    /// descritor mudam a cada ciclo de recepção.
    rx_buffers: [(u64, *mut u8); NUM_RX_DESC],
    rx_next: usize,
    tx_ring_virt: *mut u8,
    tx_buffers: [(u64, *mut u8); NUM_TX_DESC],
    tx_next: usize,
}

unsafe impl Send for E1000Nic {}

impl E1000Nic {
    /// Inicializa a placa em `base` (já mapeado): reset do dispositivo, lê
    /// o endereço MAC (`RAL0`/`RAH0`), aloca e programa os anéis de
    /// descritores de recepção e transmissão (um buffer de página própria
    /// por descritor — ver design.md), e habilita o link e os motores de
    /// recepção/transmissão.
    fn init<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>, base: *mut u8) -> Result<Self, NetworkError> {
        unsafe {
            write32(base, REG_CTRL, read32(base, REG_CTRL) | CTRL_RST);
            let mut spins = 0u32;
            while read32(base, REG_CTRL) & CTRL_RST != 0 {
                spins += 1;
                if spins > OPERATION_TIMEOUT_SPINS {
                    return Err(NetworkError::Timeout);
                }
                core::hint::spin_loop();
            }

            write32(base, REG_CTRL, read32(base, REG_CTRL) | CTRL_SLU | CTRL_FD);
        }

        let mac = unsafe {
            let ral = read32(base, REG_RAL0);
            let rah = read32(base, REG_RAH0);
            [
                (ral & 0xff) as u8,
                ((ral >> 8) & 0xff) as u8,
                ((ral >> 16) & 0xff) as u8,
                ((ral >> 24) & 0xff) as u8,
                (rah & 0xff) as u8,
                ((rah >> 8) & 0xff) as u8,
            ]
        };

        let hhdm_offset = crate::hhdm::offset();

        let rx_ring_phys = frame_allocator.alloc().map_err(|_| NetworkError::MapFailed(MapError::OutOfFrames))?;
        let rx_ring_virt = crate::hhdm::phys_to_virt(hhdm_offset, rx_ring_phys) as *mut u8;
        unsafe { core::ptr::write_bytes(rx_ring_virt, 0, PAGE_SIZE as usize) };

        let mut rx_buffers = [(0u64, core::ptr::null_mut::<u8>()); NUM_RX_DESC];
        for (i, slot) in rx_buffers.iter_mut().enumerate() {
            let buf_phys = frame_allocator.alloc().map_err(|_| NetworkError::MapFailed(MapError::OutOfFrames))?;
            let buf_virt = crate::hhdm::phys_to_virt(hhdm_offset, buf_phys) as *mut u8;
            *slot = (buf_phys, buf_virt);
            unsafe {
                (rx_ring_virt.add(i * DESC_SIZE) as *mut u64).write_volatile(buf_phys);
            }
        }

        unsafe {
            write32(base, REG_RDBAL, (rx_ring_phys & 0xffff_ffff) as u32);
            write32(base, REG_RDBAH, (rx_ring_phys >> 32) as u32);
            write32(base, REG_RDLEN, (NUM_RX_DESC * DESC_SIZE) as u32);
            write32(base, REG_RDH, 0);
            write32(base, REG_RDT, (NUM_RX_DESC - 1) as u32);
            write32(base, REG_RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);
        }

        let tx_ring_phys = frame_allocator.alloc().map_err(|_| NetworkError::MapFailed(MapError::OutOfFrames))?;
        let tx_ring_virt = crate::hhdm::phys_to_virt(hhdm_offset, tx_ring_phys) as *mut u8;
        unsafe { core::ptr::write_bytes(tx_ring_virt, 0, PAGE_SIZE as usize) };

        let mut tx_buffers = [(0u64, core::ptr::null_mut::<u8>()); NUM_TX_DESC];
        for slot in tx_buffers.iter_mut() {
            let buf_phys = frame_allocator.alloc().map_err(|_| NetworkError::MapFailed(MapError::OutOfFrames))?;
            let buf_virt = crate::hhdm::phys_to_virt(hhdm_offset, buf_phys) as *mut u8;
            *slot = (buf_phys, buf_virt);
        }

        unsafe {
            write32(base, REG_TDBAL, (tx_ring_phys & 0xffff_ffff) as u32);
            write32(base, REG_TDBAH, (tx_ring_phys >> 32) as u32);
            write32(base, REG_TDLEN, (NUM_TX_DESC * DESC_SIZE) as u32);
            write32(base, REG_TDH, 0);
            write32(base, REG_TDT, 0);
            write32(base, REG_TCTL, TCTL_EN | TCTL_PSP);
        }

        Ok(Self { base, mac, rx_ring_virt, rx_buffers, rx_next: 0, tx_ring_virt, tx_buffers, tx_next: 0 })
    }
}

impl crate::hal::NetworkDevice for E1000Nic {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_frame(&mut self, frame: &[u8]) -> Result<(), NetworkError> {
        validate_frame_size(frame.len())?;

        let slot = self.tx_next;
        let (buf_phys, buf_virt) = self.tx_buffers[slot];
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf_virt, frame.len());
            let desc = build_tx_descriptor(buf_phys, frame.len() as u16);
            core::ptr::copy_nonoverlapping(desc.as_ptr(), self.tx_ring_virt.add(slot * DESC_SIZE), DESC_SIZE);
        }

        self.tx_next = (slot + 1) % NUM_TX_DESC;
        unsafe { write32(self.base, REG_TDT, self.tx_next as u32) };

        let mut spins = 0u32;
        loop {
            let status = unsafe { (self.tx_ring_virt.add(slot * DESC_SIZE + DESC_STATUS_OFFSET) as *const u8).read_volatile() };
            if status & TX_STATUS_DD != 0 {
                break;
            }
            spins += 1;
            if spins > OPERATION_TIMEOUT_SPINS {
                return Err(NetworkError::Timeout);
            }
            core::hint::spin_loop();
        }

        Ok(())
    }

    fn poll_frame(&mut self, buf: &mut [u8]) -> Option<usize> {
        let slot = self.rx_next;
        let status = unsafe { (self.rx_ring_virt.add(slot * DESC_SIZE + DESC_STATUS_OFFSET) as *const u8).read_volatile() };
        if status & RX_STATUS_DD == 0 {
            return None;
        }

        let length =
            unsafe { (self.rx_ring_virt.add(slot * DESC_SIZE + RX_DESC_LENGTH_OFFSET) as *const u16).read_volatile() } as usize;
        let (_, buf_virt) = self.rx_buffers[slot];
        let copy_len = length.min(buf.len());
        unsafe {
            core::ptr::copy_nonoverlapping(buf_virt, buf.as_mut_ptr(), copy_len);
            (self.rx_ring_virt.add(slot * DESC_SIZE + DESC_STATUS_OFFSET) as *mut u8).write_volatile(0);
        }
        unsafe { write32(self.base, REG_RDT, slot as u32) };
        self.rx_next = (slot + 1) % NUM_RX_DESC;

        Some(copy_len)
    }
}

/// Placa de rede global, inicializada por [`init`] — `None` antes da
/// inicialização ou se nenhuma placa utilizável foi encontrada. Mesmo
/// padrão de `Spinlock` já usado por `kernel::storage::DISK`.
static NIC: Spinlock<Option<E1000Nic>> = Spinlock::new(None);

/// Inicializa o driver de rede: enumera o barramento 0 à procura de uma
/// placa Ethernet (classe/subclasse — ver design.md), habilita Bus
/// Mastering e espaço de memória no registrador de comando PCI, mapeia seu
/// `BAR0`, e inicializa a placa. Deve ser chamada no máximo uma vez, cedo
/// em `kernel_main` (depois do escalonador e do I/O APIC já prontos).
pub fn init<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<(), NetworkError> {
    let nic_device = crate::pci::enumerate_bus0()
        .find(|dev| dev.class == PCI_CLASS_NETWORK && dev.subclass == PCI_SUBCLASS_ETHERNET)
        .ok_or(NetworkError::NoController)?;

    let command = crate::pci::read_config_dword(nic_device.bus, nic_device.device, nic_device.function, PCI_COMMAND_OFFSET);
    crate::pci::write_config_dword(
        nic_device.bus,
        nic_device.device,
        nic_device.function,
        PCI_COMMAND_OFFSET,
        command | PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER,
    );

    let bar0_phys = (nic_device.bars[0] & 0xffff_fff0) as u64;
    let base = map_bar0(frame_allocator, bar0_phys)?;

    let nic = E1000Nic::init(frame_allocator, base)?;
    *NIC.lock() = Some(nic);
    Ok(())
}

/// Envia um quadro Ethernet através da placa global, se inicializada — ver
/// [`init`].
pub fn send_frame(frame: &[u8]) -> Result<(), NetworkError> {
    match NIC.lock().as_mut() {
        Some(nic) => crate::hal::NetworkDevice::send_frame(nic, frame),
        None => Err(NetworkError::DeviceError),
    }
}

/// Consulta o próximo quadro recebido pela placa global, se houver — ver
/// [`init`].
pub fn poll_frame(buf: &mut [u8]) -> Option<usize> {
    NIC.lock().as_mut().and_then(|nic| crate::hal::NetworkDevice::poll_frame(nic, buf))
}

/// Endereço MAC da placa global, se inicializada.
pub fn mac_address() -> Option<[u8; 6]> {
    NIC.lock().as_ref().map(|nic| crate::hal::NetworkDevice::mac_address(nic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_size_within_range_is_accepted() {
        assert_eq!(validate_frame_size(MIN_FRAME_SIZE), Ok(()));
        assert_eq!(validate_frame_size(MAX_FRAME_SIZE), Ok(()));
        assert_eq!(validate_frame_size(200), Ok(()));
    }

    #[test]
    fn frame_size_outside_range_is_rejected_explicitly() {
        assert_eq!(validate_frame_size(MIN_FRAME_SIZE - 1), Err(NetworkError::InvalidFrameSize));
        assert_eq!(validate_frame_size(MAX_FRAME_SIZE + 1), Err(NetworkError::InvalidFrameSize));
    }

    #[test]
    fn tx_descriptor_buffer_address_round_trips_in_little_endian() {
        let phys: u64 = 0x12_3456_789a_bc00;
        let desc = build_tx_descriptor(phys, 64);
        let reconstructed = u64::from_le_bytes(desc[0..8].try_into().unwrap());
        assert_eq!(reconstructed, phys);
    }

    #[test]
    fn tx_descriptor_length_appears_in_the_expected_bytes() {
        let desc = build_tx_descriptor(0, 0x1234);
        let length = u16::from_le_bytes(desc[8..10].try_into().unwrap());
        assert_eq!(length, 0x1234);
    }

    #[test]
    fn tx_descriptor_command_bits_are_set() {
        let desc = build_tx_descriptor(0, 64);
        assert_ne!(desc[11] & TX_CMD_EOP, 0);
        assert_ne!(desc[11] & TX_CMD_IFCS, 0);
        assert_ne!(desc[11] & TX_CMD_RS, 0);
    }

    #[test]
    fn tx_descriptor_status_byte_starts_unprocessed() {
        let desc = build_tx_descriptor(0, 64);
        assert_eq!(desc[12], 0, "STA.DD nao deveria estar ligado antes da placa processar o descritor");
    }
}
