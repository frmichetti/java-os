//! Timer periódico do Local APIC e o contador de ticks do kernel — ver
//! `openspec/specs/kernel-timer/spec.md` (quando arquivada) e `design.md`
//! da change `kernel-interrupts-timer`.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::apic::{self, MSR_X2APIC_CURRENT_COUNT, MSR_X2APIC_DIVIDE_CONFIG, MSR_X2APIC_INITIAL_COUNT, MSR_X2APIC_LVT_TIMER};
use crate::port::outb;

/// Vetor da IDT dedicado ao timer.
pub const TIMER_VECTOR: usize = 32;

/// Divisor do timer do APIC: valor codificado para "divide by 16" (ver SDM,
/// Tabela 10-10). O valor concreto do divisor não importa para a spec (que
/// só exige intervalos aproximadamente iguais), apenas que seja consistente
/// entre a calibração e a configuração final.
const DIVIDE_BY_16: u64 = 0b0011;

/// Frequência alvo dos ticks do timer.
const TARGET_FREQUENCY_HZ: u64 = 100;

/// Duração da janela de calibração contra o PIT 8254 (canal 0).
const CALIBRATION_MILLIS: u64 = 10;
const PIT_FREQUENCY_HZ: u64 = 1_193_182;

const PIT_CHANNEL0_DATA: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;
/// Canal 0, acesso lobyte/hibyte, modo 0 (interrupt on terminal count), binário.
const PIT_CMD_CHANNEL0_MODE0: u8 = 0b0011_0000;
/// Comando de "latch" do canal 0, para ler a contagem atual sem reprogramar.
const PIT_CMD_LATCH_CHANNEL0: u8 = 0b0000_0000;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Calcula a contagem inicial do timer do APIC para disparar na
/// `target_frequency_hz`, dada a frequência medida (já considerando o
/// divisor) do timer. Função pura, testável sem hardware.
pub fn initial_count_for_target(measured_frequency_hz: u64, target_frequency_hz: u64) -> u32 {
    (measured_frequency_hz / target_frequency_hz) as u32
}

/// Calcula a frequência do timer do APIC (pós-divisor, em Hz) a partir de
/// quantos ticks do APIC decorreram durante uma janela de calibração de
/// duração conhecida (medida contra uma referência externa, ex.: o PIT).
/// Função pura, testável sem hardware.
pub fn measured_frequency_hz(apic_ticks_elapsed: u64, calibration_millis: u64) -> u64 {
    (apic_ticks_elapsed * 1000) / calibration_millis
}

fn pit_calibration_count(millis: u64) -> u16 {
    ((PIT_FREQUENCY_HZ * millis) / 1000) as u16
}

/// Lê a contagem atual (latched) do canal 0 do PIT.
fn read_pit_count() -> u16 {
    unsafe {
        outb(PIT_COMMAND, PIT_CMD_LATCH_CHANNEL0);
        let low = crate::port::inb(PIT_CHANNEL0_DATA) as u16;
        let high = crate::port::inb(PIT_CHANNEL0_DATA) as u16;
        (high << 8) | low
    }
}

/// Mede a frequência real do timer do APIC (pós-divisor) comparando contra
/// o PIT 8254, cuja frequência é fixa e conhecida.
fn calibrate_apic_timer_frequency_hz() -> u64 {
    let pit_count = pit_calibration_count(CALIBRATION_MILLIS);

    unsafe {
        apic::wrmsr(MSR_X2APIC_DIVIDE_CONFIG, DIVIDE_BY_16);
        apic::wrmsr(MSR_X2APIC_LVT_TIMER, 1 << 16); // mascarado, one-shot (modo 0 por padrão)
        apic::wrmsr(MSR_X2APIC_INITIAL_COUNT, u32::MAX as u64);

        outb(PIT_COMMAND, PIT_CMD_CHANNEL0_MODE0);
        outb(PIT_CHANNEL0_DATA, (pit_count & 0xff) as u8);
        outb(PIT_CHANNEL0_DATA, (pit_count >> 8) as u8);
    }

    // Modo 0 do PIT conta até 0 e para ali (não recarrega sozinho).
    while read_pit_count() != 0 {
        core::hint::spin_loop();
    }

    let remaining = unsafe { apic::rdmsr(MSR_X2APIC_CURRENT_COUNT) };
    let apic_ticks_elapsed = (u32::MAX as u64) - remaining;

    measured_frequency_hz(apic_ticks_elapsed, CALIBRATION_MILLIS)
}

/// Incrementa o contador de ticks e reconhece a interrupção via EOI do
/// Local APIC. Chamada por `kernel::scheduler::on_timer_tick` a cada tick —
/// ver `design.md` da change `kernel-process-scheduler` ("Troca de
/// contexto") para o porquê do vetor do timer não usar mais um handler
/// `extern "x86-interrupt"` comum.
pub fn on_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    apic::send_eoi();
}

/// Calibra e configura o timer do Local APIC para disparo periódico em
/// `TARGET_FREQUENCY_HZ`, instala o vetor de entrada do escalonador
/// (`scheduler::TIMER_ENTRY`) como handler, e habilita a entrega da
/// interrupção (o handler só é efetivamente chamado depois que o chamador
/// executar `sti`).
///
/// # Safety
/// Deve ser chamada depois de `apic::init()` e com a IDT já carregada.
pub unsafe fn init() {
    crate::idt::set_handler(TIMER_VECTOR, crate::scheduler::timer_entry_addr(), 0);

    let frequency_hz = calibrate_apic_timer_frequency_hz();
    let initial_count = initial_count_for_target(frequency_hz, TARGET_FREQUENCY_HZ);

    unsafe {
        apic::wrmsr(MSR_X2APIC_DIVIDE_CONFIG, DIVIDE_BY_16);
        apic::wrmsr(MSR_X2APIC_LVT_TIMER, (TIMER_VECTOR as u64) | (1 << 17)); // modo periódico
        apic::wrmsr(MSR_X2APIC_INITIAL_COUNT, initial_count as u64);
    }
}

/// Contador de ticks, incrementado a cada disparo do timer.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_count_divides_frequency_by_target() {
        assert_eq!(initial_count_for_target(1_000_000, 100), 10_000);
    }

    #[test]
    fn measured_frequency_scales_ticks_to_one_second() {
        // 10_000 ticks em 10ms -> 1_000_000 ticks/s
        assert_eq!(measured_frequency_hz(10_000, 10), 1_000_000);
    }

    #[test]
    fn pit_calibration_count_matches_known_frequency() {
        // 10ms a 1.193.182 Hz -> ~11931 contagens
        assert_eq!(pit_calibration_count(10), 11_931);
    }
}
