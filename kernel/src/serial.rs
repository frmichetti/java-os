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

    /// Escreve um byte bruto na UART, sem nenhuma interpretação de texto
    /// (ex.: sem o `\r` automático que `write_str` insere antes de `\n`).
    /// Usado pela syscall de escrita (`kernel::syscall`), que recebe bytes
    /// arbitrários do chamador, não necessariamente texto UTF-8 válido.
    pub fn write_byte(&mut self, byte: u8) {
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

/// Adaptador de `core::fmt::Write` para código que precisa de um writer
/// concreto em vez de usar as macros `serial_println!`/`serial_print!`
/// diretamente (ex.: `memmap_report::write_memory_map`, que recebe o writer
/// como parâmetro genérico para continuar puro/testável em host). Escreve
/// na porta serial e espelha no console de framebuffer, assim como
/// [`_print`] — ver design.md da change `kernel-framebuffer-console`,
/// "Espelhamento".
pub struct MirroredWriter;

impl Write for MirroredWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::scheduler::with_interrupts_disabled(|| {
            SERIAL1.lock().write_str(s)?;
            crate::framebuffer::mirror_str(s);
            Ok(())
        })
    }
}

/// Deve ser chamada uma única vez, no início de `kernel_main`, antes de
/// qualquer uso de `serial_println!`.
pub fn init() {
    SERIAL1.lock().init();
}

/// Escreve na porta serial e espelha no console de framebuffer como uma
/// única operação atômica em relação a interrupções — ver `KNOW_ISSUES.md`,
/// "Duas threads de kernel imprimindo concorrentemente travam o timer de
/// hardware para sempre" (achado durante a change `kernel-wait-queue`, só
/// corrigido depois): sem isso, uma thread comum (interrupções habilitadas)
/// podia ser interrompida com o lock de `CONSOLE` retido, e qualquer código
/// que precisasse do mesmo lock rodando com interrupções já desabilitadas
/// (ex.: a syscall de escrita, via `SFMASK`) ficava preso esperando um lock
/// que nunca seria liberado — a própria espera (`Spinlock::lock`, com `IF`
/// já em zero) impedia o timer de entregar a interrupção que devolveria a
/// CPU à thread que detinha o lock. `with_interrupts_disabled` elimina a
/// janela de preempção inteira: nenhuma chamada a esta função pode ser
/// interrompida, então o lock nunca fica retido além da própria chamada.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    crate::scheduler::with_interrupts_disabled(|| {
        SERIAL1.lock().write_fmt(args).expect("falha ao escrever na porta serial");
        crate::framebuffer::mirror(args);
    });
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
