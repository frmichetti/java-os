//! Acesso à configuração PCI (mecanismo legado de portas 0xCF8/0xCFC) e
//! enumeração do barramento 0 — ver `design.md` da change `kernel-storage`.

use crate::port::{inl, outl};

const CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const CONFIG_DATA_PORT: u16 = 0xCFC;

const ENABLE_BIT: u32 = 1 << 31;

/// Monta o valor de 32 bits de `CONFIG_ADDRESS` para acessar o registrador
/// de configuração em `offset` (deve ser alinhado a 4 bytes — os 2 bits
/// menos significativos são sempre zerados) de uma função PCI identificada
/// por (`bus`, `device`, `function`). Função pura, testável sem hardware.
fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    ENABLE_BIT
        | ((bus as u32) << 16)
        | (((device as u32) & 0x1f) << 11)
        | (((function as u32) & 0x07) << 8)
        | ((offset as u32) & 0xfc)
}

/// Lê o dword de configuração em `offset` da função PCI (`bus`, `device`,
/// `function`), via `CONFIG_ADDRESS`/`CONFIG_DATA`.
pub fn read_config_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    unsafe {
        outl(CONFIG_ADDRESS_PORT, config_address(bus, device, function, offset));
        inl(CONFIG_DATA_PORT)
    }
}

/// Escreve `value` no dword de configuração em `offset` da função PCI
/// (`bus`, `device`, `function`) — ver `design.md` da change
/// `kernel-network`, "PCI: habilitar Bus Mastering explicitamente".
pub fn write_config_dword(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    unsafe {
        outl(CONFIG_ADDRESS_PORT, config_address(bus, device, function, offset));
        outl(CONFIG_DATA_PORT, value);
    }
}

/// Vendor ID reportado quando não há função PCI presente nessa posição do
/// barramento.
const NO_DEVICE_VENDOR_ID: u16 = 0xffff;

/// Uma função PCI encontrada pela enumeração — posição no barramento,
/// identificação, classe de hardware, e os 6 BARs (endereços base) já lidos
/// da configuração.
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub bars: [u32; 6],
}

/// Enumera todas as funções PCI presentes no barramento 0 (256 combinações
/// de dispositivo×função) — suficiente para o chipset ICH9 do QEMU/Q35, ver
/// design.md, "PCI: mecanismo legado de portas, enumeração só do barramento 0".
pub fn enumerate_bus0() -> impl Iterator<Item = PciDevice> {
    (0u8..32).flat_map(|device| {
        (0u8..8).filter_map(move |function| {
            let id = read_config_dword(0, device, function, 0x00);
            let vendor_id = (id & 0xffff) as u16;
            if vendor_id == NO_DEVICE_VENDOR_ID {
                return None;
            }
            let device_id = (id >> 16) as u16;

            let class_reg = read_config_dword(0, device, function, 0x08);
            let prog_if = ((class_reg >> 8) & 0xff) as u8;
            let subclass = ((class_reg >> 16) & 0xff) as u8;
            let class = ((class_reg >> 24) & 0xff) as u8;

            let mut bars = [0u32; 6];
            for (i, bar) in bars.iter_mut().enumerate() {
                *bar = read_config_dword(0, device, function, 0x10 + (i as u8) * 4);
            }

            Some(PciDevice { bus: 0, device, function, vendor_id, device_id, class, subclass, prog_if, bars })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_bit_is_always_set() {
        let addr = config_address(0, 0, 0, 0);
        assert_ne!(addr & ENABLE_BIT, 0);
    }

    #[test]
    fn bus_device_function_and_offset_appear_in_the_expected_bits() {
        let addr = config_address(0x12, 0x1f, 0x05, 0xa4);
        assert_eq!((addr >> 16) & 0xff, 0x12, "barramento deveria estar nos bits 23-16");
        assert_eq!((addr >> 11) & 0x1f, 0x1f, "dispositivo deveria estar nos bits 15-11");
        assert_eq!((addr >> 8) & 0x07, 0x05, "funcao deveria estar nos bits 10-8");
        assert_eq!(addr & 0xff, 0xa4, "offset ja alinhado a 4 bytes deveria ficar intacto nos bits 7-0");
    }

    #[test]
    fn offset_is_truncated_to_a_four_byte_boundary() {
        let addr = config_address(0, 0, 0, 0x07);
        assert_eq!(addr & 0xff, 0x04, "os 2 bits menos significativos do offset devem ser zerados");
    }

    #[test]
    fn device_and_function_are_masked_to_their_valid_bit_widths() {
        // device so tem 5 bits (0-31), function so tem 3 bits (0-7) —
        // valores fora disso nunca deveriam vazar para outros campos.
        let addr = config_address(0, 0xff, 0xff, 0);
        assert_eq!((addr >> 11) & 0x1f, 0x1f);
        assert_eq!((addr >> 8) & 0x07, 0x07);
        assert_eq!((addr >> 16) & 0xff, 0, "mascaramento de device/function nao deveria vazar para o campo de barramento");
    }
}
