# Proposal

## Why

A change `jvm-posix-compat-strategy` (arquivada) decidiu a estratégia de porte da JVM e inventariou as primitivas POSIX que o HotSpot exige no mínimo. O maior gap estrutural identificado: o kernel hoje só tem `kernel::sync::Spinlock` (exclusão mútua por *busy-wait*) — **não existe nenhuma primitiva real de bloqueio/sono de thread** (uma thread que fica esperando sai da fila de execução do escalonador até ser explicitamente acordada, em vez de consumir CPU girando). Isso é pré-requisito de `pthread_mutex`/`pthread_cond` (base de `PlatformEvent`/`Parker` do HotSpot — ou seja, de `park`/`unpark`, `Object.wait`/`notify`, `java.util.concurrent`), mas é útil por si só, independente da JVM: qualquer consumidor futuro que precise esperar por um evento (ex.: uma IRQ de dispositivo) sem girar em *busy-wait*. Por ser grande demais para uma só change (mesmo padrão já usado para quebrar o item 6 do roadmap em 4 changes sequenciais), esta é a primeira sub-change do item 8 ("Porte inicial da JVM").

## What Changes

- Acrescenta ao escalonador existente (`kernel::scheduler`) uma primitiva de **fila de espera** (`WaitQueue`): uma thread em execução pode bloquear voluntariamente numa `WaitQueue` (sai da rotação do escalonador, sem consumir CPU) até outra thread — ou uma ISR, ex.: uma IRQ de teclado/rede — acordá-la (`wake_one`/`wake_all`), que a devolve à rotação normal.
- O mecanismo de bloqueio reaproveita a interrupção de software (`int` na mesma *vector* já usada pelo timer) para reentrar no caminho já existente de salvar/restaurar contexto (`timer_entry`/`on_timer_tick`), em vez de uma rotina de troca de contexto nova e separada — ver design.md para a análise completa e as alternativas descartadas.
- Um demo em modo kernel (`kernel::wait_demo`, análogo a `keyboard_demo`/`storage_demo`/`network_demo`) prova o mecanismo: uma thread "produtora" acorda periodicamente uma thread "consumidora" bloqueada, que reporta cada despertar pelo console serial.
- **Não inclui ainda** (fica para sub-changes futuras do item 8): `pthread_mutex`/`pthread_cond` como API nomeada, qualquer *syscall* nova exposta a modo usuário, bloqueio com *timeout*, ou qualquer código do HotSpot/OpenJDK em si.
- Nenhuma mudança de comportamento observável para o código já existente (teclado, armazenamento, rede, modo usuário) — todas as threads hoje em rotação continuam se comportando exatamente como antes; a fila de espera é um mecanismo adicional, opcional, usado apenas pelo novo demo nesta change.

## Capabilities

### New Capabilities

Nenhuma — a fila de espera é uma extensão do comportamento do escalonador já especificado em `kernel-scheduler`, não uma capacidade nova e independente.

### Modified Capabilities

- `kernel-scheduler`: adiciona o requisito de que uma thread pode bloquear voluntariamente (sair da rotação, sem busy-wait) até ser explicitamente acordada por outra thread ou por uma rotina de interrupção, preservando intactos os requisitos já existentes (escalonamento round-robin preemptivo, preservação de contexto, stack de kernel confiável, comportamento ocioso sem threads prontas).

## Impact

- `kernel/src/scheduler.rs`: `ReadyQueue` é refatorada para permitir desviar uma thread da rotação normal (em vez de sempre recolocá-la no fim da fila) e `on_timer_tick` passa a consultar essa decisão antes de decidir o destino da thread interrompida.
- Novo módulo `kernel::wait_queue` (a primitiva em si, com lógica pura testável em host) e `kernel::wait_demo` (a demonstração em modo kernel).
- `kernel/src/main.rs`: `continue_boot` ganha o spawn de duas novas threads de demonstração (produtora/consumidora), no mesmo ponto onde `keyboard_demo` já é iniciado.
- Nenhuma mudança em `kernel::hal` (esta primitiva não é um dispositivo físico, não se encaixa no padrão de trait de HAL já estabelecido).
- `docs/architecture/overview.md`: seção 3 (Processos e Threads) e o item 8 do roadmap (seção 15) ganham uma atualização registrando esta primeira sub-change concluída.
