//! Relógio de parede (`CLOCK_REALTIME`) e relógio monotônico de alta
//! resolução (`CLOCK_MONOTONIC`) — a base de `clock_gettime` POSIX. Ver spec
//! `kernel-clock` e design.md da change `kernel-wall-clock`.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::port::{inb, outb};

// ---------------------------------------------------------------------
// Lógica pura: conversão de data civil <-> dias desde a época Unix.
// ---------------------------------------------------------------------

/// Converte uma data civil (ano/mês/dia) em dias desde 1970-01-01 —
/// algoritmo de Howard Hinnant (domínio público), correto para todo o
/// calendário gregoriano proléptico, incluindo anos bissextos. Função pura,
/// testável sem hardware.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11]: Mar=0 .. Feb=11
    let doy = (153 * mp as u64 + 2) / 5 + day as u64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe as i64 - 719468
}

/// Inverso de [`days_from_civil`]: dias desde 1970-01-01 para uma data civil
/// (ano, mês, dia) — mesmo algoritmo de Howard Hinnant. Função pura,
/// testável sem hardware.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

/// Combina data civil e hora do dia em segundos desde a época Unix. Função
/// pura, testável sem hardware.
pub fn civil_to_unix_seconds(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    days_from_civil(year, month, day) * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64
}

/// Inverso de [`civil_to_unix_seconds`]: segundos desde a época Unix para
/// data civil + hora do dia (ano, mês, dia, hora, minuto, segundo). Função
/// pura, testável sem hardware.
pub fn civil_from_unix_seconds(total_seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = total_seconds.div_euclid(86400);
    let seconds_of_day = total_seconds.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    let hour = (seconds_of_day / 3600) as u32;
    let minute = ((seconds_of_day % 3600) / 60) as u32;
    let second = (seconds_of_day % 60) as u32;
    (year, month, day, hour, minute, second)
}

/// Converte ciclos de TSC decorridos em nanossegundos, dada a frequência
/// calibrada do TSC — dividindo antes de multiplicar para nunca estourar
/// `u64` (ver design.md, "Conversão de ciclos de TSC para nanossegundos sem
/// overflow"). Função pura, testável sem hardware.
pub fn cycles_to_nanos(elapsed_cycles: u64, frequency_hz: u64) -> u64 {
    let whole_seconds = elapsed_cycles / frequency_hz;
    let remainder_cycles = elapsed_cycles % frequency_hz;
    whole_seconds * 1_000_000_000 + (remainder_cycles * 1_000_000_000) / frequency_hz
}

// ---------------------------------------------------------------------
// Leitura do relógio CMOS/RTC.
// ---------------------------------------------------------------------

const CMOS_ADDRESS: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

const RTC_REG_SECONDS: u8 = 0x00;
const RTC_REG_MINUTES: u8 = 0x02;
const RTC_REG_HOURS: u8 = 0x04;
const RTC_REG_DAY_OF_MONTH: u8 = 0x07;
const RTC_REG_MONTH: u8 = 0x08;
const RTC_REG_YEAR: u8 = 0x09;
const RTC_REG_STATUS_A: u8 = 0x0A;
const RTC_REG_STATUS_B: u8 = 0x0B;

/// Bit "Update In Progress" do registrador de status A.
const RTC_STATUS_A_UPDATE_IN_PROGRESS: u8 = 1 << 7;
/// Bit do registrador de status B: `0` = campos em BCD, `1` = binário.
const RTC_STATUS_B_BINARY: u8 = 1 << 2;
/// Bit do registrador de status B: `0` = formato 12 horas, `1` = 24 horas.
const RTC_STATUS_B_24_HOUR: u8 = 1 << 1;
/// No formato 12 horas, bit 7 do registrador de horas indica PM.
const RTC_HOUR_PM: u8 = 1 << 7;

fn cmos_read(register: u8) -> u8 {
    unsafe {
        outb(CMOS_ADDRESS, register);
        inb(CMOS_DATA)
    }
}

fn rtc_update_in_progress() -> bool {
    cmos_read(RTC_REG_STATUS_A) & RTC_STATUS_A_UPDATE_IN_PROGRESS != 0
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RawRtcReading {
    second: u8,
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    year: u8,
    status_b: u8,
}

fn read_raw_rtc() -> RawRtcReading {
    RawRtcReading {
        second: cmos_read(RTC_REG_SECONDS),
        minute: cmos_read(RTC_REG_MINUTES),
        hour: cmos_read(RTC_REG_HOURS),
        day: cmos_read(RTC_REG_DAY_OF_MONTH),
        month: cmos_read(RTC_REG_MONTH),
        year: cmos_read(RTC_REG_YEAR),
        status_b: cmos_read(RTC_REG_STATUS_B),
    }
}

/// Lê o RTC de forma estável: espera o bit de "Update In Progress" baixar
/// antes de cada leitura, e só aceita o resultado quando duas leituras
/// sucessivas (cada uma precedida de sua própria espera) baterem — evita
/// capturar um valor em transição durante a atualização de 1x/segundo do
/// chip — ver design.md, "Leitura do RTC".
fn read_rtc_stable() -> RawRtcReading {
    while rtc_update_in_progress() {
        core::hint::spin_loop();
    }
    let mut last = read_raw_rtc();
    loop {
        while rtc_update_in_progress() {
            core::hint::spin_loop();
        }
        let current = read_raw_rtc();
        if current == last {
            return current;
        }
        last = current;
    }
}

fn bcd_to_binary(value: u8) -> u8 {
    (value & 0x0F) + ((value >> 4) * 10)
}

/// Converte uma leitura crua do RTC (que pode vir em BCD ou binário, 12h ou
/// 24h — nunca assumido, sempre conforme o registrador de status B indica)
/// em segundos desde a época Unix, assumindo a década 2000-2099 para o ano
/// de 2 dígitos lido (ver design.md, "Leitura do RTC" — século não
/// implementado deliberadamente).
fn convert_raw_rtc_to_unix_seconds(raw: RawRtcReading) -> i64 {
    let is_binary = raw.status_b & RTC_STATUS_B_BINARY != 0;
    let is_24_hour = raw.status_b & RTC_STATUS_B_24_HOUR != 0;

    let is_pm = raw.hour & RTC_HOUR_PM != 0;
    let hour_field = raw.hour & !RTC_HOUR_PM;

    let (second, minute, hour_raw, day, month, year_2digit) = if is_binary {
        (raw.second, raw.minute, hour_field, raw.day, raw.month, raw.year)
    } else {
        (
            bcd_to_binary(raw.second),
            bcd_to_binary(raw.minute),
            bcd_to_binary(hour_field),
            bcd_to_binary(raw.day),
            bcd_to_binary(raw.month),
            bcd_to_binary(raw.year),
        )
    };

    let mut hour = hour_raw as u32;
    if !is_24_hour {
        if is_pm && hour != 12 {
            hour += 12;
        } else if !is_pm && hour == 12 {
            hour = 0;
        }
    }

    let year = 2000i64 + year_2digit as i64;
    civil_to_unix_seconds(year, month as u32, day as u32, hour, minute as u32, second as u32)
}

// ---------------------------------------------------------------------
// Calibração do TSC contra o PIT 8254 — mesma técnica de
// `kernel::timer::calibrate_apic_timer_frequency_hz`, reaproveitando as
// primitivas de PIT já existentes lá (ver design.md, "Calibração do TSC").
// ---------------------------------------------------------------------

const CALIBRATION_MILLIS: u64 = 10;

fn read_tsc() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
    }
    ((high as u64) << 32) | low as u64
}

/// Mede a frequência do TSC comparando contra o PIT 8254, e devolve também
/// o valor do TSC capturado ao final da janela de calibração — usado
/// diretamente como referência de boot por [`init`], evitando uma leitura
/// adicional (e uma janela extra) depois da calibração.
fn calibrate_tsc() -> (u64, u64) {
    let pit_count = crate::timer::pit_calibration_count(CALIBRATION_MILLIS);

    unsafe { outb(crate::timer::PIT_COMMAND, crate::timer::PIT_CMD_CHANNEL0_MODE0) };
    let start = read_tsc();
    unsafe {
        outb(crate::timer::PIT_CHANNEL0_DATA, (pit_count & 0xff) as u8);
        outb(crate::timer::PIT_CHANNEL0_DATA, (pit_count >> 8) as u8);
    }

    // Modo 0 do PIT conta até 0 e para ali (não recarrega sozinho).
    while crate::timer::read_pit_count() != 0 {
        core::hint::spin_loop();
    }
    let end = read_tsc();

    let frequency_hz = crate::timer::measured_frequency_hz(end - start, CALIBRATION_MILLIS);
    (frequency_hz, end)
}

// ---------------------------------------------------------------------
// Estado global e API pública.
// ---------------------------------------------------------------------

static BOOT_EPOCH_NANOS: AtomicU64 = AtomicU64::new(0);
static TSC_FREQUENCY_HZ: AtomicU64 = AtomicU64::new(0);
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);

/// Inicializa `kernel::clock`: lê o RTC, converte para segundos desde a
/// época, calibra o TSC contra o PIT, e usa o instante final da calibração
/// como referência de boot do relógio monotônico — ver design.md,
/// "`wall_clock_nanos()` reaproveita `monotonic_nanos()`" e "Risco: pequeno
/// erro sistemático". Deve ser chamada uma única vez, depois do PIT já
/// estar disponível (mesmo requisito de `kernel::timer::init`), antes de
/// qualquer uso de [`wall_clock_nanos`]/[`monotonic_nanos`].
pub fn init() {
    let raw = read_rtc_stable();
    let epoch_seconds = convert_raw_rtc_to_unix_seconds(raw);
    let (frequency_hz, reference_tsc) = calibrate_tsc();

    BOOT_EPOCH_NANOS.store((epoch_seconds as u64) * 1_000_000_000, Ordering::Relaxed);
    TSC_FREQUENCY_HZ.store(frequency_hz, Ordering::Relaxed);
    BOOT_TSC.store(reference_tsc, Ordering::Relaxed);
}

/// Nanossegundos decorridos desde [`init`] — equivalente a `CLOCK_MONOTONIC`,
/// base de `System.nanoTime()`. Nunca retrocede; resolução de nanossegundos
/// via TSC, não os 10ms de `kernel::timer::ticks()`.
pub fn monotonic_nanos() -> u64 {
    let now = read_tsc();
    let elapsed_cycles = now.wrapping_sub(BOOT_TSC.load(Ordering::Relaxed));
    cycles_to_nanos(elapsed_cycles, TSC_FREQUENCY_HZ.load(Ordering::Relaxed))
}

/// Nanossegundos desde a época Unix (1970-01-01 00:00:00 UTC) — equivalente
/// a `CLOCK_REALTIME`, base de `System.currentTimeMillis()`.
pub fn wall_clock_nanos() -> u64 {
    BOOT_EPOCH_NANOS.load(Ordering::Relaxed) + monotonic_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_from_civil_epoch_is_zero() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn days_from_civil_known_reference_date() {
        // 2000-03-01 é uma data de referência bem conhecida para este
        // algoritmo: exatamente 11017 dias desde a época.
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
    }

    #[test]
    fn days_from_civil_handles_leap_year() {
        // 2024 é bissexto: 2024-02-29 existe, e 2024-03-01 é exatamente um
        // dia depois.
        let feb29 = days_from_civil(2024, 2, 29);
        let mar1 = days_from_civil(2024, 3, 1);
        assert_eq!(mar1 - feb29, 1);
    }

    #[test]
    fn civil_from_days_is_the_inverse_of_days_from_civil() {
        for (year, month, day) in [(1970, 1, 1), (2000, 3, 1), (2024, 2, 29), (2026, 11, 3), (1969, 12, 31)] {
            let days = days_from_civil(year, month, day);
            assert_eq!(civil_from_days(days), (year, month, day));
        }
    }

    #[test]
    fn civil_to_unix_seconds_epoch_is_zero() {
        assert_eq!(civil_to_unix_seconds(1970, 1, 1, 0, 0, 0), 0);
    }

    #[test]
    fn civil_round_trips_through_unix_seconds() {
        let original = (2026, 11, 3, 14, 30, 45);
        let (y, mo, d, h, mi, s) = original;
        let seconds = civil_to_unix_seconds(y, mo, d, h, mi, s);
        assert_eq!(civil_from_unix_seconds(seconds), original);
    }

    #[test]
    fn cycles_to_nanos_exact_frequency_is_one_second() {
        let frequency_hz = 3_000_000_000u64;
        assert_eq!(cycles_to_nanos(frequency_hz, frequency_hz), 1_000_000_000);
    }

    #[test]
    fn cycles_to_nanos_does_not_overflow_where_naive_multiplication_would() {
        // Multiplicar antes de dividir (elapsed_cycles * 1_000_000_000)
        // estouraria u64 aqui: ~30 bilhões de ciclos * 1e9 > u64::MAX.
        let frequency_hz = 3_000_000_000u64; // 3 GHz
        let elapsed_cycles = frequency_hz * 10; // 10 segundos de uptime
        assert_eq!(cycles_to_nanos(elapsed_cycles, frequency_hz), 10_000_000_000);
    }

    #[test]
    fn bcd_to_binary_converts_known_values() {
        assert_eq!(bcd_to_binary(0x00), 0);
        assert_eq!(bcd_to_binary(0x09), 9);
        assert_eq!(bcd_to_binary(0x10), 10);
        assert_eq!(bcd_to_binary(0x59), 59);
    }

    #[test]
    fn convert_raw_rtc_handles_binary_24_hour_format() {
        let raw = RawRtcReading {
            second: 45,
            minute: 30,
            hour: 14,
            day: 3,
            month: 11,
            year: 26,
            status_b: RTC_STATUS_B_BINARY | RTC_STATUS_B_24_HOUR,
        };
        assert_eq!(convert_raw_rtc_to_unix_seconds(raw), civil_to_unix_seconds(2026, 11, 3, 14, 30, 45));
    }

    #[test]
    fn convert_raw_rtc_handles_bcd_12_hour_pm_format() {
        // 02:30:45 PM em BCD, 12 horas — equivalente a 14:30:45.
        let raw = RawRtcReading {
            second: 0x45,
            minute: 0x30,
            hour: 0x02 | RTC_HOUR_PM,
            day: 0x03,
            month: 0x11,
            year: 0x26,
            status_b: 0, // nem binário, nem 24h
        };
        assert_eq!(convert_raw_rtc_to_unix_seconds(raw), civil_to_unix_seconds(2026, 11, 3, 14, 30, 45));
    }

    #[test]
    fn convert_raw_rtc_handles_bcd_12_hour_midnight() {
        // 12:00:00 AM em formato 12h é meia-noite (hora 0), não hora 12.
        let raw = RawRtcReading {
            second: 0,
            minute: 0,
            hour: 0x12, // sem o bit de PM
            day: 0x01,
            month: 0x01,
            year: 0x26,
            status_b: 0,
        };
        assert_eq!(convert_raw_rtc_to_unix_seconds(raw), civil_to_unix_seconds(2026, 1, 1, 0, 0, 0));
    }
}
