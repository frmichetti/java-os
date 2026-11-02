//! Driver de teclado PS/2 (primeiro canal) — ver `design.md` da change
//! `kernel-keyboard`.

use crate::idt::InterruptStackFrame;
use crate::phys::{FrameAllocator, FrameStore};
use crate::port::{inb, outb};
use crate::sync::Spinlock;

/// Evento de tecla: pressionada ou solta, com o caractere ASCII
/// correspondente quando a tecla tem um (ex.: Shift não tem).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub pressed: bool,
    pub ascii: Option<u8>,
}

/// `true` se `code` é um break code (tecla solta) — bit 7 ligado, no
/// scancode set 1 (ver design.md, "Scancodes: set 1").
fn is_break_code(code: u8) -> bool {
    code & 0x80 != 0
}

/// Extrai o make code (bits 0-6) de um scancode, seja ele um make code ou
/// um break code.
fn make_code(code: u8) -> u8 {
    code & 0x7f
}

/// Byte de prefixo de uma sequência de tecla estendida (setas, Insert/
/// Delete, etc. — Non-Goal, ver design.md).
const EXTENDED_PREFIX: u8 = 0xE0;

fn is_extended_prefix(code: u8) -> bool {
    code == EXTENDED_PREFIX
}

const LEFT_SHIFT_MAKE_CODE: u8 = 0x2A;
const RIGHT_SHIFT_MAKE_CODE: u8 = 0x36;

/// Caractere ASCII (sem Shift, com Shift) de um make code do layout US
/// QWERTY — só as teclas alfanuméricas, de símbolo, espaço, tab, enter e
/// backspace (ver design.md); qualquer outro make code devolve `None`.
fn ascii_for_make_code(code: u8, shift: bool) -> Option<u8> {
    let (unshifted, shifted): (u8, u8) = match code {
        0x02 => (b'1', b'!'),
        0x03 => (b'2', b'@'),
        0x04 => (b'3', b'#'),
        0x05 => (b'4', b'$'),
        0x06 => (b'5', b'%'),
        0x07 => (b'6', b'^'),
        0x08 => (b'7', b'&'),
        0x09 => (b'8', b'*'),
        0x0A => (b'9', b'('),
        0x0B => (b'0', b')'),
        0x0C => (b'-', b'_'),
        0x0D => (b'=', b'+'),
        0x0E => (0x08, 0x08), // backspace
        0x0F => (b'\t', b'\t'),
        0x10 => (b'q', b'Q'),
        0x11 => (b'w', b'W'),
        0x12 => (b'e', b'E'),
        0x13 => (b'r', b'R'),
        0x14 => (b't', b'T'),
        0x15 => (b'y', b'Y'),
        0x16 => (b'u', b'U'),
        0x17 => (b'i', b'I'),
        0x18 => (b'o', b'O'),
        0x19 => (b'p', b'P'),
        0x1A => (b'[', b'{'),
        0x1B => (b']', b'}'),
        0x1C => (b'\n', b'\n'),
        0x1E => (b'a', b'A'),
        0x1F => (b's', b'S'),
        0x20 => (b'd', b'D'),
        0x21 => (b'f', b'F'),
        0x22 => (b'g', b'G'),
        0x23 => (b'h', b'H'),
        0x24 => (b'j', b'J'),
        0x25 => (b'k', b'K'),
        0x26 => (b'l', b'L'),
        0x27 => (b';', b':'),
        0x28 => (b'\'', b'"'),
        0x29 => (b'`', b'~'),
        0x2B => (b'\\', b'|'),
        0x2C => (b'z', b'Z'),
        0x2D => (b'x', b'X'),
        0x2E => (b'c', b'C'),
        0x2F => (b'v', b'V'),
        0x30 => (b'b', b'B'),
        0x31 => (b'n', b'N'),
        0x32 => (b'm', b'M'),
        0x33 => (b',', b'<'),
        0x34 => (b'.', b'>'),
        0x35 => (b'/', b'?'),
        0x39 => (b' ', b' '),
        _ => return None,
    };
    Some(if shift { shifted } else { unshifted })
}

/// Tradutor com estado (estado de Shift, prefixo estendido pendente) de
/// bytes crus de scancode set 1 para [`KeyEvent`]s — ver design.md,
/// "Scancodes: set 1, com estado de Shift, sem teclas estendidas". Lógica
/// pura, sem nenhum acesso a hardware: testável em host.
pub struct ScancodeTranslator {
    shift_left: bool,
    shift_right: bool,
    pending_extended: bool,
}

impl ScancodeTranslator {
    pub const fn new() -> Self {
        Self { shift_left: false, shift_right: false, pending_extended: false }
    }

    fn shift_held(&self) -> bool {
        self.shift_left || self.shift_right
    }

    /// Processa um byte de scancode, devolvendo o evento de tecla
    /// correspondente, se houver. Devolve `None` para: o segundo byte de
    /// uma sequência estendida (`0xE0` + byte), e qualquer scancode sem
    /// mapeamento conhecido.
    pub fn feed(&mut self, code: u8) -> Option<KeyEvent> {
        if self.pending_extended {
            self.pending_extended = false;
            return None;
        }
        if is_extended_prefix(code) {
            self.pending_extended = true;
            return None;
        }

        let pressed = !is_break_code(code);
        let base = make_code(code);

        match base {
            LEFT_SHIFT_MAKE_CODE => {
                self.shift_left = pressed;
                return Some(KeyEvent { pressed, ascii: None });
            }
            RIGHT_SHIFT_MAKE_CODE => {
                self.shift_right = pressed;
                return Some(KeyEvent { pressed, ascii: None });
            }
            _ => {}
        }

        let ascii = ascii_for_make_code(base, self.shift_held())?;
        Some(KeyEvent { pressed, ascii: Some(ascii) })
    }
}

/// Capacidade fixa da fila de eventos — ver design.md, "Fila de eventos".
const QUEUE_CAPACITY: usize = 32;

/// Buffer circular de capacidade fixa para [`KeyEvent`]s. Inserir além da
/// capacidade descarta o evento mais novo (o que está sendo inserido),
/// mantendo os já presentes intactos — ver design.md. Lógica pura,
/// testável em host.
pub struct KeyEventQueue {
    buffer: [Option<KeyEvent>; QUEUE_CAPACITY],
    head: usize,
    len: usize,
}

impl KeyEventQueue {
    pub const fn new() -> Self {
        Self { buffer: [None; QUEUE_CAPACITY], head: 0, len: 0 }
    }

    pub fn push(&mut self, event: KeyEvent) {
        if self.len == QUEUE_CAPACITY {
            return;
        }
        let tail = (self.head + self.len) % QUEUE_CAPACITY;
        self.buffer[tail] = Some(event);
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<KeyEvent> {
        if self.len == 0 {
            return None;
        }
        let event = self.buffer[self.head].take();
        self.head = (self.head + 1) % QUEUE_CAPACITY;
        self.len -= 1;
        event
    }
}

/// Fila global de eventos de tecla, preenchida pelo handler de IRQ1 e
/// consumida por quem chamar [`poll_event`] — mesmo padrão de `Spinlock`
/// global já usado por `kernel::serial::SERIAL1`/`kernel::framebuffer::CONSOLE`.
static QUEUE: Spinlock<KeyEventQueue> = Spinlock::new(KeyEventQueue::new());

/// Consulta o próximo evento de tecla pendente, se houver — não-bloqueante
/// (ver `kernel::hal::KeyboardDevice`).
///
/// Achado durante a implementação: travava o kernel inteiro na primeira
/// tecla recebida — um deadlock clássico de single-core, a mesma classe já
/// documentada em `kernel::scheduler` para `READY_QUEUE`/`CONTEXTS`
/// (ver lá, "Fila de prontos do escalonador"): a thread de demonstração
/// consulta `poll_event` em loop bem apertado, então há uma chance real de
/// a IRQ1 disparar exatamente enquanto essa thread já segura o lock de
/// `QUEUE` — e o handler de IRQ1 (que roda na mesma CPU, "por cima" da
/// thread interrompida) então trava para sempre esperando o mesmo lock,
/// que a thread interrompida nunca vai poder liberar (ela só volta a
/// executar depois que o handler retornar). A correção é a mesma já usada
/// pelo escalonador: desabilitar interrupções durante a seção crítica do
/// lado consumidor, para que a IRQ1 nunca possa interromper exatamente no
/// meio dela.
pub fn poll_event() -> Option<KeyEvent> {
    crate::scheduler::with_interrupts_disabled(|| QUEUE.lock().pop())
}

fn push_event(event: KeyEvent) {
    QUEUE.lock().push(event);
}

/// Alça (zero-size) para o teclado PS/2 do primeiro canal, implementando
/// [`crate::hal::KeyboardDevice`] por delegação à fila global — mesmo
/// padrão de "struct concreta implementando a HAL" já usado por
/// `FramebufferConsole`/`Console` em `kernel-framebuffer-console`.
pub struct Ps2Keyboard;

impl crate::hal::KeyboardDevice for Ps2Keyboard {
    fn poll_event(&mut self) -> Option<KeyEvent> {
        poll_event()
    }
}

/// Tradutor de scancode global, atualizado pelo handler de IRQ1 — estado
/// separado da fila de eventos (dois `Spinlock`s independentes, cada um com
/// uma seção crítica curta, sempre adquiridos na mesma ordem dentro do
/// handler: sem risco de deadlock).
static TRANSLATOR: Spinlock<ScancodeTranslator> = Spinlock::new(ScancodeTranslator::new());

const DATA_PORT: u16 = 0x60;
const STATUS_COMMAND_PORT: u16 = 0x64;

const STATUS_OUTPUT_BUFFER_FULL: u8 = 1 << 0;
const STATUS_INPUT_BUFFER_FULL: u8 = 1 << 1;

const CMD_DISABLE_PORT1: u8 = 0xAD;
const CMD_DISABLE_PORT2: u8 = 0xA7;
const CMD_READ_CONFIG_BYTE: u8 = 0x20;
const CMD_WRITE_CONFIG_BYTE: u8 = 0x60;
const CMD_ENABLE_PORT1: u8 = 0xAE;

/// Bit 0 do byte de configuração do controlador: habilita a IRQ1 do
/// primeiro canal.
const CONFIG_PORT1_IRQ_ENABLE: u8 = 1 << 0;
/// Bit 1: habilita a IRQ12 do segundo canal — mantido desligado (segundo
/// canal fora de escopo, ver design.md).
const CONFIG_PORT2_IRQ_ENABLE: u8 = 1 << 1;

/// Vetor da IDT dedicado à IRQ1 (teclado) — o próximo livre depois do
/// timer (`timer::TIMER_VECTOR`).
const KEYBOARD_VECTOR: usize = 33;
/// IRQ ISA do teclado no primeiro canal PS/2.
const KEYBOARD_IRQ: u8 = 1;

unsafe fn wait_until_ready_for_input() {
    unsafe {
        while inb(STATUS_COMMAND_PORT) & STATUS_INPUT_BUFFER_FULL != 0 {
            core::hint::spin_loop();
        }
    }
}

unsafe fn wait_until_output_available() {
    unsafe {
        while inb(STATUS_COMMAND_PORT) & STATUS_OUTPUT_BUFFER_FULL == 0 {
            core::hint::spin_loop();
        }
    }
}

unsafe fn send_controller_command(command: u8) {
    unsafe {
        wait_until_ready_for_input();
        outb(STATUS_COMMAND_PORT, command);
    }
}

unsafe fn write_data_byte(byte: u8) {
    unsafe {
        wait_until_ready_for_input();
        outb(DATA_PORT, byte);
    }
}

unsafe fn read_data_byte() -> u8 {
    unsafe {
        wait_until_output_available();
        inb(DATA_PORT)
    }
}

/// Descarta qualquer byte já pendente no buffer de saída do controlador,
/// sem esperar por um novo (ao contrário de [`read_data_byte`]) — usado só
/// durante a inicialização, antes de qualquer IRQ estar habilitada.
unsafe fn flush_output_buffer() {
    unsafe {
        while inb(STATUS_COMMAND_PORT) & STATUS_OUTPUT_BUFFER_FULL != 0 {
            let _ = inb(DATA_PORT);
        }
    }
}

/// Inicializa o controlador 8042: desabilita os dois canais, esvazia o
/// buffer de saída, habilita a IRQ1 do primeiro canal (mantendo a IRQ12 do
/// segundo desabilitada) no byte de configuração, e reabilita só o primeiro
/// canal — ver design.md, "Teclado PS/2: inicialização mínima".
unsafe fn init_controller() {
    unsafe {
        send_controller_command(CMD_DISABLE_PORT1);
        send_controller_command(CMD_DISABLE_PORT2);
        flush_output_buffer();

        send_controller_command(CMD_READ_CONFIG_BYTE);
        let mut config = read_data_byte();
        config |= CONFIG_PORT1_IRQ_ENABLE;
        config &= !CONFIG_PORT2_IRQ_ENABLE;
        send_controller_command(CMD_WRITE_CONFIG_BYTE);
        write_data_byte(config);

        send_controller_command(CMD_ENABLE_PORT1);
    }
}

/// Handler de IRQ1: lê o scancode recebido, traduz (atualizando o estado de
/// Shift/prefixo estendido em [`TRANSLATOR`]) e, se houver um evento,
/// empilha na fila global — depois reconhece a interrupção via EOI do Local
/// APIC.
///
/// Achado durante a implementação: ler só um byte por IRQ perde bytes
/// seguintes que cheguem antes do buffer de saída do controlador ser
/// esvaziado (ex.: make+break code da mesma tecla, em sequência rápida) —
/// o IRQ1 é por borda, e uma borda nova só ocorre quando `STATUS_OUTPUT_BUFFER_FULL`
/// transiciona de 0 para 1; se o byte anterior não foi lido ainda quando o
/// próximo chega, o controlador não gera outra borda, e o byte extra fica
/// parado no buffer para sempre (nenhuma IRQ futura o entrega). O handler
/// esvazia o buffer por completo a cada disparo, não só um byte.
extern "x86-interrupt" fn keyboard_irq_handler(_frame: InterruptStackFrame) {
    unsafe {
        while inb(STATUS_COMMAND_PORT) & STATUS_OUTPUT_BUFFER_FULL != 0 {
            let scancode = inb(DATA_PORT);
            let event = TRANSLATOR.lock().feed(scancode);
            if let Some(event) = event {
                push_event(event);
            }
        }
    }
    crate::apic::send_eoi();
}

/// Inicializa o teclado PS/2: o controlador 8042 ([`init_controller`]),
/// registra o handler de IRQ1 na IDT, mapeia a página MMIO do I/O APIC
/// (`kernel::ioapic::init`) e roteia a IRQ1 para esse vetor via
/// `kernel::ioapic::route_irq` — ver design.md.
///
/// # Safety
/// Deve ser chamada depois de `apic::init()` e `idt::load()` (Local APIC e
/// IDT já prontos), e só uma vez.
pub unsafe fn init<S: FrameStore>(frame_allocator: &mut FrameAllocator<S>) {
    unsafe {
        init_controller();
        crate::idt::set_handler(KEYBOARD_VECTOR, keyboard_irq_handler as *const () as u64, 0);
        crate::ioapic::init(frame_allocator).expect("mapear a pagina MMIO do I/O APIC falhou");
        crate::ioapic::route_irq(KEYBOARD_IRQ, KEYBOARD_VECTOR as u8, crate::apic::local_apic_id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_make_code_from_break_code_of_the_same_base_value() {
        let make = 0x1E; // 'a' pressionada
        let brk = 0x9E; // 'a' solta
        assert!(!is_break_code(make));
        assert!(is_break_code(brk));
        assert_eq!(make_code(brk), make);
    }

    #[test]
    fn recognizes_the_extended_prefix_byte() {
        assert!(is_extended_prefix(0xE0));
        assert!(!is_extended_prefix(0x1E));
    }

    #[test]
    fn letter_without_shift_produces_the_lowercase_character() {
        let mut translator = ScancodeTranslator::new();
        let event = translator.feed(0x1E).unwrap(); // 'a' pressionada
        assert_eq!(event, KeyEvent { pressed: true, ascii: Some(b'a') });
    }

    #[test]
    fn same_letter_with_shift_held_produces_the_uppercase_character() {
        let mut translator = ScancodeTranslator::new();
        translator.feed(LEFT_SHIFT_MAKE_CODE).unwrap(); // Shift pressionado
        let event = translator.feed(0x1E).unwrap(); // 'a' pressionada
        assert_eq!(event, KeyEvent { pressed: true, ascii: Some(b'A') });
    }

    #[test]
    fn pressing_and_releasing_shift_updates_state_without_an_ascii_character() {
        let mut translator = ScancodeTranslator::new();
        let pressed = translator.feed(LEFT_SHIFT_MAKE_CODE).unwrap();
        assert_eq!(pressed, KeyEvent { pressed: true, ascii: None });

        let shifted = translator.feed(0x1E).unwrap();
        assert_eq!(shifted.ascii, Some(b'A'), "Shift deveria estar ativo apos o make code");

        let released = translator.feed(LEFT_SHIFT_MAKE_CODE | 0x80).unwrap();
        assert_eq!(released, KeyEvent { pressed: false, ascii: None });

        let unshifted = translator.feed(0x1E).unwrap();
        assert_eq!(unshifted.ascii, Some(b'a'), "Shift deveria estar inativo apos o break code");
    }

    #[test]
    fn unknown_scancode_produces_no_event() {
        let mut translator = ScancodeTranslator::new();
        // 0x01 (Esc) nao esta em nenhuma tabela de tradução.
        assert_eq!(translator.feed(0x01), None);
    }

    #[test]
    fn byte_preceded_by_extended_prefix_produces_no_event() {
        let mut translator = ScancodeTranslator::new();
        assert_eq!(translator.feed(0xE0), None);
        // Byte que seria uma tecla valida (ex.: 'a'), mas faz parte da
        // sequencia estendida iniciada pelo 0xE0 acima.
        assert_eq!(translator.feed(0x1E), None);
    }

    #[test]
    fn queue_pops_events_in_the_same_order_they_were_pushed() {
        let mut queue = KeyEventQueue::new();
        let a = KeyEvent { pressed: true, ascii: Some(b'a') };
        let b = KeyEvent { pressed: true, ascii: Some(b'b') };
        queue.push(a);
        queue.push(b);
        assert_eq!(queue.pop(), Some(a));
        assert_eq!(queue.pop(), Some(b));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn pushing_past_capacity_drops_the_newest_event_keeping_existing_ones_intact() {
        let mut queue = KeyEventQueue::new();
        for i in 0..QUEUE_CAPACITY {
            queue.push(KeyEvent { pressed: true, ascii: Some(i as u8) });
        }
        // Fila cheia: este evento deveria ser descartado.
        queue.push(KeyEvent { pressed: true, ascii: Some(0xFF) });

        for i in 0..QUEUE_CAPACITY {
            assert_eq!(queue.pop(), Some(KeyEvent { pressed: true, ascii: Some(i as u8) }));
        }
        assert_eq!(queue.pop(), None, "o evento descartado nao deveria ter sido inserido");
    }
}
