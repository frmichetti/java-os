# Design

## Context

Ver `proposal.md` — Why/What Changes. Estado atual relevante, pesquisado no código real antes deste design:

- `kernel::timer` já calibra o timer do Local APIC contra o PIT 8254 (`calibrate_apic_timer_frequency_hz`, em `kernel/src/timer.rs`): programa o canal 0 do PIT em modo 0 (contagem regressiva, interrupção ao terminar) para uma janela de `CALIBRATION_MILLIS` (10ms), espera a contagem chegar a zero fazendo *polling* da porta de dados do PIT, e mede quantos ticks do APIC se passaram nessa janela conhecida. A mesma técnica (janela de tempo conhecida via PIT, medir um contador interno antes/depois) se aplica diretamente a calibrar o *Time Stamp Counter* (TSC) da CPU.
- `kernel::port::{inb, outb}` já existem e são usados por vários drivers (serial, PIC, PIT) — mesmas primitivas necessárias para acessar o relógio CMOS/RTC (portas 0x70/0x71).
- Nenhuma suposição de ACPI/MADT é feita em nenhum lugar do kernel (`kernel-ioapic`: "endereço MMIO fixo... sem parsing de ACPI/MADT"; `kernel-storage`/`kernel-pci`: só barramento 0) — mesmo espírito de simplificação pragmática, documentada, se aplica ao registrador de século do CMOS (não padronizado entre implementações; nem sempre exposto da mesma forma).
- Este kernel roda primariamente sob QEMU/TCG (ver `KNOW_ISSUES.md` e as limitações de timing já documentadas) — o TSC emulado por QEMU tem taxa fixa, sem as complicações de *frequency scaling*/TSC não-invariante de hardware real. Validar em hardware real fica para o item 12 do roadmap ("Hardening multi-hardware"), não nesta change.

## Goals / Non-Goals

**Goals:**
- `wall_clock_nanos()` (equivalente a `CLOCK_REALTIME`): nanossegundos desde a época Unix, derivados de uma leitura única do relógio CMOS/RTC no boot.
- `monotonic_nanos()` (equivalente a `CLOCK_MONOTONIC`): nanossegundos decorridos desde a inicialização, via TSC calibrado — resolução muito mais fina que os 10ms do contador de ticks já existente (`kernel::timer::ticks()`), que continua inalterado.
- Lógica pura e testável em host sempre que possível: conversão de data civil (ano/mês/dia/hora/min/seg) para segundos desde a época, e conversão de ciclos de TSC decorridos para nanossegundos (ambas sem nenhum acesso a hardware).
- Um demo em modo kernel que prova os dois relógios em boot real.

**Non-Goals (nesta change):**
- Qualquer *syscall* nova expondo estes relógios a modo usuário — mesmo adiamento já aplicado a `kernel::vmm`/`kernel::wait_queue`/`kernel::pthread`.
- Ajuste de relógio em tempo de execução (NTP ou equivalente) — `wall_clock_nanos()` só avança a partir do valor lido do RTC no boot, nunca é corrigido depois.
- Fusos horários — sempre UTC (mesma convenção do próprio `CLOCK_REALTIME` e do RTC de uma VM tipicamente configurada em UTC).
- Detectar/validar que o TSC é invariante (via `CPUID`) ou lidar com *frequency scaling* de hardware real — assume-se taxa fixa, adequado ao ambiente de teste atual (QEMU/TCG); revisitar quando o item 12 do roadmap (hardware real) for de fato trabalhado.
- Século do RTC via registrador dedicado (não padronizado) — assume-se a década atual (2000-2099) a partir do ano de 2 dígitos lido, documentado explicitamente como simplificação (mesmo espírito de `kernel-ioapic`/`kernel-pci`).

## Decisions

### Leitura do RTC: esperar o período "quieto" entre atualizações, não parsear o registrador de século
O chip RTC (CMOS) atualiza seus registradores de data/hora uma vez por segundo; ler no meio dessa atualização pode devolver um valor corrompido (parte do segundo antigo, parte do novo). Técnica padrão, usada aqui: esperar o bit "*Update In Progress*" (registrador de status A, bit 7) baixar, ler todos os campos, esperar o bit subir de novo (confirma que a leitura anterior aconteceu dentro da janela quieta), ler os campos de novo, e comparar — só aceitar o resultado quando as duas leituras baterem (senão repetir). Os campos (segundos, minutos, horas, dia, mês, ano) podem vir em BCD ou binário, e a hora em formato 12 ou 24 horas — o registrador de status B (bits 2 e 1) diz qual; a conversão (BCD→binário, 12h→24h) é aplicada conforme indicado, nunca assumida.

**Não implementado**: localizar e ler o registrador de século (posição não padronizada entre BIOS/UEFI — às vezes exposto via o campo `CENTURY` da tabela ACPI FADT, às vezes num registrador CMOS fixo que varia por fabricante). Em vez disso, o ano de 2 dígitos lido é assumido como `2000 + ano` — correto para qualquer data entre 2000 e 2099, cobrindo toda a vida útil esperada deste kernel sem a complexidade de localizar/parsear ACPI (que o projeto já evita deliberadamente em outros pontos).

### Conversão de data civil para segundos desde a época: algoritmo puro, testável em host
Data (ano, mês, dia) → número de dias desde 1970-01-01 usa o algoritmo de Howard Hinnant (`days_from_civil`, de domínio público, amplamente usado em bibliotecas de data/hora C++): uma fórmula fechada, sem *loop*, correta para todo o calendário gregoriano proléptico, incluindo anos bissextos. Combinado com hora/minuto/segundo, dá segundos desde a época. Função pura (`fn days_from_civil(year: i64, month: u32, day: u32) -> i64`), sem nenhum acesso a hardware — testável em host com datas conhecidas (ex.: 1970-01-01 → dia 0; 2000-03-01 → valor conhecido de referência).

### Calibração do TSC contra o PIT: mesma técnica já usada para o timer do APIC, não compartilhada via abstração nova
`kernel::clock` programa o PIT para uma janela de tempo conhecida (mesmo `CALIBRATION_MILLIS` de `kernel::timer`, ou um valor próprio) e mede `RDTSC` antes/depois, igual em estrutura a `calibrate_apic_timer_frequency_hz` (só troca "ler o contador do APIC" por "ler o TSC"). Deliberadamente **não** extraída para uma função genérica compartilhada entre os dois módulos: é usada uma única vez em cada um, a duplicação é pequena (a parte específica de cada medição, não o laço de espera do PIT em si), e introduzir uma abstração para isso violaria a preferência do projeto por evitar abstrações prematuras.

### Conversão de ciclos de TSC para nanossegundos sem overflow: dividir antes de multiplicar
`elapsed_cycles * 1_000_000_000 / frequency_hz`, feito ingenuamente nessa ordem, estoura `u64` depois de só alguns segundos de tempo de atividade (ex.: a ~3 bilhões de ciclos/s, `elapsed_cycles * 1e9` já se aproxima do limite de `u64` com menos de 10s de uptime). Em vez disso: separa em segundos inteiros e o resto —
```
segundos = elapsed_cycles / frequency_hz
resto    = elapsed_cycles % frequency_hz
nanos    = segundos * 1_000_000_000 + resto * 1_000_000_000 / frequency_hz
```
`resto` é sempre menor que `frequency_hz`, então `resto * 1e9` nunca estoura para frequências de CPU realistas (ordens de grandeza abaixo do limite de `u64`), e `segundos * 1e9` só estouraria depois de centenas de anos de tempo de atividade contínuo — não uma preocupação real. Função pura (`fn cycles_to_nanos(elapsed_cycles: u64, frequency_hz: u64) -> u64`), testável em host com valores conhecidos.

### `wall_clock_nanos()` reaproveita `monotonic_nanos()` em vez de reler o RTC
`wall_clock_nanos()` = (segundos desde a época lidos do RTC no boot, convertidos para nanossegundos) + `monotonic_nanos()` (tempo decorrido desde esse exato instante, via TSC). Isso evita qualquer necessidade de reler o RTC (lento, não pensado para *polling*) depois do boot, e automaticamente herda a resolução fina do relógio monotônico.

## Achados durante a implementação

**Ajuste de implementação em relação ao plano original**: `kernel::timer::pit_calibration_count`/`read_pit_count`/as constantes de porta do PIT foram promovidas de privadas para `pub(crate)` e reaproveitadas diretamente por `kernel::clock`, em vez de duplicadas como o design original previa ("não compartilhada via abstração nova"). Na prática, essas funções já existiam prontas e idênticas ao que `kernel::clock` precisava — reaproveitá-las via uma simples mudança de visibilidade não introduz nenhuma abstração nova (só evita copiar código já existente), o que é consistente com a preferência geral do projeto por evitar duplicação desnecessária. A única parte que permanece deliberadamente não-compartilhada é o laço de calibração em si (a lógica específica de cada medição — ciclos de TSC vs. contador do APIC), como o design.md original já previa.

**Achado na integração em `continue_boot`**: a primeira versão do demo (`kernel::clock_demo`) usava `kernel::timer::ticks()` + `hlt` para pausar entre as duas leituras do relógio monotônico — mas isso exigiria um `sti` próprio (já que o demo roda **antes** de `idle_loop()` habilitar interrupções pelo resto do boot), que ficaria permanentemente ligado dali em diante, habilitando interrupções cedo demais (antes do escalonador/teclado/syscalls terminarem de ser configurados em `continue_boot`) — uma regressão real de comportamento, pega antes de qualquer teste em boot real. Corrigido trocando a pausa por um *busy-wait* em torno do próprio `monotonic_nanos()` (`RDTSC`, funciona independente do estado de interrupções) — mesmo espírito das esperas curtas já usadas durante a calibração do timer/TSC contra o PIT.

**Verificado em boot real (QEMU), 5 execuções independentes:** em todas, a data/hora de parede reportada bateu com a hora real do host no momento do teste (diferença de 10-20s, compatível com o tempo de build+boot da imagem, nunca horas/dias de diferença — confirma que a leitura do RTC, a conversão BCD/12h-24h e a suposição de década 2000-2099 estão corretas para o ambiente de teste atual), e o relógio monotônico sempre avançou entre as duas leituras, com um delta consistente de ~100.03-100.06ms para uma pausa configurada de exatamente 100ms (confirma a calibração do TSC contra o PIT).

## Riscos / Trade-offs

- **[Risco] Pequeno erro sistemático entre o instante em que o RTC é lido e o instante em que o TSC é "zerado" como referência de boot** → Aceito: a ordem de inicialização (ler RTC, imediatamente capturar o TSC de referência) minimiza a janela, mas não a elimina — um erro da ordem de microssegundos é irrelevante para o caso de uso (`System.currentTimeMillis()` nunca prometeu precisão de microssegundos).
- **[Trade-off] Século do RTC assumido como 2000-2099, sem ler nenhum registrador de século** → Aceito, documentado explicitamente (ver Non-Goals); revisitar só se este kernel precisar rodar além de 2099 (improvável ser uma preocupação real).
- **[Trade-off] TSC assumido de taxa fixa, sem checar invariância via `CPUID`** → Aceito para o ambiente de teste atual (QEMU/TCG); revisitar junto do item 12 do roadmap (hardware real).
- **[Risco] `RDTSC` pode, em teoria, ser reordenado pela CPU em relação a instruções vizinhas (não é serializante)** → Aceito: para medir uma janela de calibração de 10ms ou o tempo decorrido entre duas leituras normais de `monotonic_nanos()` (não para sincronização precisa entre núcleos, que este kernel single-core não precisa), a imprecisão de reordenação (poucos ciclos) é desprezível perto da resolução que de fato importa aqui.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: `cargo test -p kernel --lib` sem regressão (testes novos cobrindo `days_from_civil`/`cycles_to_nanos`, lógica pura), `cargo kernel-build`/`cargo xtask test` sem regressão de comportamento esperado no console serial, e verificação manual em boot real confirmando que a data/hora relatada corresponde à hora real do host/VM e que o relógio monotônico cresce de forma consistente entre duas leituras. Rollback é reverter os commits desta change; nenhum dado persistente é afetado.
