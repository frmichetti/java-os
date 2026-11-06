# Arquitetura do Java OS — Visão Geral

> Este documento consolida, como documentação permanente do projeto, a arquitetura
> técnica de referência definida originalmente em
> `openspec/changes/kernel-boot-foundation/design.md` (seção "Arquitetura do Sistema
> (Referência)"). Ele cobre as 15 áreas de arquitetura do Java OS. Apenas as áreas
> 1 (Kernel, incluindo interrupções/timer e stack própria com guard page), 2 (Memória,
> exceto huge pages), 3 (Processos e Threads), 4 (System Calls, exceto isolamento de
> falhas — ver a seção; o carregador de executáveis ELF64 estáticos e o linker
> dinâmico mínimo já existem), e 12-14 (repositório/build/testes) estão implementadas
> até o momento (changes `kernel-boot-foundation`, `kernel-memory-management`,
> `kernel-interrupts-timer`, `kernel-stack-guard-page`, `kernel-process-scheduler`,
> `kernel-syscalls`, `kernel-posix-virtual-memory`, `kernel-wait-queue`,
> `kernel-pthread-sync`, `kernel-wall-clock`, `kernel-elf-loader`,
> `kernel-dynamic-linker`, `kernel-fat32-filesystem` e `kernel-file-syscalls`);
> as demais ficam registradas aqui para orientar changes futuras.

## 1. Arquitetura do Kernel

- **Linguagem/plataforma**: Rust `no_std` + Assembly x86_64 para os trechos que exigem
  controle direto (entrada de boot, troca de contexto, IDT/GDT stubs, instruções
  privilegiadas sem equivalente seguro em Rust).
- **Modelo de boot**: UEFI exclusivamente, via bootloader **Limine** (Limine boot
  protocol), que entrega ao kernel: mapa de memória, framebuffer, RSDP/ACPI,
  informações de módulos carregados, e outras boot requests.
- **Modelo de execução do kernel**: monolítico modular — um único espaço de
  endereçamento privilegiado (anel 0) contendo gerenciamento de memória, escalonador,
  drivers e IPC, organizado em crates Rust internas por subsistema (não microkernel).
- **Interrupções e exceções**: IDT e GDT/TSS próprios (`kernel::idt`/`kernel::gdt`,
  change `kernel-interrupts-timer`), substituindo os que o Limine deixa montados no
  boot. Handlers para as exceções de CPU essenciais (page fault, general protection
  fault, double fault — double fault e page fault com stacks dedicadas via entradas
  de IST no TSS, para continuar sendo entregues mesmo diante de um estouro da stack
  corrente; ver `kernel-stack-guard-page`). Um handler padrão (reconhece via EOI do
  Local APIC e retorna) cobre qualquer vetor de IRQ sem handler específico.
  Interrupções via Local APIC em modo **x2APIC** (MSR-based, obrigatório — sem
  fallback para xAPIC/MMIO); o PIC 8259 legado é desabilitado. O timer do Local APIC
  é calibrado contra o PIT 8254 e configurado para disparo periódico, mantendo um
  contador de ticks (`kernel::timer`) ainda não consumido por nenhum escalonador.
- **Stack do kernel**: desde a change `kernel-stack-guard-page`, o kernel troca da
  stack fornecida pelo Limine para uma stack própria (`kernel::stack`, 64 KiB, faixa
  virtual isolada) assim que o alocador de frames/paginação ficam prontos, com a
  página imediatamente abaixo do seu limite inferior deliberadamente não mapeada
  (guard page). Um estouro de stack resulta em page fault imediato e diagnosticável
  (endereço dentro da guard page), em vez de corromper memória adjacente
  silenciosamente.
- **Concorrência no kernel**: inicialmente single-core; SMP é adiado para uma change
  futura, mas as estruturas de dados já nascem pensando em `Sync`/`Send`.
- **Relógio de parede e monotônico** (`kernel-wall-clock`, `kernel::clock`): quarta
  sub-change do item 8 do roadmap — base de `clock_gettime` POSIX
  (`System.currentTimeMillis()`/`System.nanoTime()` do HotSpot). `wall_clock_nanos()`
  (`CLOCK_REALTIME`) lê o relógio CMOS/RTC uma única vez no boot (esperando o período
  "quieto" entre atualizações, convertendo BCD/12h-24h conforme os bits do
  registrador de status B indicarem, assumindo a década 2000-2099 para o ano de 2
  dígitos — sem parsear o registrador de século, mesmo espírito de simplificação já
  usado por `kernel-ioapic`/`kernel-pci`) e soma o tempo decorrido desde então.
  `monotonic_nanos()` (`CLOCK_MONOTONIC`) usa o *Time Stamp Counter* da CPU,
  calibrado contra o PIT 8254 com a mesma técnica já usada para o timer do Local
  APIC — resolução de nanossegundos, não os 10ms do contador de ticks existente
  (`kernel::timer::ticks()`, inalterado). Ainda sem *syscall* expondo isso a modo
  usuário, sem ajuste de relógio em tempo de execução e sem fuso horário (sempre
  UTC) — ficam para sub-changes futuras do item 8. Verificado em boot real via
  `kernel::clock_demo`: a data/hora reportada bate com a hora real do host, e o
  relógio monotônico sempre avança entre duas leituras, em múltiplas execuções.

## 2. Arquitetura de Memória

- **Paginação**: 4-level paging (x86_64 padrão, 48-bit virtual address space), páginas
  de 4 KiB. O kernel implementa seu próprio gerenciamento de tabelas de página
  (`kernel::paging`, change `kernel-memory-management`) — caminhada via `CR3` +
  HHDM, criação de tabelas intermediárias sob demanda, e `map`/`unmap` de páginas
  arbitrárias com controle de permissões — escrito à mão (sem a crate `x86_64`) para
  manter consistência com o driver serial. Huge pages (2 MiB/1 GiB) permanecem futuras.
- **Layout de endereço virtual**: higher-half kernel — o kernel reside na metade
  superior do espaço de endereçamento, deixando a metade inferior livre para
  processos de usuário. O binário do kernel em si é linkado no topo dos 2 GiB
  superiores (`0xffffffff80000000`), exigido pelo `code-model=kernel`. A HHDM
  (higher-half direct map) fornecida pelo Limine ocupa o índice 256 da PML4; o heap
  do kernel está reservado a partir de `0xffff_9000_0000_0000` (índice 288); a stack
  dedicada do kernel (thread de boot) está reservada a partir de
  `0xffff_a000_0000_0000` (índice 320), com a guard page logo abaixo (ver
  `kernel-stack-guard-page`); o pool de stacks de kernel de cada thread nova está
  reservado a partir de `0xffff_b000_0000_0000` (índice 352, `kernel::stack::THREAD_STACK_POOL_BASE`,
  ver `kernel-process-scheduler`); a janela de memória virtual anônima de
  `kernel::vmm` está reservada a partir de `0xffff_c000_0000_0000` (índice 384 —
  **corrigido pela change `kernel-safepoint-signal`**: originalmente também usava o
  índice 352, colidindo silenciosamente com o pool de stacks de thread acima; ver
  `design.md` dessa change para o histórico completo).
- **Alocador físico**: `kernel::phys::FrameAllocator` — uma lista livre intrusiva (cada
  frame livre guarda, nos seus próprios bytes via HHDM, o endereço do próximo frame
  livre) inicializada a partir das regiões `usable` do memory map do Limine. Detecta
  explicitamente esgotamento de memória, endereços fora do intervalo gerenciado e
  double-free, sem precisar de um bitmap de bookkeeping separado.
- **Alocador de heap do kernel**: `kernel::heap`, usando a crate `linked_list_allocator`
  como `#[global_allocator]`, mapeado sobre uma faixa virtual fixa reservada (máximo de
  256 MiB) via `kernel::paging`. Cresce sob demanda (mapeando mais frames e chamando
  `extend()`) quando uma alocação não cabe no espaço já mapeado; falhas definitivas de
  alocação (memória física ou faixa reservada esgotadas) são tratadas por um
  `#[alloc_error_handler]` que reporta via serial e halta, em vez de corromper memória.
  Libera o uso de `alloc::{Vec, String, Box}` em qualquer parte do kernel.
- **Isolamento de memória entre processos** (futuro): cada processo de
  usuário/aplicação Java terá seu próprio espaço de endereçamento (CR3 próprio),
  construído sobre a mesma API de `kernel::paging` já implementada.
- **Memória virtual anônima POSIX** (`kernel-posix-virtual-memory`, `kernel::vmm`):
  segunda sub-change do item 8 do roadmap (porte da JVM), entrega a base de
  `mmap`/`munmap`/`mprotect` identificada como gap em `jvm-posix-compat-strategy` —
  reservar uma faixa de endereço virtual sem comprometer memória física (`reserve`),
  comprometer sob demanda com frames físicos zerados (`commit`), descomprometer
  devolvendo os frames ao alocador físico (`decommit`), alterar permissões de uma
  sub-região já comprometida sem desmapear (`protect`, sobre a nova primitiva
  `Mapper::protect` de `kernel-paging`) e liberar uma reserva inteira com
  reaproveitamento do espaço virtual por reservas futuras (`release`). Opera sobre
  uma janela fixa de 64 GiB a partir de `0xffff_c000_0000_0000` (índice 384 da PML4 —
  ver seção 1 para o endereço corrigido pela change `kernel-safepoint-signal`), só em
  modo kernel por enquanto —
  sem nenhuma *syscall* exposta a modo usuário e sem integração com
  `kernel::process::Process` (uma faixa de reserva por processo fica para uma
  sub-change futura). Verificado em boot real: o caminho feliz completo de
  `kernel::vmm_demo` (reservar, comprometer, escrever/ler um padrão conhecido,
  proteger, descomprometer, liberar, reservar de novo) roda sem gerar nenhuma falta
  de página; os quatro cenários de acesso inválido da spec (acesso não comprometido,
  escrita após proteção, acesso após descomprometimento, reaproveitamento do espaço
  liberado) foram confirmados individualmente com um gatilho temporário — ver
  `design.md` da change, "Achados durante a implementação".

## 3. Gerenciamento de Processos e Threads

- **Processos** (`kernel::process::Process`, change `kernel-process-scheduler`): cada
  processo possui seu próprio espaço de endereçamento (`kernel::paging::AddressSpace`,
  uma PML4 própria), com a metade superior (HHDM, heap, pool de stacks de thread,
  código do kernel — índices 256-511) compartilhada por referência com todos os outros
  processos, e a metade inferior inicialmente vazia (reservada para mapeamentos
  específicos de processo, ainda sem uso real até a change de modo usuário/syscalls).
  Um processo tem pelo menos uma thread.
- **Threads** (`kernel::process::Thread`): a unidade de escalonamento dentro de um
  processo. Cada thread tem sua própria stack de kernel (64 KiB, protegida por guard
  page — generalização de `kernel-stack-guard-page` para um pool de stacks, uma por
  thread) e seu próprio contexto de registradores da CPU.
- **Escalonador** (`kernel::scheduler`): preemptivo, round-robin, disparado pelo timer
  periódico do Local APIC já existente (`kernel-timer`) — a cada tick, o contexto da
  thread corrente é salvo e a próxima thread pronta é retomada, sem exigir cooperação.
  A troca de contexto é feita por uma rotina em assembly puro no vetor do timer (não
  pelo mecanismo `extern "x86-interrupt"` do Rust, que não expõe os registradores de
  uso geral necessários para uma troca de contexto completa).
- **Limitação atual**: tudo roda em modo kernel (anel 0) — não há modo usuário, não há
  `syscall`/`sysret`, e threads/processos só podem ser criados a partir de funções Rust
  do próprio kernel (sem carregamento de executáveis). Isso fica para a change futura de
  System Calls (ver seção 4).
- **Bloqueio/despertar voluntário de thread** (`kernel-wait-queue`, `kernel::wait_queue`):
  primeira sub-change do item 8 do roadmap (porte da JVM) — base de `pthread_mutex`/
  `pthread_cond` (`PlatformEvent`/`Parker` do HotSpot). `WaitQueue::block_current()`
  permite que a thread corrente saia voluntariamente da rotação do escalonador (sem
  busy-wait) até ser explicitamente devolvida a ela por `wake_one`/`wake_all` — de
  outra thread ou de uma ISR, com segurança (`with_interrupts_disabled` em toda seção
  crítica, mesmo precedente de `kernel::keyboard::poll_event`). Implementado como um
  vetor de interrupção de software **dedicado** (distinto do vetor do timer), que
  reentra na mesma lógica de decisão de escalonamento (`ReadyQueue`
  pop/decide-destino/next, extraída para `kernel::scheduler::schedule_next` e
  compartilhada com o tick real do timer) sem tocar na contagem de ticks/EOI do Local
  APIC, que só fazem sentido para uma interrupção real de hardware. Ainda sem API
  nomeada de `pthread_mutex`/`pthread_cond`, sem *syscall* exposta a modo usuário e
  sem suporte a *timeout* — ficam para sub-changes futuras do item 8. Verificado em
  boot real via `kernel::wait_demo` (produtora/consumidora, reportando cada despertar
  pelo console serial). Encontrou (e, em manutenção posterior, corrigiu) um bug
  pré-existente não relacionado em `kernel::serial`/`kernel::framebuffer` — ver
  `KNOW_ISSUES.md`, "Duas threads de kernel imprimindo concorrentemente travam o
  timer de hardware para sempre" (RESOLVIDO): `_print`/`MirroredWriter` agora são
  atômicos em relação a interrupções.
- **`pthread_mutex`/`pthread_cond` nomeados** (`kernel-pthread-sync`, `kernel::pthread`):
  segunda sub-change do item 8 do roadmap, construída sobre `kernel::wait_queue`.
  `Mutex` (exclusão mútua não-reentrante, bloqueio real sem busy-wait — `lock`/`unlock`)
  e `CondVar` (variável de condição estilo POSIX/Java — `wait(&Mutex)`/`notify_one`/
  `notify_all`, libera e reaquire o mutex associado atomicamente ao redor da espera).
  `WaitQueue` ganhou uma primitiva de bloqueio condicional (`block_current_if`, reavalia
  um predicado e registra a intenção de bloqueio como uma única operação atômica em
  relação a interrupções) para que `Mutex::lock` nunca perca um `unlock` concorrente —
  a garantia de `CondVar` contra notificação perdida depende da convenção de notificar
  com o mutex associado detido, mesmo contrato de `pthread_cond_signal`/`Object.notify`.
  Ainda sem *syscall* expondo isso a modo usuário, sem suporte a *timeout* e sem mutex
  reentrante — ficam para sub-changes futuras do item 8. Verificado em boot real via
  `kernel::pthread_demo`: um contador disputado por 4 threads sob o mesmo `Mutex`
  fecha exatamente com a soma esperada (nenhum incremento perdido) e um par
  produtor/consumidor com `Mutex`+`CondVar` entrega o item corretamente, em múltiplas
  execuções independentes.
- "Processo" no Java OS pode, no futuro, significar tanto um processo nativo do kernel
  quanto uma aplicação Java rodando dentro de uma JVM isolada — ainda não implementado
  (depende do porte da JVM, roadmap itens 7-9).

## 4. System Calls

- **Modo usuário** (`kernel-user-mode`, change `kernel-syscalls`): o kernel pode
  executar uma thread em anel 3, com código e stack próprios mapeados como
  acessíveis por usuário (`PageFlags.user_accessible`), sem acesso a memória
  exclusiva do kernel. Novos segmentos de GDT (`USER_CODE_SELECTOR`/
  `USER_DATA_SELECTOR`) na ordem específica exigida por `syscall`/`sysret`
  (dados de usuário antes de código de usuário).
- **Interface de chamada de sistema** (`kernel-syscalls`): `syscall`/`sysret`
  (x86_64, por desempenho, via MSRs `STAR`/`LSTAR`/`SFMASK` e `EFER.SCE`). A
  rotina de entrada troca para a stack de kernel da thread corrente antes de
  tocar qualquer dado vindo do modo usuário — o escalonador mantém essa stack
  (`TSS.rsp0` e uma variável global, já que o kernel é single-core) sempre
  coerente com a thread corrente, a cada troca de contexto.
- **ABI definida nesta primeira implementação**: número da chamada em `RAX`,
  até quatro argumentos em `RDI`/`RSI`/`RDX`/`R10`, retorno em `RAX`; um número
  de chamada desconhecido é rejeitado explicitamente.
- **Chamadas implementadas** (ver design.md das changes `kernel-syscalls` e
  `kernel-jvm-posix-syscalls`, que estendeu a tabela abaixo expondo a modo
  usuário as primitivas POSIX já construídas em modo kernel — `kernel::vmm`,
  `kernel::pthread`, `kernel::clock`):

  | Número | Nome | Argumentos | Retorno |
  |---|---|---|---|
  | 0 | `SYSCALL_WRITE` | `ptr`, `len` | bytes escritos |
  | 1 | `SYSCALL_GET_TICKS` | — | contador de ticks |
  | 2 | `SYSCALL_VM_RESERVE` | `size` | endereço base, ou erro |
  | 3 | `SYSCALL_VM_COMMIT` | `addr`, `len`, `prot` | `0`, ou erro |
  | 4 | `SYSCALL_VM_DECOMMIT` | `addr`, `len` | `0`, ou erro |
  | 5 | `SYSCALL_VM_PROTECT` | `addr`, `len`, `prot` | `0`, ou erro |
  | 6 | `SYSCALL_VM_RELEASE` | `addr` | `0`, ou erro |
  | 7 | `SYSCALL_MUTEX_CREATE` | — | handle, ou erro |
  | 8 | `SYSCALL_MUTEX_LOCK` | `handle` | `0`, ou erro |
  | 9 | `SYSCALL_MUTEX_UNLOCK` | `handle` | `0`, ou erro |
  | 10 | `SYSCALL_COND_CREATE` | — | handle, ou erro |
  | 11 | `SYSCALL_COND_WAIT` | `cond_handle`, `mutex_handle` | `0`, ou erro |
  | 12 | `SYSCALL_COND_NOTIFY_ONE` | `handle` | `0`, ou erro |
  | 13 | `SYSCALL_COND_NOTIFY_ALL` | `handle` | `0`, ou erro |
  | 14 | `SYSCALL_CLOCK_WALL` | — | nanossegundos desde a época |
  | 15 | `SYSCALL_CLOCK_MONOTONIC` | — | nanossegundos monotônicos |
  | 16 | `SYSCALL_DLOPEN` | `path_ptr`, `path_len` | `handle`, ou erro |
  | 17 | `SYSCALL_DLSYM` | `handle`, `name_ptr`, `name_len` | endereço, ou erro |
  | 18 | `SYSCALL_OPEN` | `path_ptr`, `path_len` | `handle`, ou erro |
  | 19 | `SYSCALL_READ` | `handle`, `buf_ptr`, `buf_len` | bytes lidos, ou erro |
  | 20 | `SYSCALL_CLOSE` | `handle` | `0`, ou erro |

  **Arquivos abertos** (change `kernel-file-syscalls`): `SYSCALL_OPEN`/
  `SYSCALL_READ`/`SYSCALL_CLOSE` expõem `kernel::fat32` a modo usuário —
  gap deixado em aberto pela change `kernel-fat32-filesystem` e pré-requisito
  para o HotSpot carregar classes do class path. `kernel::file_handles`
  (mesmo padrão de tabela dinâmica de `kernel::pthread_handles`, índice do
  `Vec` como `handle`) registra, na abertura, o conteúdo completo do arquivo
  já lido (sem leitura incremental por cluster) junto com um cursor de
  posição; cada leitura copia a partir do cursor e o avança; o fechamento
  libera o `Vec<u8>` do conteúdo imediatamente — diferente de
  `pthread_handles`, cujos objetos nunca são destruídos (não há API de
  bloqueio exigindo referência `'static` aqui). Mesma limitação das demais
  tabelas de handle: global ao kernel, sem isolamento entre processos.

  As chamadas de memória virtual (2-6) endereçam por ponteiro (convenção
  `mmap`/`munmap`/`mprotect`: `prot` é uma máscara de 2 bits, bit 0 =
  gravável, bit 1 = executável, com `user_accessible` sempre forçado,
  independentemente do valor recebido). As de mutex/condvar (7-13) criam
  objetos nomeados dinamicamente (`kernel::pthread_handles`, via `Box::leak`,
  sem destruição ainda) identificados por um `handle` opaco. Todo erro
  alcançável por estas chamadas colapsa no mesmo sentinela `SYSCALL_ERROR` já
  usado por "número de chamada desconhecido" — nenhum esquema de erro
  granular (estilo `errno`) existe ainda.
- **Carregador de executáveis**: `kernel::elf` (change `kernel-elf-loader`,
  ver spec `kernel-elf-loader`) interpreta um executável ELF64 estático
  (`ET_EXEC`, x86-64, sem relocations/PIE) lido do sistema de arquivos
  (`kernel::fat32`), mapeia cada segmento `PT_LOAD` com as permissões
  corretas (zerando explicitamente o BSS — achado da change: `Mapper::map_new`
  não zera o frame que aloca) e transfere controle para o ponto de entrada
  declarado, via `kernel::scheduler::spawn_from_elf`.
  **Linker dinâmico**: `kernel::dylib` (change `kernel-dynamic-linker`, ver
  spec `kernel-dynamic-linker`) carrega bibliotecas compartilhadas ELF64
  (`ET_DYN`) dentro de um processo já em execução — reaproveita o parsing de
  `kernel::elf` (que passou a aceitar `ET_DYN` além de `ET_EXEC`), carrega
  dependências `DT_NEEDED` recursivamente (resolvidas em `/lib/<nome>`),
  aplica relocações x86-64 (`R_X86_64_RELATIVE`/`GLOB_DAT`/`JUMP_SLOT`/`64`)
  de forma eager, e expõe `dlopen`/`dlsym` como *syscalls*. Estado de
  bibliotecas carregadas é isolado por espaço de endereçamento, chaveado
  pelo CR3 físico ativo no momento da *syscall*
  (`kernel::paging::current_root()`) em vez de por um novo rastreamento no
  escalonador. Ainda sem *lazy binding*, *symbol versioning*, TLS dinâmico,
  `dlclose`, nem resolução de símbolo contra o executável principal — fica
  para quando/se necessário durante o porte do HotSpot (próxima sub-change
  concreta do roadmap, item 8). Os programas de teste gerados em compilação
  e copiados para uma página de
  usuário em runtime (`kernel::user_demo`, `kernel::user_posix_demo`)
  continuam existindo, usados para provar o mecanismo de `syscall`/`sysret`
  em si, independente do carregador.
- **Limitação atual**: uma exceção causada por código de modo usuário ainda
  halta o kernel inteiro (sem isolamento de falhas por processo/thread
  ainda), e as tabelas de handle de mutex/condvar e a janela de
  `kernel::vmm` continuam globais ao kernel inteiro, sem isolamento entre
  processos diferentes.
- Separação esperada entre syscalls "de sistema" e a superfície exposta à JVM
  fica para quando a JVM existir (roadmap itens 7-9).

## 5. HAL e Drivers

- **HAL em `kernel::hal`**: `trait Console: core::fmt::Write`
  (`kernel-framebuffer-console`), sem métodos próprios além de
  `core::fmt::Write` — suficiente para os dois consoles existentes;
  `trait KeyboardDevice` (`kernel-keyboard`), com um único método
  não-bloqueante (`poll_event`), implementado por `kernel::keyboard::Ps2Keyboard`;
  `trait BlockDevice` (`kernel-storage`), com `block_size`/`read_block`/
  `write_block` por LBA, implementado por `kernel::storage::AhciDisk`; e
  `trait NetworkDevice` (`kernel-network`), com `mac_address`/`send_frame`/
  `poll_frame`, implementado por `kernel::network::E1000Nic`. Com isso, os
  quatro traits previstos desde `kernel-framebuffer-console` (saída,
  teclado, armazenamento, rede) estão todos implementados.
- **Console de framebuffer** (`kernel-framebuffer-console`, `kernel::framebuffer`):
  segundo canal de saída de texto, sobre o framebuffer linear fornecido pelo
  Limine (`FramebufferRequest`). Conversão de pixel genérica a partir das
  máscaras RGB reportadas (`red/green/blue_mask_size`/`_shift`), fonte bitmap
  8×8 própria (ASCII 32-126, com glifo de reserva fora desse intervalo),
  rolagem por cópia de linhas de pixels. Inicializado bem cedo em
  `kernel_main` (antes até de `HHDM_REQUEST` — `Framebuffer::address()` já
  vem diretamente utilizável, confirmado em boot real), com tratamento
  explícito de indisponibilidade (nenhuma resposta, nenhum framebuffer, ou
  `memory_model`/`bpp` não suportados — só `FRAMEBUFFER_RGB` com 16/24/32
  bits por pixel).
- **Espelhamento automático**: `kernel::serial::_print` (usada por
  `serial_println!`/`serial_print!`) passa a também escrever no console de
  framebuffer, se inicializado — reflete automaticamente todo o código de
  diagnóstico que já usava essas macros. Código que escreve direto num
  `core::fmt::Write` concreto em vez de usar essas macros (ex.:
  `memmap_report::write_memory_map`) precisa do adaptador
  `kernel::serial::MirroredWriter`; código que escreve bytes crus (ex.:
  `syscall::sys_write`) chama `kernel::framebuffer::mirror_str` diretamente
  quando os bytes são UTF-8 válido. Sem console serial ou de framebuffer
  "soltos" sem espelhamento — qualquer novo call site de diagnóstico direto
  na serial deve usar um desses dois mecanismos.
- **I/O APIC mínimo** (`kernel-keyboard`, `kernel::ioapic`): primeiro
  roteamento de uma IRQ de hardware externa — endereço MMIO fixo
  (`0xFEC00000`, padrão de plataforma PC-compatível, sem parsing de
  ACPI/MADT), mapeado via `kernel::paging::Mapper` (a HHDM do Limine não
  cobre esse endereço). Pré-requisito descoberto durante a implementação:
  o PIC 8259 legado estava completamente mascarado e não havia nenhum
  caminho para uma IRQ externa chegar à CPU antes desta change; agora o
  IMCR (portas 0x22/0x23) também é comutado para modo APIC em
  `kernel::pic::disable()`.
- **Teclado PS/2** (`kernel-keyboard`, `kernel::keyboard`): driver do
  controlador 8042 (primeiro canal) e tradução de scancodes set 1 (com
  estado de Shift) para eventos de tecla, expostos via a nova abstração de
  HAL `kernel::hal::KeyboardDevice`. Fila de eventos circular de capacidade
  fixa, consumida por uma thread de demonstração
  (`kernel::keyboard_demo`) que ecoa teclas recebidas nos consoles
  existentes. Verificado via `sendkey`/QMP do QEMU com aceleração de
  hardware (`-accel kvm`) — sob emulação por software pura (TCG) a 100 Hz
  (frequência padrão do escalonador), a entrega de IRQ externa de baixa
  frequência compete mal com o timer e só a primeira tecla simulada chega
  de forma confiável (limitação de timing do ambiente de teste, não do
  kernel — ver `KNOW_ISSUES.md`).
- **PCI** (`kernel-storage`, `kernel::pci`): acesso à configuração PCI via o
  mecanismo legado de portas 0xCF8/0xCFC e enumeração do barramento 0 —
  suficiente para localizar o controlador AHCI do chipset do QEMU/Q35, e
  reutilizável por qualquer dispositivo PCI futuro (rede).
- **Armazenamento AHCI** (`kernel-storage`, `kernel::storage`): driver de
  bloco bruto (sem sistema de arquivos) sobre um controlador SATA/AHCI —
  localizado via `kernel::pci`, com `ABAR` (BAR5) mapeado explicitamente
  (mesmo tratamento do I/O APIC: a HHDM não garante cobri-lo). Inicializa
  uma porta, emite comandos `READ`/`WRITE DMA EXT` no slot 0 por *polling*
  (sem IRQ — decisão explícita, ver `design.md` da change). Verificado em
  boot real: leitura do setor de boot (assinatura `0x55AA`) e uma escrita
  seguida de leitura num LBA de teste, ambos corretos já na primeira
  tentativa — ao contrário do teclado, este driver não depende de nenhuma
  IRQ, então não sofre da limitação de timing do QEMU/TCG documentada em
  `KNOW_ISSUES.md`.
- **Sistema de arquivos FAT32** (`kernel-fat32-filesystem`, `kernel::gpt`/
  `kernel::fat32`): localiza a partição "EFI System Partition" lendo a
  tabela de partição GPT do disco (`kernel::gpt::find_efi_system_partition`,
  genérica sobre `hal::BlockDevice` — sem hardcodar o LBA de início que o
  `xtask` usa para montar a imagem), monta essa partição como FAT32
  (`kernel::fat32::Fat32Filesystem`, validando o setor de inicialização) e lê
  o conteúdo completo de um arquivo dado seu caminho absoluto, resolvendo
  diretórios e seguindo a cadeia de clusters via a FAT — somente leitura,
  somente nomes curtos 8.3 (sem VFAT/nomes longos). Toda a lógica, incluindo
  montagem e leitura de arquivo, é genérica sobre `hal::BlockDevice` e
  testável em host sem hardware (ao contrário de `kernel::vmm`/
  `kernel::syscall`, que precisam de `Mapper`/`CR3` reais para seus
  caminhos de sucesso). Verificado em boot real: lê `/EFI/BOOT/BOOTX64.EFI`
  da própria partição de boot (319.488 bytes, múltiplos clusters) e confirma
  sua assinatura `MZ`. **Exposição via *syscall* a modo usuário entregue
  pela change `kernel-file-syscalls`**: `SYSCALL_OPEN`/`SYSCALL_READ`/
  `SYSCALL_CLOSE` — ver seção 4.
- **Rede** (`kernel-network`, `kernel::network`): driver de placa Ethernet
  Intel 82540EM (`e1000`) — localizado via `kernel::pci`, com Bus Mastering
  habilitado explicitamente no registrador de comando PCI (`kernel::pci`
  ganhou escrita de configuração nesta change — até então só lia) e `BAR0`
  mapeado explicitamente (mesmo tratamento do I/O APIC/`ABAR`). Anéis de
  descritores de recepção/transmissão (8 cada), envio/recepção de quadros
  Ethernet brutos por *polling* (sem IRQ, mesma decisão de `kernel-storage`
  — nenhum dos dois sofre da limitação de timing do QEMU/TCG documentada em
  `KNOW_ISSUES.md`, exclusiva do teclado). O modelo da placa é fixado
  explicitamente via uma nova flag em `xtask` (`-device e1000`), em vez de
  depender do modelo que o QEMU expõe por padrão (uma Intel 82574L/e1000e,
  descoberta durante a pesquisa desta change). Verificado em boot real:
  endereço MAC lido corretamente, quadro de teste aceito pela placa, e
  confirmado via captura de pacotes (`-object filter-dump` do QEMU) que os
  bytes exatos do quadro saíram pela rede emulada.

Com `kernel-network`, o item 6 do roadmap (HAL e drivers essenciais) está
**completo**: console serial, framebuffer, timer/APIC, I/O APIC, teclado,
armazenamento e rede — todos implementados.

## 6. Estratégia de Portabilidade do OpenJDK (decidida — maior risco técnico do projeto)

- Portar o HotSpot completo para um kernel próprio e não-POSIX é a tarefa de maior
  risco do projeto, pois o HotSpot assume um ambiente hospedeiro rico (libc, pthreads,
  mmap/mprotect POSIX, sinais Unix, `/proc`, etc.).
- **Decisão (`jvm-posix-compat-strategy`)**: camada de compatibilidade POSIX mínima
  implementada no kernel, para que o HotSpot (ou uma JVM derivada) rode sobre ela com
  o mínimo de modificação possível no seu próprio código-fonte — idealmente tratando o
  Java OS como "mais um `os_<plataforma>`" dentro da estrutura já existente de
  `src/hotspot/os/<plataforma>` do HotSpot, análogo a `os_linux`/`os_bsd`.
  Alternativas descartadas: porte direto das camadas de abstração de SO do próprio
  HotSpot para as primitivas nativas do kernel sem POSIX intermediário (acoplamento
  profundo demais com código interno do HotSpot, que muda entre versões); e JVM
  alternativa minimalista para bootstrap (adiaria, não eliminaria, o problema).
- **Inventário de primitivas POSIX mínimas exigidas pelo HotSpot** (pesquisado em
  `jvm-posix-compat-strategy/design.md`, com fontes), cruzado com o que o kernel já
  tem:
  - *Memória virtual* (`mmap`/`munmap`/`mprotect`, para heap Java, *code cache* do JIT
    e páginas de guarda): o kernel só tem `kernel::paging::Mapper::map`/`map_new`
    (página já resolvida, sem conceito de "reservado mas não committado", nem de
    alterar permissões de uma página já mapeada).
  - *Threads nativas e sincronização* (`pthread_create`, `pthread_mutex`/`pthread_cond`
    — base de `park`/`unpark`, `Object.wait`/`notify`, `java.util.concurrent`): o
    escalonador cria threads kernel, mas não há API pensada para uma VM hóspede, e
    **não existe nenhuma primitiva de bloqueio/sono de thread no kernel hoje** (só
    *busy-wait* via `kernel::sync::Spinlock`) — o maior gap estrutural identificado.
  - *Sinal de safepoint* (`SIGSEGV` numa página de polling protegida, para parar todas
    as threads Java simultaneamente sem polling explícito em cada ponto de segurança):
    **entregue pela change `kernel-safepoint-signal`** — `kernel::safepoint::register`
    permite marcar uma faixa de endereço como ponto de poll; uma falta de página por
    ausência (`P=0`) dentro dela retoma a execução automaticamente (`iretq` sem ação
    adicional) em vez do `page_fault_handler` haltar o kernel, combinado com
    `SYSCALL_VM_DECOMMIT`/`SYSCALL_VM_COMMIT` já existentes para armar/liberar a
    página — sem desvio genérico para um handler escolhido pelo processo (decisão
    explícita: ver design.md da change, "Por que não bloquear a thread").
  - *Relógio* (`clock_gettime`, para `System.nanoTime()`/`currentTimeMillis()`):
    `kernel::timer` já conta ticks desde o boot; falta relógio de parede real e,
    possivelmente, resolução mais fina.
  - *E/S de arquivo* (`open`/`read`, para carregar classes do *class path*):
    **entregue pelas changes `kernel-fat32-filesystem` e `kernel-file-syscalls`**
    — `kernel::fat32::Fat32Filesystem` monta a partição de boot (localizada via
    `kernel::gpt`) e lê um arquivo inteiro por caminho absoluto;
    `SYSCALL_OPEN`/`SYSCALL_READ`/`SYSCALL_CLOSE` expõem isso a modo usuário
    com um handle de arquivo aberto e cursor de leitura sequencial
    (`kernel::file_handles`) — ver seção 4.
- **Carregamento dinâmico de bibliotecas — decidido (`jvm-dynamic-linking-strategy`)**:
  o HotSpot "de fábrica" depende de **carregamento dinâmico de bibliotecas
  compartilhadas** (`libjvm.so` carregando `libjava.so` via `dlopen`/linker dinâmico, e
  JNI carregando bibliotecas nativas de aplicações) — um requisito de peso comparável
  aos acima. **Decisão**: build totalmente estático do JDK/Leyden ("Hermetic Java") é
  **rejeitado** nesta etapa — não apenas por ser experimental, mas porque o próprio
  *workaround* que o Leyden usa para a limitação de `dlopen()` a partir de memória da
  glibc depende de `dlopen("/proc/self/fd/" + fd)`, uma API exclusiva do Linux
  (`/proc`) que o Java OS não tem nem planeja ter. Rota adotada: implementar um
  **linker ELF dinâmico mínimo no próprio kernel**, inteiramente sob controle do
  projeto. **Achado**: independentemente dessa escolha, o kernel na época não tinha
  nenhum carregador de executável ELF, nem para um binário estaticamente linkado —
  todo código de modo usuário então (`kernel::user_demo`/`kernel::user_posix_demo`) era
  um blob de assembly copiado manualmente para uma página. Carregar um executável ELF
  estático e fazer o linker dinâmico de `.so` (símbolos, relocações, `dlopen`/`dlsym`)
  são problemas distintos — o primeiro é pré-requisito do segundo. **Entregue pela
  change `kernel-elf-loader`**: `kernel::elf` interpreta um ELF64 estático (`ET_EXEC`)
  lido via `kernel::fat32`, mapeia seus segmentos `PT_LOAD` com as permissões corretas
  (BSS zerado explicitamente) e transfere controle para o ponto de entrada, via
  `kernel::scheduler::spawn_from_elf` — ver seção 4. **Entregue pela change
  `kernel-dynamic-linker`**: o linker dinâmico de `.so` propriamente dito —
  `kernel::dylib` carrega bibliotecas compartilhadas `ET_DYN` dentro de um
  processo já em execução, resolve dependências `DT_NEEDED` recursivamente e
  relocações x86-64 (`RELATIVE`/`GLOB_DAT`/`JUMP_SLOT`/`64`), e expõe
  `dlopen`/`dlsym` como *syscalls* — ver seção 4. Sem *lazy binding*,
  *versioning*, TLS dinâmico nem `dlclose` ainda (mínimo suficiente para
  provar o mecanismo, não uma `ld.so` completa).
- Esta change não implementa nenhuma primitiva nova — isso fica inteiramente para a
  change do item 8 do roadmap ("Porte inicial da JVM"), que decide a forma concreta de
  cada primitiva do inventário acima (ex.: syscalls novas vs. extensão das existentes)
  antes de integrar um HotSpot de fato.

## 7. Arquitetura da JVM (futuro)

- Modelo híbrido: (a) runtime compartilhado para serviços de sistema confiáveis;
  (b) JVM isolada por aplicação de terceiros.
- A JVM isolada depende dos primitivos de processo/memória (seções 2-3).
- GC/JIT do HotSpot assumem controle fino sobre proteção de páginas e sinais.

## 8. Modelo de Execução das Aplicações (futuro)

- Aplicações Java tradicionais (JAR) rodam na JVM isolada por aplicação, com
  verificação de permissões mediada pelo Java OS Platform.
- Aplicações "nativas" do Java OS podem rodar no runtime compartilhado.

## 9. APIs Java Próprias (futuro)

- APIs específicas do Java OS para: gerenciamento de aplicações, sistema de arquivos,
  dispositivos, rede/comunicação, processos Java, atualizações/configurações, e as
  APIs da Gaming Platform (launcher, ROM manager, emulator manager, save manager,
  controller manager).

## 10. Arquitetura Gráfica (futuro)

- Caminho inicial de saída gráfica: framebuffer linear fornecido pelo Limine.
- GUI completa é responsabilidade de uma camada Java acima do kernel.

## 11. Arquitetura de Emulação (futuro)

- Gaming Platform prevê múltiplos emuladores "próprios" em Java, cada um como módulo
  plugável gerenciado pelo Emulator Manager.

## 12. Estrutura de Repositórios

Monorepo único, organizado como workspace Cargo na raiz:

```
java-os/
├── Cargo.toml                  # workspace raiz
├── rust-toolchain.toml         # pino do toolchain Rust (nightly)
├── x86_64-java_os.json         # target spec customizado
├── .cargo/config.toml          # flags unstable (json-target-spec) e aliases (xtask, kernel-build)
├── kernel/                     # crate do kernel (no_std)
│   ├── Cargo.toml
│   ├── build.rs                # aplica o linker.ld apenas ao binário
│   ├── linker.ld
│   └── src/
│       ├── lib.rs              # módulos testáveis no host (serial, fmtbuf, memmap_report,
│       │                       #   hhdm, phys, paging, heap, sync, gdt, idt,
│       │                       #   apic, pic, port, timer)
│       └── main.rs             # binário bare-metal (#![no_std]#![no_main])
├── xtask/                      # automação de build/run/test (cargo-xtask)
├── boot/
│   ├── limine.conf             # configuração do Limine (protocolo UEFI)
│   ├── limine/                 # Limine vendorizado (não versionado, ver .gitignore)
│   └── dist/                   # imagem de disco gerada (não versionada)
├── docs/
│   └── architecture/           # este documento
├── .github/workflows/          # CI
└── openspec/                   # planejamento (OpenSpec)
```

Componentes futuros (runtime Java/OpenJDK port, Java OS Platform, Gaming Platform)
entram como diretórios/subprojetos irmãos de `kernel/` quando forem propostos.

## 13. Pipeline de Compilação

- **Kernel**: `cargo kernel-build` (alias para `cargo build -p kernel --target
  x86_64-java_os.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem`).
- **Imagem de boot**: `cargo xtask build-image` monta uma imagem de disco GPT/FAT
  (partição EFI System) com `sgdisk` + `mtools` (`mformat`/`mmd`/`mcopy`), copiando
  `BOOTX64.EFI` do Limine, o binário do kernel e `limine.conf`.
- **Execução**: `cargo xtask run -- <args extras do qemu>` levanta QEMU com
  `-bios <OVMF>` apontando para a imagem gerada.
- **Teste**: `cargo xtask test` roda o smoke test de boot headless, com timeout e
  captura de serial.
- **CI (GitHub Actions)**: `.github/workflows/kernel-ci.yml` — build do kernel,
  testes unitários do crate, e smoke test de boot em `ubuntu-latest`.

## 14. Estratégia de Testes

- **Testes de caixa-preta**: smoke test de boot via QEMU + captura de serial
  (`cargo xtask test`).
- **Testes unitários no kernel `no_std`**: módulos puros (sem I/O de hardware), como
  `memmap_report`, são compilados com `#![cfg_attr(not(test), no_std)]` e testados
  normalmente no host via `cargo test -p kernel --lib`.
- **Testes futuros** (memória, processos, syscalls): devem reusar a mesma
  infraestrutura de `xtask`/QEMU, estendendo os smoke tests com novos cenários por
  capability.

## 15. Roadmap Completo

1. **`kernel-boot-foundation`** (concluída): arquitetura completa documentada; boot
   UEFI/Limine funcional no QEMU; relato de boot e mapa de memória; smoke test
   automatizado + CI.
2. **`kernel-memory-management`** (concluída): alocador de frames físicos, paginação
   própria (mapear/desmapear sob demanda), heap do kernel crescendo sob demanda.
3. **`kernel-interrupts-timer`** (concluída): IDT/GDT/TSS próprios, handlers de
   exceção de CPU com double fault via IST, Local APIC (x2APIC) substituindo o PIC
   8259, timer periódico com contador de ticks.
4. **`kernel-process-scheduler`** (concluída): processos com espaço de endereçamento
   próprio, threads com stack protegida por guard page e contexto de registradores,
   escalonador preemptivo round-robin disparado pelo timer — tudo ainda em modo kernel
   (anel 0), sem modo usuário/syscalls.
5. **`kernel-syscalls`** (concluída): modo usuário (anel 3), interface
   `syscall`/`sysret` (STAR/LSTAR/SFMASK, EFER.SCE), ABI mínima (número +
   4 argumentos + retorno), duas primeiras chamadas (escrita no console,
   consulta de ticks) — ainda sem carregador de executáveis nem isolamento de
   falhas por processo.
6. **HAL e drivers essenciais** (concluído): `kernel-framebuffer-console`
   entregou a primeira abstração de HAL (`trait Console`) e o console de
   framebuffer, com espelhamento automático do console serial existente.
   `kernel-keyboard` entregou o primeiro I/O APIC (roteamento de IRQ
   externa, pré-requisito para qualquer dispositivo externo futuro), o
   driver de teclado PS/2 e `trait KeyboardDevice`. `kernel-storage`
   entregou acesso a configuração PCI (`kernel::pci`), o driver de bloco
   bruto AHCI e `trait BlockDevice`. `kernel-network` entregou escrita de
   configuração PCI, o driver de placa Ethernet e1000 e `trait
   NetworkDevice`, fechando este item do roadmap.
7. **`jvm-posix-compat-strategy`** (concluída): decisão formal da estratégia de porte
   do OpenJDK/HotSpot — camada de compatibilidade POSIX mínima no kernel (ver
   seção 6) — e inventário das primitivas que faltam, sem nenhuma implementação
   ainda.
8. **Porte inicial da JVM** (em andamento): primeira JVM minimamente funcional
   rodando um "hello world" Java sobre o Java OS, dividido em sub-changes
   sequenciais (mesmo padrão já usado para o item 6). `kernel-wait-queue`
   (concluída) entregou a primeira sub-change: a primitiva de bloqueio/despertar
   voluntário de thread (`kernel::wait_queue` — `block_current`/`wake_one`/
   `wake_all`, base de `pthread_mutex`/`pthread_cond`, ver seção 3).
   `kernel-posix-virtual-memory` (concluída) entregou a segunda sub-change: a
   camada de memória virtual anônima POSIX (`kernel::vmm` —
   `reserve`/`commit`/`decommit`/`protect`/`release`, base de `mmap`/`munmap`/
   `mprotect`) e a primitiva `Mapper::protect` em `kernel-paging` (ver seção 2).
   `kernel-pthread-sync` (concluída) entregou a terceira sub-change: `pthread_mutex`/
   `pthread_cond` nomeados (`kernel::pthread::{Mutex, CondVar}`, ver seção 3),
   construídos sobre `kernel::wait_queue`. `kernel-wall-clock` (concluída) entregou
   a quarta sub-change: relógio de parede e monotônico (`kernel::clock` —
   `wall_clock_nanos`/`monotonic_nanos`, base de `clock_gettime`, ver seção 1).
   `kernel-jvm-posix-syscalls` (concluída) entregou a quinta sub-change: as
   *syscalls* expondo todas as primitivas acima a modo usuário (ver seção 4
   — `SYSCALL_VM_*`, `SYSCALL_MUTEX_*`, `SYSCALL_COND_*`, `SYSCALL_CLOCK_*`),
   com tabelas de handle dinâmicas para mutex/condvar nomeados
   (`kernel::pthread_handles`) e tradução endereço→`Region`
   (`Vmm::region_containing`) para as chamadas de memória virtual.
   `kernel-safepoint-signal` (concluída) entregou a sexta sub-change: o sinal de
   safepoint (`kernel::safepoint::register`, falta de página por ausência numa
   faixa registrada retoma a execução em vez de haltar — ver seção 6). Durante
   sua implementação foi descoberto e corrigido um bug pré-existente em duas
   sub-changes anteriores: `kernel::vmm::VMM_WINDOW_START` colidia com
   `kernel::stack::THREAD_STACK_POOL_BASE` (ambos no índice 352 da PML4) — ver
   seção 1 e o design.md da change para o histórico completo.
   `kernel-fat32-filesystem` (concluída) entregou a sétima sub-change: o
   sistema de arquivos (`kernel::gpt`/`kernel::fat32`, localizado via GPT,
   somente leitura, nomes curtos 8.3 — ver seção 5).
   `jvm-dynamic-linking-strategy` (concluída) fechou a decisão de
   carregamento dinâmico de bibliotecas, pendente desde
   `jvm-posix-compat-strategy`: build estático do JDK/Leyden rejeitado
   (dependência de `/proc/self/fd`, exclusiva do Linux); linker ELF dinâmico
   mínimo no kernel adotado — ver seção 6. `kernel-elf-loader` (concluída)
   entregou o pré-requisito identificado por essa decisão: o **carregador de
   executável ELF64 estático** (`kernel::elf`/`kernel::scheduler::spawn_from_elf`
   — ver seção 4), sem relocations/PIE nem bibliotecas dinâmicas.
   `kernel-dynamic-linker` (concluída) entregou o **linker ELF dinâmico
   mínimo** propriamente dito: `kernel::dylib` carrega bibliotecas
   compartilhadas `ET_DYN` dentro de um processo em execução (dependências
   `DT_NEEDED` recursivas, relocações `RELATIVE`/`GLOB_DAT`/`JUMP_SLOT`/`64`
   aplicadas de forma eager, estado isolado por espaço de endereçamento via
   `kernel::paging::current_root()`), exposto a modo usuário via `dlopen`/
   `dlsym` — ver seção 4/6. `kernel-file-syscalls` (concluída) entregou mais
   uma sub-change, a mais recente até o momento: `SYSCALL_OPEN`/`SYSCALL_READ`/
   `SYSCALL_CLOSE` expõem `kernel::fat32` a modo usuário através de um handle
   de arquivo aberto com cursor de leitura sequencial (`kernel::file_handles`,
   mesmo padrão de tabela dinâmica de `kernel::pthread_handles`, mas liberando
   a memória do conteúdo ao fechar) — ver seção 4/6. Único passo restante do
   item 8: o próprio porte do HotSpot sobre todas as primitivas já entregues.
9. **Modelo híbrido de execução de JVM**: runtime compartilhado vs. JVM isolada por
   aplicação.
10. **Java OS Platform (Java)**: gerenciamento de arquivos, serviços de sistema, rede,
    APIs próprias, interface gráfica inicial.
11. **Gaming Platform (Java)**: launcher, ROM/Emulator/Save/Controller managers,
    primeiros emuladores próprios.
12. **Hardening multi-hardware**: validação em hardware real além de
    QEMU/VMware/VirtualBox; SMP; drivers adicionais.
