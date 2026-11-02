use core::fmt::{self, Write};

use crate::port::{inb, outb};
use crate::sync::Spinlock;

const COM1_BASE: u16 = 0x3F8;

/// Driver mínimo para a UART 16550 (porta serial COM1), usado como canal
/// primário de saída de boot — é trivialmente capturável pelo QEMU sem
/// depender de framebuffer (ver "Decisões desta Change" em design.md).
pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    const fn new(base: u16) -> Self {
        Self { base }
    }

    fn init(&mut self) {
        unsafe {
            outb(self.base + 1, 0x00); // desabilita interrupções da UART
            outb(self.base + 3, 0x80); // habilita DLAB para configurar o divisor de baud rate
            outb(self.base, 0x03); // divisor = 3 -> 38400 baud (byte baixo)
            outb(self.base + 1, 0x00); // divisor (byte alto)
            outb(self.base + 3, 0x03); // 8 bits, sem paridade, 1 stop bit; desabilita DLAB
            outb(self.base + 2, 0xC7); // habilita e limpa FIFOs, threshold de 14 bytes
            outb(self.base + 4, 0x0B); // IRQs habilitadas nos pinos, RTS/DSR ativos
        }
    }

    fn line_status(&self) -> u8 {
        unsafe { inb(self.base + 5) }
    }

    fn transmitter_empty(&self) -> bool {
        self.line_status() & 0x20 != 0
    }

    fn write_byte(&mut self, byte: u8) {
        while !self.transmitter_empty() {
            core::hint::spin_loop();
        }
        unsafe { outb(self.base, byte) };
    }
}

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

pub static SERIAL1: Spinlock<SerialPort> = Spinlock::new(SerialPort::new(COM1_BASE));

/// Deve ser chamada uma única vez, no início de `kernel_main`, antes de
/// qualquer uso de `serial_println!`.
pub fn init() {
    SERIAL1.lock().init();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    SERIAL1.lock().write_fmt(args).expect("falha ao escrever na porta serial");
}

/// Variante de emergência: ignora o lock de `SERIAL1` em vez de esperar por
/// ele. Usada por handlers de exceção de CPU e outros caminhos de falha
/// irrecuperável, onde o kernel está prestes a haltar e travar esperando um
/// lock (possivelmente já corrompido pela própria falha) seria pior do que
/// uma escrita não sincronizada.
///
/// # Safety
/// Só deve ser chamada de um contexto que não vai retornar à execução
/// normal (o kernel entra em halt logo em seguida).
#[doc(hidden)]
pub unsafe fn _print_emergency(args: fmt::Arguments) {
    let port = unsafe { SERIAL1.force_get_mut() };
    let _ = port.write_fmt(args);
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

/// Ver [`_print_emergency`] — para usar em handlers de falha irrecuperável.
#[macro_export]
macro_rules! serial_print_emergency {
    ($($arg:tt)*) => (unsafe { $crate::serial::_print_emergency(format_args!($($arg)*)) });
}

#[macro_export]
macro_rules! serial_println_emergency {
    () => ($crate::serial_print_emergency!("\n"));
    ($($arg:tt)*) => ($crate::serial_print_emergency!("{}\n", format_args!($($arg)*)));
}
