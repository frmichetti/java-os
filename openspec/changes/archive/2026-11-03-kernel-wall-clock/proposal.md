# Proposal

## Why

A change `jvm-posix-compat-strategy` (arquivada) listou `clock_gettime` entre as primitivas POSIX mínimas exigidas pelo HotSpot, para `System.nanoTime()`/`System.currentTimeMillis()`: "`kernel::timer` já conta ticks desde o boot; falta relógio de parede real e, possivelmente, resolução mais fina". O kernel hoje só tem `kernel::timer::ticks()` — um contador monotônico de ticks do timer do Local APIC a 100 Hz (10ms de resolução), sem nenhuma noção de data/hora real (`CLOCK_REALTIME`) e sem resolução mais fina que 10ms (`System.nanoTime()` não exige uma resolução específica, mas 10ms é grosseiro demais para medir a maioria dos intervalos que código Java mede com ele).

Diferente de sinal de safepoint, sistema de arquivos e carregamento dinâmico (os outros gaps restantes do item 8 do roadmap), relógio de parede é independente dos demais, autocontido, e de risco bem menor — não exige nenhuma mudança em memória, exceções ou carregamento de código, só leitura de dois periféricos de hardware já bem documentados (o relógio CMOS/RTC e o Time Stamp Counter da CPU).

## What Changes

- Novo módulo `kernel::clock`, com duas funções nomeadas (mapeando diretamente para os dois relógios que `clock_gettime` POSIX precisa diferenciar):
  - `wall_clock_nanos() -> u64`: nanossegundos desde a época Unix (1970-01-01 00:00:00 UTC) — equivalente a `CLOCK_REALTIME`, base de `System.currentTimeMillis()`. Calculado lendo o relógio CMOS/RTC **uma única vez**, no boot (convertendo data/hora para segundos desde a época), e somando o tempo decorrido desde essa leitura (via o relógio monotônico de alta resolução abaixo) — nunca relendo o RTC repetidamente (leitura de RTC é relativamente lenta e não pensada para polling frequente).
  - `monotonic_nanos() -> u64`: nanossegundos decorridos desde a inicialização deste relógio (nunca diminui, independente de qualquer ajuste de data/hora) — equivalente a `CLOCK_MONOTONIC`, base de `System.nanoTime()`. Resolução de nanossegundos de verdade (não os 10ms do contador de ticks existente), via o Time Stamp Counter (`RDTSC`) da CPU, calibrado contra o PIT 8254 — mesma técnica já usada por `kernel::timer::calibrate_apic_timer_frequency_hz` para calibrar o timer do Local APIC.
- **Não inclui ainda** (fica para sub-changes futuras do item 8): nenhuma *syscall* nova expondo esses relógios a modo usuário (mesmo adiamento já aplicado a `kernel::vmm`/`kernel::wait_queue`/`kernel::pthread`: só existe um consumidor de modo usuário real quando o porte da JVM em si existir), ajuste de relógio em tempo de execução (NTP ou equivalente — o relógio de parede, uma vez calculado no boot, só avança, nunca é corrigido), e fusos horários (`wall_clock_nanos` é sempre UTC, como o próprio `CLOCK_REALTIME`/o relógio CMOS de uma VM tipicamente configurada em UTC).
- Um demo em modo kernel (`kernel::clock_demo`) prova os dois relógios: reporta a data/hora de parede lida do RTC uma vez, e duas leituras de `monotonic_nanos()` espaçadas no tempo, confirmando que a segunda é sempre maior que a primeira e que a diferença corresponde, aproximadamente, ao intervalo real decorrido.
- Nenhuma mudança de comportamento observável para o código já existente (`kernel::timer::ticks()` continua existindo e sendo usado exatamente como hoje pelo escalonador/demos — `kernel::clock` é uma camada adicional, não uma substituição).

## Capabilities

### New Capabilities
- `kernel-clock`: relógio de parede (`CLOCK_REALTIME` — data/hora real desde a época Unix) e relógio monotônico de alta resolução (`CLOCK_MONOTONIC` — nanossegundos decorridos, baseado no Time Stamp Counter) — a base de `clock_gettime` POSIX.

### Modified Capabilities

Nenhuma — `kernel-timer` (o contador de ticks do timer do Local APIC a 100 Hz) continua exatamente como está; `kernel-clock` é uma capacidade nova e independente, só consumindo o PIT 8254 (já usado por `kernel-timer` para calibração) como referência de tempo real durante a própria calibração do TSC.

## Impact

- Novo módulo `kernel::clock` (`wall_clock_nanos`, `monotonic_nanos`, e a leitura/calibração internas) e `kernel::clock_demo` (a demonstração em modo kernel).
- `kernel/src/main.rs`: `continue_boot` ganha a inicialização de `kernel::clock` e a chamada de `kernel::clock_demo::run()`, em algum ponto depois do timer/PIT já estarem disponíveis (mesmo ponto, ou próximo, de onde `kernel::timer::init()` já é chamado).
- Nenhuma mudança em `kernel::timer`, `kernel::apic` ou `kernel::scheduler`.
- `docs/architecture/overview.md`: seção 1 (Kernel) ou uma nova subseção dedicada, e o item 8 do roadmap (seção 15), ganham uma atualização registrando esta sub-change concluída.
