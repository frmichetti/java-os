//! Demonstração de verificação de `kernel::clock` — análoga a
//! `kernel::storage_demo`/`kernel::network_demo` (uma operação única, não um
//! fluxo contínuo): reporta a data/hora de parede uma vez, e duas leituras
//! do relógio monotônico espaçadas por uma pausa real, confirmando que a
//! segunda é sempre maior que a primeira — ver design.md da change
//! `kernel-wall-clock`.

use crate::clock;

/// Duração da pausa entre as duas leituras do relógio monotônico — ~0.1s,
/// suficiente para a diferença ser claramente distinguível no log, sem
/// alongar o boot perceptivelmente.
///
/// Implementada como *busy-wait* em torno do próprio `monotonic_nanos()`
/// (não via `kernel::timer::ticks()`/`hlt`): este demo roda **antes** de
/// `idle_loop()` habilitar interrupções pelo resto do boot — usar `hlt`
/// aqui exigiria um `sti` próprio, que ficaria permanentemente ligado dali
/// em diante, habilitando interrupções cedo demais (antes do escalonador/
/// teclado/syscalls terminarem de ser configurados). `monotonic_nanos()` é
/// só uma leitura de `RDTSC`, funciona independente do estado de
/// interrupções — mesmo espírito das esperas curtas e limitadas já usadas
/// durante a calibração do timer/TSC contra o PIT.
const PAUSE_NANOS: u64 = 100_000_000;

/// Roda a demonstração completa pelo console serial. Chamada uma única vez,
/// de forma síncrona (não é uma thread de kernel).
pub fn run() {
    let wall_nanos = clock::wall_clock_nanos();
    let total_seconds = (wall_nanos / 1_000_000_000) as i64;
    let (year, month, day, hour, minute, second) = clock::civil_from_unix_seconds(total_seconds);
    crate::serial_println!(
        "clock: relogio de parede = {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        year,
        month,
        day,
        hour,
        minute,
        second
    );

    let first_reading = clock::monotonic_nanos();
    while clock::monotonic_nanos().wrapping_sub(first_reading) < PAUSE_NANOS {
        core::hint::spin_loop();
    }
    let second_reading = clock::monotonic_nanos();

    if second_reading > first_reading {
        crate::serial_println!(
            "clock: monotonico {} ns -> {} ns (delta={} ns) - PASSOU",
            first_reading,
            second_reading,
            second_reading - first_reading
        );
    } else {
        crate::serial_println!(
            "clock: FALHOU monotonico nao avancou ({} ns -> {} ns)",
            first_reading,
            second_reading
        );
    }
}
