# Tasks

## 1. Lógica pura: conversão de data civil e de ciclos de TSC

- [x] 1.1 Implementar `days_from_civil(year: i64, month: u32, day: u32) -> i64` (algoritmo de Howard Hinnant, ver design.md) e uma função que combina isso com hora/minuto/segundo em segundos desde a época Unix. Verificar com testes unitários em host: `1970-01-01 00:00:00` → `0`; uma data conhecida de referência (ex.: `2000-03-01 00:00:00`); um ano bissexto tratado corretamente (ex.: `2024-02-29`).
- [x] 1.2 Implementar `cycles_to_nanos(elapsed_cycles: u64, frequency_hz: u64) -> u64` (divide antes de multiplicar, ver design.md — "Conversão de ciclos de TSC... sem overflow"). Verificar com testes unitários em host: um caso que estouraria `u64` se multiplicado ingenuamente antes de dividir não estoura com esta implementação; resultado correto para valores pequenos conhecidos (ex.: `frequency_hz` ciclos em exatamente 1 segundo → `1_000_000_000` ns).

## 2. `kernel::clock`: leitura do RTC e calibração do TSC

- [x] 2.1 Implementar a leitura do relógio CMOS/RTC (portas 0x70/0x71): esperar o bit de "Update In Progress" baixar, ler os campos, esperar o bit subir de novo, reler, e só aceitar quando as duas leituras baterem; aplicar conversão de BCD para binário e de 12h para 24h conforme os bits do registrador de status B indicarem (nunca assumidos) — ver design.md, "Leitura do RTC". Verificar que `cargo kernel-build` compila (a leitura real de hardware só é verificável em boot real).
- [x] 2.2 Implementar a calibração do TSC contra o PIT 8254 (mesma técnica de `kernel::timer::calibrate_apic_timer_frequency_hz`, mas medindo `RDTSC` em vez do contador do APIC — ver design.md, "Calibração do TSC"). Verificar que `cargo kernel-build` compila.
- [x] 2.3 Implementar `kernel::clock::init()`: lê o RTC (tarefa 2.1), converte para segundos desde a época (tarefa 1.1), calibra o TSC (tarefa 2.2) e captura o valor de `RDTSC` de referência do boot, nessa ordem (minimizando a janela entre a leitura do RTC e a captura da referência do TSC — ver design.md, "Risco: pequeno erro sistemático"). Verificar que `cargo kernel-build` compila.

## 3. `kernel::clock`: API pública

- [x] 3.1 Implementar `kernel::clock::monotonic_nanos() -> u64` (ciclos de TSC decorridos desde a referência de boot, convertidos via `cycles_to_nanos` da tarefa 1.2). Verificar que `cargo kernel-build` compila.
- [x] 3.2 Implementar `kernel::clock::wall_clock_nanos() -> u64` (segundos desde a época lidos no boot, convertidos para nanossegundos, mais `monotonic_nanos()` — ver design.md, "`wall_clock_nanos()` reaproveita `monotonic_nanos()`"). Verificar que `cargo kernel-build` compila.

## 4. Demonstração e verificação em boot real

- [x] 4.1 Implementar `kernel::clock_demo`: reporta a data/hora de `wall_clock_nanos()` (convertida de volta para data civil legível) pelo console serial uma vez; faz duas leituras de `monotonic_nanos()` espaçadas por uma pausa real (ex.: aguardando um número conhecido de ticks de `kernel::timer`), reportando ambas e confirmando que a segunda é maior que a primeira. Verificar que `cargo kernel-build` compila.
- [x] 4.2 Em `continue_boot` (`kernel/src/main.rs`), inicializar `kernel::clock` (depois do PIT já estar disponível — mesmo ponto de `kernel::timer::init()`) e chamar `kernel::clock_demo::run()`. Verificar com `cargo xtask test` que o boot continua passando sem regressão no console serial.
- [x] 4.3 Confirmar em boot real (console serial) que a data/hora reportada corresponde à hora real do host/VM no momento do teste (dentro de uma margem de minutos, considerando fuso/UTC), e que a segunda leitura do relógio monotônico é sempre maior que a primeira, em múltiplas execuções. Documentar o resultado em design.md, como já é costume neste projeto.
- [x] 4.4 Confirmar `cargo test -p kernel --lib` (suíte completa de host) sem regressão, incluindo os novos testes das tarefas 1.1 e 1.2.
- [x] 4.5 Atualizar `docs/architecture/overview.md`: seção 1 (Kernel) ou uma nova subseção dedicada a relógio/tempo, registrando `kernel::clock` (`wall_clock_nanos`/`monotonic_nanos`), e o item 8 do roadmap (seção 15), registrando esta sub-change como concluída (sem marcar o item 8 inteiro — ainda faltam *syscalls* para expor isso a modo usuário, sinal de safepoint, sistema de arquivos, linking dinâmico, e o próprio porte do HotSpot). Verificar lendo o arquivo atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para as modificações propostas.
