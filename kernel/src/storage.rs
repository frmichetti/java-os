//! Driver de armazenamento AHCI (SATA) — ver `design.md` da change
//! `kernel-storage`.

use crate::paging::{MapError, Mapper, PageFlags, PAGE_SIZE};
use crate::phys::{FrameAllocator, FrameStore};

/// Tipo de FIS "Register Host to Device" (comando enviado do host ao
/// dispositivo).
const FIS_TYPE_REG_H2D: u8 = 0x27;

/// Comandos ATA usados por este driver — leitura/escrita com LBA de 48 bits.
pub const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;

/// Bit 7 do segundo byte do FIS Register H2D: distingue um FIS de comando
/// (atualiza o registrador de comando do dispositivo) de uma atualização de
/// controle.
const FIS_COMMAND_BIT: u8 = 1 << 7;
/// Bit 6 do byte de dispositivo: seleciona modo LBA (em vez de CHS legado).
const DEVICE_LBA_MODE: u8 = 1 << 6;

/// Monta os 20 bytes do FIS de registrador Host-to-Device para um comando
/// ATA com LBA de 48 bits (`ATA_CMD_READ_DMA_EXT`/`ATA_CMD_WRITE_DMA_EXT`) —
/// ver design.md, "AHCI: inicialização mínima de uma porta". Função pura,
/// testável sem hardware.
pub fn build_register_h2d_fis(command: u8, lba: u64, sector_count: u16) -> [u8; 20] {
    let mut fis = [0u8; 20];
    fis[0] = FIS_TYPE_REG_H2D;
    fis[1] = FIS_COMMAND_BIT;
    fis[2] = command;
    fis[4] = (lba & 0xff) as u8;
    fis[5] = ((lba >> 8) & 0xff) as u8;
    fis[6] = ((lba >> 16) & 0xff) as u8;
    fis[7] = DEVICE_LBA_MODE;
    fis[8] = ((lba >> 24) & 0xff) as u8;
    fis[9] = ((lba >> 32) & 0xff) as u8;
    fis[10] = ((lba >> 40) & 0xff) as u8;
    fis[12] = (sector_count & 0xff) as u8;
    fis[13] = ((sector_count >> 8) & 0xff) as u8;
    fis
}

/// Monta o primeiro dword de um cabeçalho de comando AHCI: `fis_len_dwords`
/// nos bits 0-4 (CFL), direção de escrita no bit 6 (W), e a quantidade de
/// entradas PRDT nos bits 16-31 (PRDTL). Função pura, testável sem hardware.
pub fn build_command_header_dword0(fis_len_dwords: u8, write: bool, prdt_count: u16) -> u32 {
    let mut dw0 = (fis_len_dwords as u32) & 0x1f;
    if write {
        dw0 |= 1 << 6;
    }
    dw0 |= (prdt_count as u32) << 16;
    dw0
}

/// Monta o terceiro dword de uma entrada PRDT: a contagem de bytes do
/// buffer menos um (bits 0-21). O bit de interrupção ao completar (bit 31)
/// nunca é usado por este driver — ver design.md, Non-Goal de E/S por
/// interrupção. Função pura, testável sem hardware.
pub fn build_prdt_entry_dword3(byte_count: u32) -> u32 {
    (byte_count - 1) & 0x3f_ffff
}

/// Tamanho de bloco fixo deste driver — o padrão de setor lógico para
/// discos emulados pelo QEMU (e a imensa maioria dos discos reais até
/// "Advanced Format", fora de escopo aqui) — ver design.md.
pub const BLOCK_SIZE: usize = 512;

/// Classe/subclasse/interface de programação PCI de um controlador AHCI
/// (Mass Storage Controller, SATA, AHCI 1.0).
pub const PCI_CLASS_MASS_STORAGE: u8 = 0x01;
pub const PCI_SUBCLASS_SATA: u8 = 0x06;
pub const PCI_PROG_IF_AHCI: u8 = 0x01;

/// Motivo pelo qual o driver de armazenamento não pôde ser inicializado, ou
/// pelo qual um comando falhou — ver design.md, "indisponibilidade tratada
/// explicitamente".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    /// Nenhum controlador AHCI (classe 0x01, subclasse 0x06, prog-if 0x01)
    /// foi encontrado na enumeração do barramento 0.
    NoController,
    /// O controlador AHCI foi encontrado, mas nenhuma porta implementada
    /// tem um dispositivo SATA conectado (`PxSIG`/`PxSSTS`).
    NoDevice,
    /// Falha ao mapear o `ABAR` ou ao alocar as estruturas da AHCI.
    MapFailed(MapError),
    /// O comando foi emitido mas `PxTFD` reportou erro.
    CommandFailed,
    /// O comando nunca terminou dentro do limite de espera ocupada.
    Timeout,
}

impl From<MapError> for StorageError {
    fn from(e: MapError) -> Self {
        StorageError::MapFailed(e)
    }
}

// --- Registradores da HBA (offsets em bytes a partir do ABAR) ---
const HBA_GHC: usize = 0x04;
const HBA_PI: usize = 0x0C;
const HBA_PORT_BASE: usize = 0x100;
const HBA_PORT_STRIDE: usize = 0x80;
/// Páginas suficientes para cobrir os registradores genéricos da HBA e a
/// tabela de até 32 portas (`HBA_PORT_BASE + 32 * HBA_PORT_STRIDE = 0x1100`).
const HBA_MAPPED_PAGES: u64 = 2;

const GHC_AE: u32 = 1 << 31;

// --- Registradores de porta (offsets em bytes a partir da base da porta) ---
const PORT_CLB: usize = 0x00;
const PORT_CLBU: usize = 0x04;
const PORT_FB: usize = 0x08;
const PORT_FBU: usize = 0x0C;
const PORT_IS: usize = 0x10;
const PORT_CMD: usize = 0x18;
const PORT_TFD: usize = 0x20;
const PORT_SIG: usize = 0x24;
const PORT_SSTS: usize = 0x28;
const PORT_CI: usize = 0x38;

const PORT_CMD_ST: u32 = 1 << 0;
const PORT_CMD_FRE: u32 = 1 << 4;
const PORT_CMD_CR: u32 = 1 << 14;
const PORT_CMD_FR: u32 = 1 << 15;

const TFD_ERR: u32 = 1 << 0;

/// Assinatura de um dispositivo SATA comum (não-ATAPI) em `PxSIG`.
const SATA_SIG_ATA: u32 = 0x0000_0101;

/// Limite simples de espera ocupada por um comando — generoso o bastante
/// para uma leitura/escrita de um único setor num disco emulado, sem
/// depender de nenhum temporizador (o kernel já tem `kernel::timer`, mas
/// usar um contador de iterações mantém este driver independente dele).
const COMMAND_TIMEOUT_SPINS: u32 = 50_000_000;

unsafe fn read32(base: *mut u8, offset: usize) -> u32 {
    unsafe { (base.add(offset) as *const u32).read_volatile() }
}

unsafe fn write32(base: *mut u8, offset: usize, value: u32) {
    unsafe { (base.add(offset) as *mut u32).write_volatile(value) }
}

/// `true` se `PxSSTS.DET` indica um dispositivo presente com comunicação
/// PHY já estabelecida.
fn port_device_present(ssts: u32) -> bool {
    (ssts & 0x0f) == 0x03
}

/// Mapeia as páginas MMIO do `ABAR` (BAR5 do controlador AHCI) no endereço
/// virtual da HHDM correspondente — mesmo achado/tratamento do I/O APIC em
/// `kernel-keyboard`: a HHDM do Limine não garante cobrir o endereço físico
/// de um BAR PCI arbitrário.
fn map_abar<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>, abar_phys: u64) -> Result<*mut u8, StorageError> {
    let hhdm_offset = crate::hhdm::offset();
    let virt_base = crate::hhdm::phys_to_virt(hhdm_offset, abar_phys);
    let mut mapper = Mapper::new(frame_allocator);
    for page in 0..HBA_MAPPED_PAGES {
        let offset = page * PAGE_SIZE;
        mapper.map(virt_base + offset, abar_phys + offset, PageFlags::KERNEL_RW, false)?;
    }
    Ok(virt_base as *mut u8)
}

/// Procura, entre as portas implementadas (`PxSSTS`), a primeira com um
/// dispositivo SATA (não-ATAPI) presente — ver design.md.
fn find_sata_port(hba_base: *mut u8) -> Option<usize> {
    let pi = unsafe { read32(hba_base, HBA_PI) };
    for port in 0..32usize {
        if pi & (1 << port) == 0 {
            continue;
        }
        let port_base = HBA_PORT_BASE + port * HBA_PORT_STRIDE;
        let ssts = unsafe { read32(hba_base, port_base + PORT_SSTS) };
        let sig = unsafe { read32(hba_base, port_base + PORT_SIG) };
        if port_device_present(ssts) && sig == SATA_SIG_ATA {
            return Some(port);
        }
    }
    None
}

unsafe fn stop_command_engine(hba_base: *mut u8, port_base: usize) {
    unsafe {
        let mut cmd = read32(hba_base, port_base + PORT_CMD);
        cmd &= !(PORT_CMD_ST | PORT_CMD_FRE);
        write32(hba_base, port_base + PORT_CMD, cmd);
        while read32(hba_base, port_base + PORT_CMD) & (PORT_CMD_CR | PORT_CMD_FR) != 0 {
            core::hint::spin_loop();
        }
    }
}

unsafe fn start_command_engine(hba_base: *mut u8, port_base: usize) {
    unsafe {
        let mut cmd = read32(hba_base, port_base + PORT_CMD);
        cmd |= PORT_CMD_FRE;
        write32(hba_base, port_base + PORT_CMD, cmd);
        cmd |= PORT_CMD_ST;
        write32(hba_base, port_base + PORT_CMD, cmd);
    }
}

/// Alça (não zero-size, guarda os endereços das estruturas AHCI já
/// alocadas) para o disco SATA da porta inicializada — implementa
/// [`crate::hal::BlockDevice`]. `base`/os demais ponteiros são endereços de
/// memória do kernel (HHDM) ou do próprio dispositivo (físicos); o kernel
/// ainda não é SMP, seguro marcar como `Send`.
pub struct AhciDisk {
    hba_base: *mut u8,
    port_index: usize,
    cmd_list_virt: *mut u8,
    cmd_table_virt: *mut u8,
    dma_buf_virt: *mut u8,
    dma_buf_phys: u64,
}

unsafe impl Send for AhciDisk {}

impl AhciDisk {
    /// Inicializa a porta `port_index` da HBA em `hba_base` (já mapeada):
    /// para o motor de comando se já estiver rodando, aloca (uma página por
    /// estrutura — o alinhamento de 4 KiB cobre os alinhamentos exigidos
    /// pela AHCI) a lista de comandos, a área de recebimento de FIS, a
    /// tabela de comando do slot 0, e um buffer de DMA de um bloco (usado
    /// por `read_block`/`write_block` — ver design.md), e liga o motor de
    /// comando.
    fn init_port<S: FrameStore>(
        frame_allocator: &mut FrameAllocator<S>,
        hba_base: *mut u8,
        port_index: usize,
    ) -> Result<Self, StorageError> {
        let port_base = HBA_PORT_BASE + port_index * HBA_PORT_STRIDE;
        unsafe { stop_command_engine(hba_base, port_base) };

        let hhdm_offset = crate::hhdm::offset();
        let cmd_list_phys = frame_allocator.alloc().map_err(|_| StorageError::MapFailed(MapError::OutOfFrames))?;
        let fis_phys = frame_allocator.alloc().map_err(|_| StorageError::MapFailed(MapError::OutOfFrames))?;
        let cmd_table_phys = frame_allocator.alloc().map_err(|_| StorageError::MapFailed(MapError::OutOfFrames))?;
        let dma_buf_phys = frame_allocator.alloc().map_err(|_| StorageError::MapFailed(MapError::OutOfFrames))?;

        let cmd_list_virt = crate::hhdm::phys_to_virt(hhdm_offset, cmd_list_phys) as *mut u8;
        let fis_virt = crate::hhdm::phys_to_virt(hhdm_offset, fis_phys) as *mut u8;
        let cmd_table_virt = crate::hhdm::phys_to_virt(hhdm_offset, cmd_table_phys) as *mut u8;
        let dma_buf_virt = crate::hhdm::phys_to_virt(hhdm_offset, dma_buf_phys) as *mut u8;

        unsafe {
            core::ptr::write_bytes(cmd_list_virt, 0, PAGE_SIZE as usize);
            core::ptr::write_bytes(fis_virt, 0, PAGE_SIZE as usize);
            core::ptr::write_bytes(cmd_table_virt, 0, PAGE_SIZE as usize);
            core::ptr::write_bytes(dma_buf_virt, 0, PAGE_SIZE as usize);

            write32(hba_base, port_base + PORT_CLB, (cmd_list_phys & 0xffff_ffff) as u32);
            write32(hba_base, port_base + PORT_CLBU, (cmd_list_phys >> 32) as u32);
            write32(hba_base, port_base + PORT_FB, (fis_phys & 0xffff_ffff) as u32);
            write32(hba_base, port_base + PORT_FBU, (fis_phys >> 32) as u32);

            // Cabeçalho de comando do slot 0: DW2/DW3 = CTBA/CTBAU (fixos —
            // sempre a mesma tabela de comando, já que só usamos o slot 0).
            let header0 = cmd_list_virt as *mut u32;
            header0.add(2).write_volatile((cmd_table_phys & 0xffff_ffff) as u32);
            header0.add(3).write_volatile((cmd_table_phys >> 32) as u32);

            start_command_engine(hba_base, port_base);
        }

        Ok(Self {
            hba_base,
            port_index,
            cmd_list_virt,
            cmd_table_virt,
            dma_buf_virt,
            dma_buf_phys,
        })
    }

    /// Emite um comando ATA (`command`) no slot 0 para o LBA/contagem de
    /// setores dados, apontando a única entrada PRDT para `self.dma_buf_phys`
    /// (sempre o buffer de DMA interno — ver design.md), e espera
    /// (*polling*) pela conclusão.
    fn submit_command(&mut self, command: u8, lba: u64, write: bool) -> Result<(), StorageError> {
        let port_base = HBA_PORT_BASE + self.port_index * HBA_PORT_STRIDE;

        let fis = build_register_h2d_fis(command, lba, 1);
        unsafe {
            core::ptr::copy_nonoverlapping(fis.as_ptr(), self.cmd_table_virt, fis.len());

            // Entrada PRDT: offset fixo 0x80 na tabela de comando (depois do
            // FIS de comando, do comando ATAPI e da área reservada — ver
            // design.md).
            let prdt = self.cmd_table_virt.add(0x80) as *mut u32;
            prdt.write_volatile((self.dma_buf_phys & 0xffff_ffff) as u32);
            prdt.add(1).write_volatile((self.dma_buf_phys >> 32) as u32);
            prdt.add(2).write_volatile(0);
            prdt.add(3).write_volatile(build_prdt_entry_dword3(BLOCK_SIZE as u32));

            let header0 = self.cmd_list_virt as *mut u32;
            header0.write_volatile(build_command_header_dword0(5, write, 1));
            header0.add(1).write_volatile(0); // PRDBC — o hardware preenche

            write32(self.hba_base, port_base + PORT_IS, u32::MAX);
            write32(self.hba_base, port_base + PORT_CI, 1);
        }

        let mut spins = 0u32;
        loop {
            let ci = unsafe { read32(self.hba_base, port_base + PORT_CI) };
            if ci & 1 == 0 {
                break;
            }
            spins += 1;
            if spins > COMMAND_TIMEOUT_SPINS {
                return Err(StorageError::Timeout);
            }
            core::hint::spin_loop();
        }

        let tfd = unsafe { read32(self.hba_base, port_base + PORT_TFD) };
        if tfd & TFD_ERR != 0 {
            return Err(StorageError::CommandFailed);
        }

        Ok(())
    }
}

/// Valida que `buf_len` é exatamente `block_size` — erro explícito em vez
/// de uma operação parcial silenciosa (ver design.md). Função pura,
/// testável sem hardware.
fn validate_buffer_size(buf_len: usize, block_size: usize) -> Result<(), crate::hal::BlockError> {
    if buf_len == block_size {
        Ok(())
    } else {
        Err(crate::hal::BlockError::WrongBufferSize)
    }
}

impl crate::hal::BlockDevice for AhciDisk {
    fn block_size(&self) -> usize {
        BLOCK_SIZE
    }

    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), crate::hal::BlockError> {
        validate_buffer_size(buf.len(), BLOCK_SIZE)?;
        self.submit_command(ATA_CMD_READ_DMA_EXT, lba, false)
            .map_err(|_| crate::hal::BlockError::DeviceError)?;
        buf.copy_from_slice(unsafe { core::slice::from_raw_parts(self.dma_buf_virt, BLOCK_SIZE) });
        Ok(())
    }

    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), crate::hal::BlockError> {
        validate_buffer_size(buf.len(), BLOCK_SIZE)?;
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), self.dma_buf_virt, BLOCK_SIZE) };
        self.submit_command(ATA_CMD_WRITE_DMA_EXT, lba, true)
            .map_err(|_| crate::hal::BlockError::DeviceError)?;
        Ok(())
    }
}

use crate::sync::Spinlock;

/// Disco de armazenamento global, inicializado por [`init`] — `None` antes
/// da inicialização ou se nenhum controlador/dispositivo utilizável foi
/// encontrado. Mesmo padrão de `Spinlock` já usado por
/// `kernel::framebuffer::CONSOLE`/`kernel::keyboard`.
static DISK: Spinlock<Option<AhciDisk>> = Spinlock::new(None);

/// Inicializa o driver de armazenamento: enumera o barramento 0 à procura
/// de um controlador AHCI (classe/subclasse/prog-if — ver design.md),
/// mapeia seu `ABAR`, localiza a primeira porta com um dispositivo SATA
/// conectado, e inicializa essa porta. Deve ser chamada no máximo uma vez,
/// cedo em `kernel_main` (depois do escalonador e do I/O APIC já prontos).
pub fn init<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) -> Result<(), StorageError> {
    let ahci_device = crate::pci::enumerate_bus0()
        .find(|dev| {
            dev.class == PCI_CLASS_MASS_STORAGE && dev.subclass == PCI_SUBCLASS_SATA && dev.prog_if == PCI_PROG_IF_AHCI
        })
        .ok_or(StorageError::NoController)?;

    let abar_phys = (ahci_device.bars[5] & 0xffff_fff0) as u64;
    let hba_base = map_abar(frame_allocator, abar_phys)?;

    unsafe {
        let ghc = read32(hba_base, HBA_GHC);
        write32(hba_base, HBA_GHC, ghc | GHC_AE);
    }

    let port_index = find_sata_port(hba_base).ok_or(StorageError::NoDevice)?;
    let disk = AhciDisk::init_port(frame_allocator, hba_base, port_index)?;
    *DISK.lock() = Some(disk);
    Ok(())
}

/// Lê o bloco `lba` do disco global, se inicializado — ver [`init`].
pub fn read_block(lba: u64, buf: &mut [u8]) -> Result<(), crate::hal::BlockError> {
    match DISK.lock().as_mut() {
        Some(disk) => crate::hal::BlockDevice::read_block(disk, lba, buf),
        None => Err(crate::hal::BlockError::DeviceError),
    }
}

/// Escreve o bloco `lba` do disco global, se inicializado — ver [`init`].
pub fn write_block(lba: u64, buf: &[u8]) -> Result<(), crate::hal::BlockError> {
    match DISK.lock().as_mut() {
        Some(disk) => crate::hal::BlockDevice::write_block(disk, lba, buf),
        None => Err(crate::hal::BlockError::DeviceError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fis_type_and_command_bit_are_set() {
        let fis = build_register_h2d_fis(ATA_CMD_READ_DMA_EXT, 0, 1);
        assert_eq!(fis[0], FIS_TYPE_REG_H2D);
        assert_ne!(fis[1] & FIS_COMMAND_BIT, 0);
    }

    #[test]
    fn command_byte_matches_the_parameter() {
        let fis = build_register_h2d_fis(ATA_CMD_WRITE_DMA_EXT, 0, 1);
        assert_eq!(fis[2], ATA_CMD_WRITE_DMA_EXT);
    }

    #[test]
    fn forty_eight_bit_lba_round_trips_through_the_expected_bytes() {
        let lba: u64 = 0x12_3456_789a_bc; // usa bits altos (acima de 32 bits)
        let fis = build_register_h2d_fis(ATA_CMD_READ_DMA_EXT, lba, 1);
        let reconstructed = (fis[4] as u64)
            | (fis[5] as u64) << 8
            | (fis[6] as u64) << 16
            | (fis[8] as u64) << 24
            | (fis[9] as u64) << 32
            | (fis[10] as u64) << 40;
        assert_eq!(reconstructed, lba);
    }

    #[test]
    fn device_byte_signals_lba_mode() {
        let fis = build_register_h2d_fis(ATA_CMD_READ_DMA_EXT, 0, 1);
        assert_ne!(fis[7] & DEVICE_LBA_MODE, 0);
    }

    #[test]
    fn sector_count_appears_in_the_expected_bytes() {
        let fis = build_register_h2d_fis(ATA_CMD_READ_DMA_EXT, 0, 0x1234);
        assert_eq!(fis[12], 0x34);
        assert_eq!(fis[13], 0x12);
    }

    #[test]
    fn command_header_cfl_reflects_the_fis_length() {
        let dw0 = build_command_header_dword0(5, false, 1);
        assert_eq!(dw0 & 0x1f, 5);
    }

    #[test]
    fn command_header_write_bit_reflects_the_direction() {
        let read_dw0 = build_command_header_dword0(5, false, 1);
        let write_dw0 = build_command_header_dword0(5, true, 1);
        assert_eq!(read_dw0 & (1 << 6), 0, "leitura nao deveria ligar o bit de escrita");
        assert_ne!(write_dw0 & (1 << 6), 0, "escrita deveria ligar o bit de escrita");
    }

    #[test]
    fn command_header_prdtl_matches_the_entry_count() {
        let dw0 = build_command_header_dword0(5, false, 3);
        assert_eq!((dw0 >> 16) & 0xffff, 3);
    }

    #[test]
    fn prdt_byte_count_is_the_buffer_size_minus_one() {
        assert_eq!(build_prdt_entry_dword3(512), 511);
    }

    #[test]
    fn buffer_of_the_correct_size_is_accepted() {
        assert_eq!(validate_buffer_size(BLOCK_SIZE, BLOCK_SIZE), Ok(()));
    }

    #[test]
    fn buffer_of_the_wrong_size_is_rejected_explicitly() {
        assert_eq!(
            validate_buffer_size(BLOCK_SIZE - 1, BLOCK_SIZE),
            Err(crate::hal::BlockError::WrongBufferSize)
        );
        assert_eq!(
            validate_buffer_size(BLOCK_SIZE + 1, BLOCK_SIZE),
            Err(crate::hal::BlockError::WrongBufferSize)
        );
    }
}
