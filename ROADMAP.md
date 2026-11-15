# Roadmap — Java OS

Este documento responde a duas perguntas de forma direta: **o que já funciona** e
**o que falta**, com destaque especial para os dois marcos que mais importam a médio
prazo — rodar uma JVM e ter uma interface gráfica. Para profundidade técnica, veja:

- `docs/architecture/overview.md` — arquitetura de referência completa (15 áreas) e o
  roadmap detalhado por sub-change (seção 15).
- `SPECIFICATION.md` — regras de negócio, casos de uso e modelo de domínio do sistema
  tal como ele existe hoje.
- `openspec/changes/archive/` — o histórico completo de planejamento (proposta, design,
  tarefas) de cada entrega, uma pasta por change.

Atualizado em 2026-11-07, após `kernel-file-syscalls`, `libc-minimal-port`,
`kernel-fpu-context-switch`, `cxx-minimal-runtime`, `os-javaos-skeleton`,
`os-javaos-memory-pd`, `os-javaos-platform-sync` e `libc-float-math`.

## O que já está implementado (verificado em boot real)

### Kernel e boot
- Boot UEFI via Limine, relatório de boot e mapa de memória, smoke test automatizado + CI
  (`kernel-boot-foundation`).
- IDT/GDT/TSS próprios, exceções de CPU com stacks dedicadas via IST (`#DF`, `#PF`),
  Local APIC em modo x2APIC substituindo o PIC 8259, timer periódico (`kernel-interrupts-timer`).
- Stack do kernel com guard page própria, generalizada depois para um pool de stacks
  (uma por thread) (`kernel-stack-guard-page`).

### Memória
- Alocador de frames físicos (lista livre intrusiva) e paginação própria (4 níveis,
  mapear/desmapear sob demanda) (`kernel-memory-management`).
- Heap do kernel crescendo sob demanda (`linked_list_allocator`).
- Memória virtual anônima estilo POSIX — `reserve`/`commit`/`decommit`/`protect`/`release`
  — expostas a modo usuário via syscall (`kernel-posix-virtual-memory`, `kernel-jvm-posix-syscalls`).

### Processos, threads e escalonador
- Processos com espaço de endereçamento próprio, threads com stack protegida por guard
  page, escalonador preemptivo round-robin disparado pelo timer (`kernel-process-scheduler`).
- Bloqueio/despertar voluntário de thread sem busy-wait (`kernel-wait-queue`) e
  `pthread_mutex`/`pthread_cond` nomeados sobre essa base (`kernel-pthread-sync`), ambos
  expostos a modo usuário via syscall.
- **Término de processo/thread** (`SYSCALL_EXIT`) — gap descoberto durante o port da libc
  C, já que nenhum programa de usuário tinha como terminar antes disso
  (`libc-minimal-port`).
- **Estado de FPU/SSE/XMM preservado na troca de contexto** (`FXSAVE`/`FXRSTOR` por
  thread, `CR0`/`CR4` configurados explicitamente no boot) — verificado com duas threads
  concorrentes somando `double` via SSE, preemptadas repetidamente, cada uma com
  resultado correto e isolado (`kernel-fpu-context-switch`).

### System calls
- Modo usuário real (anel 3), interface `syscall`/`sysret` (STAR/LSTAR/SFMASK,
  `EFER.SCE`), ABI mínima (número + 4 argumentos + retorno) (`kernel-syscalls`).
- Relógio de parede e monotônico (`CLOCK_REALTIME`/`CLOCK_MONOTONIC`) expostos a modo
  usuário (`kernel-wall-clock`).
- Sinal de safepoint (página de poll que gera `#PF` e retoma automaticamente, base do
  mecanismo de safepoint de uma JVM) (`kernel-safepoint-signal`).
- **E/S de arquivo via syscall** (`SYSCALL_OPEN`/`READ`/CLOSE`, handle com cursor de
  leitura sequencial) (`kernel-file-syscalls`).
- **Tentativa de aquisição não-bloqueante de mutex nomeado** (`SYSCALL_MUTEX_TRYLOCK`,
  expõe `kernel::pthread::Mutex::try_acquire`) (`os-javaos-platform-sync`).
- 23 chamadas de sistema no total hoje — todo erro ainda colapsa num único sentinela
  (`SYSCALL_ERROR`), sem esquema granular estilo `errno`.

### HAL e drivers (completo desde o item 6 do roadmap)
- Console serial + console de framebuffer (espelhados automaticamente) (`kernel-framebuffer-console`).
- Teclado PS/2 + I/O APIC (primeira IRQ externa roteada) (`kernel-keyboard`).
- Armazenamento AHCI, bloco bruto (`kernel-storage`).
- Rede Ethernet e1000, quadros brutos por polling (`kernel-network`).

### Sistema de arquivos
- FAT32 somente leitura, nomes curtos 8.3, montado via tabela de partição GPT
  (`kernel-fat32-filesystem`).

### Carregamento de executáveis e linker dinâmico
- Carregador de ELF64 estático (`ET_EXEC`), zera BSS explicitamente, transfere controle
  ao ponto de entrada (`kernel-elf-loader`).
- Linker dinâmico mínimo para bibliotecas `.so` (`ET_DYN`) — resolve `DT_NEEDED`
  recursivamente, aplica relocações x86-64, expõe `dlopen`/`dlsym` via syscall
  (`kernel-dynamic-linker`, decisão em `jvm-dynamic-linking-strategy`).

### Libc C mínima para programas de usuário
- **newlib retargetado** (sem `libc/sys` próprio — build genérica `x86_64-elf` com um
  arquivo de glue externo sobre as syscalls já existentes), cobrindo `stdio`/`string`/
  `reent`/`malloc` (via `sbrk`)/E-S de arquivo/término de processo (`libc-minimal-port`).
- Verificado em boot real com um programa C de verdade: aloca memória, escreve no
  console, lê um arquivo do disco e termina — tudo via a libc portada.

### `libm`/conversão float reais na libc C
- **Ponto flutuante real** (`float`/`double` por valor, `libm` inteira —
  `sin`/`cos`/`sqrt`/`pow`/etc. —, e conversão string↔float — `strtod`/`atof`/`dtoa`)
  — excluído deliberadamente por `libc-minimal-port` (SSE desabilitado no toolchain
  C/C++ até então), reconstruído agora que `kernel-fpu-context-switch` tornou SSE
  seguro em modo usuário. Bastou remover as flags `-mno-sse*` dos wrappers de
  cross-compilação (`scripts/cross/x86_64-elf-gcc`/`g++`) e rodar o build do newlib
  de novo — validado empiricamente que o GCC do host em x86_64 já habilita
  SSE/SSE2 por padrão, sem precisar de nenhuma flag positiva adicional
  (`libc-float-math`).
- **Printf/scanf com `%f`/`%e`/`%g` continua deliberadamente fora de escopo** — o
  HotSpot não depende disso para formatação numérica Java; fica como possível
  trabalho futuro.
- **Dois achados reais no próprio `scripts/cross/build-newlib.sh`**, anteriores a
  esta change mas só descobertos por ela: (1) o script nunca arquivava objetos de
  `newlib/libm` no `libc.a` final (só olhava `newlib/libc`), então `libm` nunca
  esteve disponível mesmo quando compilava; (2) a detecção do diretório-fonte do
  newlib (`NEWLIB_SRC`) também casava com os diretórios de build/instalação
  (`vendor/newlib-build`/`vendor/newlib-install`, nunca populados), escondido até
  uma build do zero ser tentada. Ambos corrigidos.
- **Achado crítico não relacionado a SSE/newlib, em `userland_c/crt0.S`**: `_start`
  herdava do kernel um `RSP` ajustado para a convenção de "função chamada" da ABI
  System V, mas `_start` é o ponto de entrada do processo, que a ABI exige começar
  alinhado a 16 bytes — o descompasso deixava `main` com `RSP` desalinhado em 8
  bytes, invisível em qualquer programa anterior (nenhum usava SSE) até o primeiro
  `movaps` real do projeto (`libm_test_program`) causar um `#GP` imediato.
  Corrigido com `and $-16, %rsp` em `_start`, mesma convenção de `crt0` real
  (glibc/musl) — sem nenhuma mudança no kernel.
- Verificado em boot real com um programa C de verdade: aritmética `double`,
  `sqrt`/`sin`, `strtod`/`atof` — todos com resultado correto; confirmado que os 6
  programas de teste C/C++ pré-existentes continuam passando sem regressão.

### Runtime C++ mínimo para programas de usuário
- **`new`/`delete`, despacho de método virtual** — `operator new`/`operator delete`
  delegando para `malloc`/`free` da libc já portada, `__cxa_pure_virtual` como stub
  defensivo. **Sem exceções, RTTI, nem construtores/destrutores de objetos globais**
  (achado real: `-fno-exceptions -fno-rtti -fno-use-cxa-atexit` elimina toda
  dependência dessas três coisas para um `.cpp` com herança/virtual compilar e linkar)
  (`cxx-minimal-runtime`).
- Verificado em boot real: despacho de método virtual puro através de ponteiro de
  classe-base executa a implementação derivada, e um objeto alocado via `new` é
  liberado via `delete` sem corromper nada.

### Esqueleto `os_javaos`/`os_cpu/javaos_zero` do HotSpot
- Primeiro contato direto com o código-fonte real do HotSpot: 19 arquivos novos em
  `os/javaos`/`os_cpu/javaos_zero` (variante **Zero** — interpretador puro em C++, sem
  geração de código de máquina, reaproveitada sem modificação), com assinatura
  conforme a interface real (`share/runtime/os.hpp`, ~18 métodos `pd_*`) e corpos
  majoritariamente stub (`fatal()`) (`os-javaos-skeleton`).
- Todos os 19 arquivos **compilam** contra a árvore real vendorizada (sparse-checkout
  de `src/hotspot` na tag `jdk-27-ga`, sem o `configure`/`make` do OpenJDK).
- Achados reais ao longo do caminho: `classfile_constants.h`/`jni_md.h` não são
  código-fonte de `src/hotspot` (vêm de `src/java.base`, vendorizados pontualmente); o
  HotSpot não é `-ffreestanding` (aplicação hosted, ao contrário do kernel); a
  variante Zero depende de `libffi-dev`; e um porte novo de verdade eventualmente
  precisará editar um arquivo compartilhado do HotSpot (`os.hpp`) para ganhar uma
  classe auxiliar `os::Javaos` — evitado por ora.

### `pd_*` de memória reais do esqueleto `os_javaos`
- **Os seis hooks de memória com primitiva de kernel equivalente têm comportamento
  real** (não mais `fatal()`): `pd_reserve_memory`, `pd_commit_memory` (2
  sobrecargas), `pd_commit_memory_or_exit` (2 sobrecargas), `pd_uncommit_memory`,
  `pd_release_memory`, e `os::protect_memory` (achado real: esta função nem existia
  no esqueleto original, não era "só" stub) — todos sobre uma nova API em C
  (`userland_c/include/javaos_vm.h`/`glue.c`) que encapsula as cinco syscalls de VM
  já existentes (`SYSCALL_VM_RESERVE`/`COMMIT`/`DECOMMIT`/`PROTECT`/`RELEASE`)
  (`os-javaos-memory-pd`).
- Verificado em boot real por um programa C dedicado (`userland_c/javaos_vm_test_program.c`,
  carregado como `/bin/javaosvm.elf` via o carregador ELF já existente): reservar,
  comprometer, escrever/reler, proteger como somente leitura, descomprometer, liberar,
  e dois casos de erro — tudo confirmado via a mesma API em C que os corpos reais de
  `os_javaos.cpp` usam. A tradução C++ de `os_javaos.cpp` em si continua verificada só
  por compilação (`-fsyntax-only` contra os headers reais do HotSpot) — linkar/rodar o
  arquivo real do HotSpot continua fora de escopo (ver destaque abaixo).
- `pd_*` de memória sem primitiva de kernel equivalente continuam `fatal()`
  explícito, comportamento inalterado: `pd_attempt_reserve_memory_at` (endereço
  fixo), `pd_attempt_map_memory_to_file_at`/`pd_map_memory`/`pd_unmap_memory`
  (mapeamento de arquivo), `pd_reserve_memory_special` (páginas grandes),
  `pd_realign_memory` (realinhamento).

### `PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/`PlatformParker` reais do porte `os_javaos`
- **`pthread_mutex_init`/`destroy`/`lock`/`unlock`/`trylock` e `pthread_cond_init`/
  `destroy`/`signal`/`broadcast`/`wait` têm comportamento real** em
  `userland_c/glue.c`, sobre as syscalls de mutex/condvar que o kernel já expõe —
  incluindo uma nova syscall (`SYSCALL_MUTEX_TRYLOCK`) para a tentativa de aquisição
  não-bloqueante, que não tinha primitiva equivalente antes. `destroy` devolve
  sucesso sem liberar o objeto no kernel (tabelas de handle já não suportam remoção,
  decisão anterior). Inicialização estática preguiçosa (`PTHREAD_MUTEX_INITIALIZER`/
  `PTHREAD_COND_INITIALIZER`) suportada, deliberadamente sem proteção contra corrida
  (seguro hoje — nenhuma syscall de criação de thread existe ainda)
  (`os-javaos-platform-sync`).
- **`PlatformMutex`/`PlatformMonitor` (`os/javaos/mutex_javaos.cpp`, novo arquivo —
  antes nem existia, nem como stub) têm comportamento real** sobre as funções acima:
  construção/destruição/`lock`/`unlock`/`try_lock` reais, `notify`/`notify_all`
  reais, e **`wait(millis)` real tanto para `millis == 0` (sem prazo) quanto para
  `millis != 0`** (com prazo, sobre a primitiva de espera com prazo do kernel —
  `kernel-timed-wait`) — devolve `OS_OK`/`OS_TIMEOUT` conforme o caso.
- **`PlatformEvent`/`PlatformParker` (`os/javaos/park_javaos.cpp`, novo arquivo)
  têm comportamento real** (`os-javaos-park-sync`): `PlatformEvent` como semáforo
  binário real de 3 estados (`park`/`park(millis)`/`park_nanos`/`unpark`, protocolo
  idêntico ao já vendorizado em `os/posix/os_posix.cpp`), e `PlatformParker`
  construído/destruído de verdade (mutex + dois `pthread_cond_t`, `REL_INDEX`/
  `ABS_INDEX`). **Fora de escopo, deliberadamente**: `Parker::park`/`unpark` (a
  classe compartilhada que de fato usa `PlatformParker`) — depende de
  `JavaThread::current()`/`ThreadBlockInVM`/`OSThreadWaitState`/safepoint,
  infraestrutura nunca referenciada por este porte; não é mais uma questão de
  primitiva do kernel faltando (ver roadmap, "Próximo passo imediato"). **Não
  verificado em boot real**: `PlatformEvent`/`PlatformParker` são classes C++
  internas do HotSpot sem nenhuma superfície chamável a partir de C — verificadas só
  por compilação real (`g++ -fsyntax-only`) contra a árvore vendorizada, mesma
  disciplina já aceita para partes do porte sem superfície C.
- Verificado em boot real por dois programas C dedicados
  (`userland_c/javaos_pthread_test_program.c`, carregado como `/bin/jpthread.elf`;
  `userland_c/timedwait_test_program.c`, carregado como `/bin/timedw.elf`):
  round-trip real de mutex (init/lock/trylock/unlock/destroy), de condvar sem
  esperador (init/signal/broadcast/destroy), e do **caminho de expiração** de
  `pthread_cond_timedwait` (prazo de 50 ms sem notificador, confirmando `ETIMEDOUT`
  e tempo decorrido compatível). **Não verificado em boot real**: bloqueio real +
  despertar por notificação (`pthread_cond_wait`/`pthread_cond_timedwait` antes do
  prazo) entre execuções distintas — não existe hoje nenhuma syscall de criação de
  thread em modo usuário para provar isso com segurança (chamar `cond_wait` sem
  ninguém para notificar de volta travaria o processo de teste para sempre).

### Isolamento de falha por processo
- **Uma exceção de CPU (`#PF`/`#GP`) causada por código de modo usuário encerra só a
  thread causadora, em vez de derrubar o kernel inteiro** (`kernel-fault-isolation`):
  os handlers de `kernel::idt` distinguem a origem pelo `RPL` de `CS` — modo kernel
  continua se comportando exatamente como antes (diagnóstico + halt total); modo
  usuário reaproveita o mesmo mecanismo já existente de `SYSCALL_EXIT`
  (`kernel::scheduler::terminate_current_thread`, extraída e compartilhada entre os
  dois casos). `#DF` (double fault) continua sempre fatal, sem distinção — o estado
  nesse ponto já está comprometido demais para confiar em qualquer decisão além de
  parar.
- **Equivalente a isolar o processo inteiro hoje**, não só a thread: nenhum processo
  de modo usuário tem mais de uma thread ainda (não existe syscall de criação de
  thread em modo usuário). Propagar o encerramento para outras threads do mesmo
  processo, quando essa syscall existir, fica para depois.
- Verificado em boot real por um programa C dedicado
  (`userland_c/fault_test_program.c`, carregado como `/bin/fault.elf`, depois de
  vários outros processos/threads de teste já em execução): causa um `#PF` em modo
  usuário de propósito (escrita em endereço `0x0`) e confirma que (a) nenhuma
  instrução sua depois da falta executa, e (b) todos os demais processos/threads já
  em execução continuam reportando exatamente os mesmos resultados de antes, sem
  nenhum `hlt_loop`/pânico.

### Liberação real de recursos do processo encerrado
- **A memória física do espaço de endereçamento de uma thread terminada volta a
  ficar disponível** (`kernel-process-cleanup`): `AddressSpace::free_user_mappings`
  percorre recursivamente a metade inferior da PML4 (nunca a superior,
  compartilhada) e devolve ao `kernel::phys::FrameAllocator` toda página de usuário
  mapeada e cada tabela intermediária (PDPT/PD/PT) — chamada de
  `kernel::scheduler::schedule_next` assim que uma thread é definitivamente
  excluída da rotação por ter terminado, tanto voluntariamente (`SYSCALL_EXIT`)
  quanto por exceção de CPU (`kernel-fault-isolation`). A própria PML4 (4 KiB) fica
  retida de propósito — evita um ponteiro *dangling* no `Process`, que continua
  existindo para sempre.
- Verificado em boot real: o log de boot mostra a liberação de toda thread que
  termina (voluntária ou involuntária), cada uma com um ganho positivo e plausível
  de frames livres (8 a 20, compatível com o tamanho real de cada ELF/stack) — não
  só `/bin/fault.elf`, mas também `test.elf`/`libctest.elf`/`cxxtest.elf`/
  `javaosvm.elf`/`jpthread.elf`/`libm.elf`/`timedw.elf`.
- **As tabelas de handle (`pthread_handles`, `file_handles`, janela de
  `kernel::vmm`) ganharam posse por processo** (`kernel-handle-ownership`):
  `kernel::process::Context` ganha `process: ProcessId`, e cada tabela passa a
  exigir o dono na criação e rejeitar (com o mesmo sentinela de "handle
  desconhecido" já existente, nenhum código de erro novo) qualquer consulta
  cujo chamador não seja o dono — um processo nunca mais consegue acessar o
  handle de outro. Ao terminar, `kernel::file_handles::close_all_owned_by` e
  `kernel::vmm::release_all_owned_by` fecham/liberam os arquivos/reservas do
  processo encerrado, chamadas do mesmo ponto de `schedule_next` que já libera
  a memória física (`kernel-process-cleanup`). **`pthread_handles` ganhou só a
  parte de isolamento** (posse + rejeição entre processos) — continua vazando
  os próprios objetos `Mutex`/`CondVar` (`Box::leak`'d), Non-Goal documentado
  desde `kernel-pthread-sync` e mantido deliberadamente nesta change (ver
  design.md de `kernel-handle-ownership`, "Por que `pthread_handles` continua
  sem destruição real").
- **Ainda vazam**: os próprios objetos de `pthread_handles` (acima), o pool de
  stacks de *kernel* (`kernel::stack`), e o próprio `Process`/`Thread` nas
  tabelas de bookkeeping do escalonador (nunca removidos dos vetores globais).
- Verificado em boot real (`cargo xtask test`): o log mostra, logo após o
  diagnóstico já existente de frames físicos liberados, `kernel_handle_ownership:
  thread N fechou X arquivo(s) e liberou Y reserva(s) de memoria virtual` para
  cada thread terminada que de fato possuía arquivos/reservas abertos (ex.:
  a thread coordenadora de `kernel::safepoint_demo`, liberando sua página de
  poll reservada ao terminar).

### Validação de ponteiro de usuário em argumentos de syscall
- **Toda syscall que recebe um ponteiro de modo usuário (`SYSCALL_WRITE`,
  `SYSCALL_DLOPEN`, `SYSCALL_DLSYM`, `SYSCALL_OPEN`, `SYSCALL_READ`) agora
  valida essa memória antes de acessá-la** (`kernel-user-pointer-validation`):
  nova `kernel::paging::Mapper::validate_user_range` confirma que toda página
  tocada pelo intervalo está mapeada, acessível a partir do modo usuário e
  (quando a chamada escreve nela) gravável — rejeitando com o novo sentinela
  `SYSCALL_EFAULT` se não. Fecha um buraco real de `kernel-fault-isolation`:
  antes, um ponteiro inválido causava `#PF` já em modo **kernel** (depois da
  transição de `syscall`), fora do alcance daquele isolamento, derrubando o
  kernel inteiro.
- **Escopo deliberadamente mínimo**: só distingue "ponteiro inválido"
  (`SYSCALL_EFAULT`) do sentinela genérico já existente (`SYSCALL_ERROR`) —
  não é um esquema de erro granular ao estilo POSIX (`ENOENT`/`EINVAL`/
  `EACCES`/etc. por chamada), que continua não implementado (a pedido
  explícito do usuário, consultado diretamente — ver design.md da change,
  "Alternativas consideradas").
- Verificado em boot real: novo `/bin/efault.elf` reporta `resultado PPPP`
  (ponteiro válido inalterado, ponteiro nulo/inválido rejeitado em
  `SYSCALL_WRITE`/`SYSCALL_OPEN`, buffer de leitura inválido rejeitado sem
  consumir o cursor do arquivo em `SYSCALL_READ`), sem nenhuma exceção de CPU.

### Esquema de erro granular estilo POSIX (`-errno`) em toda syscall
- **Toda syscall que falha agora devolve um código de erro POSIX real**
  (`kernel-syscall-errno`): `SYSCALL_ERROR`/`SYSCALL_TIMEOUT`/`SYSCALL_EFAULT`
  (três sentinelas fixos ad-hoc) foram removidos e substituídos por
  `kernel::syscall::syscall_err(errno)`, que codifica `-errno` como inteiro
  de 64 bits assinado reinterpretado como `u64` — a mesma convenção das
  *syscalls* reais do kernel Linux. Cada causa de falha já distinguível
  internamente (`VmmError`/`Fat32Error`/`DylibError`) ganhou um código
  específico (`EINVAL`/`ENOENT`/`EISDIR`/`ENOTDIR`/`EBADF`/`EBUSY`/
  `ENOMEM`/`EIO`/`ENODEV`/`ELOOP`/`ENOEXEC`/`ETIMEDOUT`/`ENOSYS`/`EFAULT`),
  catalogados em design.md da change, "Mapeamento de erro por syscall".
  `userland_c/glue.c` propaga o valor real para `errno` em cada função,
  corrigindo uma imprecisão real: `pthread_mutex_trylock` antes sempre
  devolvia `EBUSY`, mesmo para um `handle` desconhecido (deveria ser
  `EINVAL` — exatamente a distinção que o `pthread_mutex_trylock(3)` POSIX
  real faz).
- **Escopo deliberadamente mínimo quanto à granularidade de causas de baixo
  nível**: as 11 variantes de `DylibError` que representam alguma forma de
  "biblioteca ELF malformada" colapsam todas em `ENOEXEC` (nenhum errno
  POSIX real distingue, por exemplo, "segmento `PT_DYNAMIC` fora dos
  limites" de "tag obrigatória ausente" — ambas são só "não é uma
  biblioteca válida").
- **Achado real grave, corrigido durante a verificação em boot real**: a
  convenção `-errno` só funciona de forma genérica ("qualquer retorno
  negativo é erro") quando nenhum endereço de sucesso tem o bit 63 ligado —
  verdade no Linux (memória de usuário na metade inferior), falsa neste
  kernel, cuja janela de memória virtual anônima (`VMM_WINDOW_START =
  0xffff_c000_0000_0000`) fica na metade superior por decisão de
  `kernel-posix-virtual-memory`. A primeira versão de `glue.c` usava essa
  checagem genérica e derrubava `malloc`/`fopen`/as cinco operações de
  `javaos_vm_*` (todo endereço de sucesso da janela de VMM parecia um erro)
  — corrigido para verificar a faixa estreita de verdade (`errno` sempre
  < 300), não qualquer valor negativo. Registrado em `KNOW_ISSUES.md` com
  o racional completo, já que é uma armadilha real para qualquer código C
  futuro que inspecione o retorno bruto de uma *syscall* deste kernel.
- Verificado em boot real: todo programa de teste já existente (`file_syscalls_demo`,
  `javaos_vm_test_program`, `user_posix_demo`, `libc_test_program`,
  `efault_test_program`, etc.) reporta exatamente o mesmo padrão de sucesso
  de antes desta change.

### Parsing de ACPI/MADT
- **O kernel descobre a topologia real de interrupção via ACPI em vez de
  assumir os valores fixos corretos só para QEMU/VirtualBox/VMware**
  (`kernel-acpi-madt`): nova capacidade `kernel::acpi` localiza e valida o
  RSDP (via `RsdpRequest` do Limine)/RSDT ou XSDT/MADT, e extrai o endereço
  MMIO do I/O APIC e as substituições de origem de interrupção legada
  declaradas. `kernel::ioapic::init` usa o endereço descoberto quando
  disponível (com o endereço fixo de sempre como contingência);
  `kernel::keyboard` resolve o `GSI` real da IRQ1 via
  `kernel::acpi::resolve_isa_irq_gsi`, em vez de assumir identidade.
- **Verificado em boot real de forma mais rica do que o esperado**: o
  QEMU/OVMF deste projeto expõe uma MADT real e válida, não só o caminho de
  contingência — `acpi: MADT valida - I/O APIC em 0xfec00000 - 5
  substituicao(oes) de IRQ legada` (`IRQ0→GSI2`, o remapeamento clássico do
  PIT, mais 4 substituições identidade declaradas explicitamente). IRQ1
  (teclado) não tem substituição nesta plataforma, então nenhum
  comportamento observável muda, mas o caminho de descoberta via ACPI está
  genuinamente exercitado, não só codificado e nunca testado.
- **Dois achados reais graves, corrigidos durante a implementação**: um bug
  de auto-consistência que rejeitava toda tabela ACPI real maior que seu
  próprio cabeçalho (mascarado como "ACPI indisponível"), e uma alocação de
  heap antes de `kernel::heap::init` existir (aborta o boot inteiro) —
  ambos registrados em `KNOW_ISSUES.md` com o racional completo.
- **Fora de escopo, deliberadamente**: múltiplos I/O APICs, enumeração de
  outros processadores/Local APICs (SMP é item de prazo mais longo, abaixo),
  substituição de endereço do Local APIC (já vem de MSR, mais direto), e
  qualquer tabela ACPI além da MADT (FADT/HPET/MCFG/SRAT, interpretação de
  AML/DSDT/SSDT) — ver design.md da change, "Non-Goals".

## O que está previsto

### Próximo passo imediato
- Criação de thread nativa em modo usuário (`pthread_create`) — nova
  *syscall* de criação de thread + `os_javaos::pd_start_thread` real,
  reaproveitando `kernel::scheduler`/`kernel::process` já existentes.
  Bloqueio duro para o porte do HotSpot (ver "Destaque: o que falta para a
  JVM rodar", item 6.1): nenhuma JVM liga sem conseguir criar as threads
  nativas que ela mesma precisa (`VMThread`/`ServiceThread`/etc.), antes
  mesmo de qualquer escolha de GC/interpretador.

### Longo prazo
- Modelo híbrido de execução de JVM (runtime compartilhado vs. JVM isolada por
  aplicação) — roadmap item 9.
- "Java OS Platform" em Java: gerenciamento de arquivos, serviços de sistema, rede, APIs
  próprias, interface gráfica inicial — roadmap item 10.
- "Gaming Platform" em Java: launcher, ROM/Emulator/Save/Controller managers,
  emuladores próprios — roadmap item 11.
- SMP (múltiplos núcleos) — fora de escopo deliberado até aqui; exigiria revisar
  `Spinlock`, a stack de kernel confiável (hoje uma única variável global) e toda
  estrutura de dados global do escalonador.

## Destaque: o que falta para a JVM rodar

A base de primitivas de kernel que o HotSpot precisa (inventariada por
`jvm-posix-compat-strategy`) está **completa**: memória virtual anônima, `pthread_mutex`/
`pthread_cond`, relógio, sinal de safepoint, carregamento de executável/biblioteca
dinâmica, E/S de arquivo, uma libc C mínima com ponto flutuante real (`libm`+
conversão float, `libc-float-math`), um runtime C++ mínimo (`new`/`delete`,
despacho virtual), um esqueleto `os_javaos`/`os_cpu/javaos_zero` que compila contra
a interface real do HotSpot, os `pd_*` de memória desse esqueleto com comportamento
real (`os-javaos-memory-pd`), `PlatformMutex`/`PlatformMonitor` reais
(`os-javaos-platform-sync`), espera com prazo real no kernel/libc
(`kernel-timed-wait`), e `PlatformEvent`/`PlatformParker` reais
(`os-javaos-park-sync`). O que ainda falta, em ordem de dependência:

1. ~~Implementação de verdade dos `pd_*` do esqueleto `os_javaos`~~ — **concluído
   para memória** (`pd_reserve_memory`/`pd_commit_memory`/`pd_uncommit_memory`/
   `pd_release_memory`/`os::protect_memory` sobre as syscalls de VM já existentes,
   `os-javaos-memory-pd`). Os `pd_*` fora de memória (`pd_start_thread`,
   `pd_create_stack_guard_pages`, `pd_dll_unload`) continuam `fatal()`/stub — cobertos
   pelos itens 2 e seguintes.
2. ~~`PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/`PlatformParker` de verdade~~ —
   **concluído**: `PlatformMutex`/`PlatformMonitor`, incluindo `wait(millis)` com e
   sem prazo (`os-javaos-platform-sync` + `kernel-timed-wait`), e `PlatformEvent`
   (semáforo binário de 3 estados real, com e sem prazo) + a construção/destruição
   real de `PlatformParker` (`os-javaos-park-sync`) — todas sobre
   `pthread_mutex_*`/`pthread_cond_*`/`pthread_cond_timedwait`/`clock_gettime` reais
   e duas novas syscalls (`SYSCALL_MUTEX_TRYLOCK`, `SYSCALL_COND_WAIT` estendida com
   prazo). **Único resto, deliberadamente fora de escopo**: `Parker::park`/`unpark`
   (a classe compartilhada que dá uso real a `PlatformParker`) — depende de
   `JavaThread::current()`/`ThreadBlockInVM`/`OSThreadWaitState`/safepoint,
   infraestrutura de estado de thread que este porte nunca tocou; não é mais questão
   de primitiva de kernel faltando, é o próprio início da integração de
   runtime/estado de thread do HotSpot (ver item 5 abaixo e além).
3. ~~`libm`/conversão float completa na libc~~ — **concluído**
   (`libc-float-math`): `sin`/`cos`/`sqrt`/`pow`/etc. e `strtod`/`atof`/`dtoa` reais,
   sobre SSE habilitado no toolchain C/C++. `printf`/`scanf` com `%f`/`%e`/`%g`
   continua deliberadamente fora de escopo (ver roadmap, seção "`libm`/conversão
   float reais na libc C").
4. **Exceções C++, RTTI e `libstdc++`/STL** — desligados deliberadamente no runtime C++
   mínimo (`-fno-exceptions -fno-rtti`); o esqueleto `os_javaos` em si não precisou de
   nenhum dos três, mas isso cobre só a camada `os`/`os_cpu` — o restante do HotSpot
   (GC, JIT, interpretador) ainda não foi examinado, então se algum deles precisar
   disso continua sendo o escopo de uma sub-change própria.
5. ~~Isolamento de falha por processo~~ — **concluído para exceções de CPU**
   (`#PF`/`#GP` em modo usuário, `kernel-fault-isolation`): uma JVM que crashasse
   hoje encerraria só sua própria thread, não o kernel inteiro. Recursos do
   processo encerrado ainda vazam (ver roadmap, "Próximo passo imediato") — liberação
   real fica para depois, mas não bloqueia mais nenhum teste de boot real que rode
   código de usuário com risco de falhar.
6. Só depois disso: o próprio código-fonte do HotSpot (GC, JIT, interpretador,
   carregamento de classes via o `ClassLoader`) — mapeado por uma exploração
   (`openspec explore`) contra a árvore real vendorizada (`vendor/jdk-27-ga/
   src/hotspot`), ainda não implementado. Em ordem de dependência:
   1. **Criação de thread nativa em modo usuário (`pthread_create`) —
      próximo passo imediato**: nenhuma *syscall* de criação de thread em
      modo usuário existe hoje (`kernel::scheduler::spawn_sharing_current_
      address_space` só é usada internamente pelo próprio kernel), e
      `os_javaos::pd_start_thread` continua `fatal()`/stub (ver item 1
      acima). Bloqueio duro, não contornável por nenhuma escolha de GC/
      interpretador: o HotSpot cria várias threads nativas (`VMThread`,
      `ServiceThread`, `NotificationThread`, `MonitorDeflationThread`) só
      para inicializar a VM, antes de `Main.main` sequer rodar
      (`share/runtime/vmThread.cpp`/`serviceThread.cpp`/
      `notificationThread.cpp`/`monitorDeflationThread.cpp`). Pré-requisito
      puramente mecânico — reaproveita `kernel::scheduler`/`kernel::process`
      já existentes, não exige entender GC/interpretador ainda.
   2. **Escolha do coletor de lixo mais simples para o primeiro porte**:
      `share/gc/epsilon` (~15 arquivos — nunca coleta, só aloca até o heap
      estourar, zero lógica de marcação/compactação) é o candidato mais
      óbvio para uma primeira integração, frente a `share/gc/serial` (~29
      arquivos, o GC "de verdade" mais simples do HotSpot). `share/gc/
      shared` (~209 arquivos) é a base comum inevitável com qualquer
      escolha.
   3. **Interpretador**: `share/interpreter` (~36 arquivos —
      `templateInterpreter`/`templateTable`) é a infraestrutura do
      interpretador **com geração de código de máquina**, usada pelos
      ports com JIT (`cpu/x86`, etc.) — a variante Zero não usa essa
      infraestrutura. O interpretador de verdade da variante Zero vive em
      `cpu/zero` (~55 arquivos — `bytecodeInterpreter_zero.*`: um laço
      `switch` gigante em C++ puro, sem geração de código de máquina;
      `frame_zero`/`stack_zero`/`interpreterFrame_zero`: pilha de quadros
      simulada em C++, já que não há quadros de pilha de máquina reais).
   4. **`classfile`/`oops`** (parsing de `.class`/`ClassLoader`/verificador
      de bytecode, ~83 arquivos; representação de objeto/classe em
      memória, ~125 arquivos) — necessários independentemente da escolha
      de GC ou interpretador.
   5. **`runtime`** (~211 arquivos, o maior subsistema) — concentra
      `Thread`/`JavaThread`/`Arguments`/a sequência de inicialização da VM
      (`init.cpp`). A ordem exata de dependências internas (o que `init.cpp`
      chama, em que ordem, o que cada `os::`/`pd_*` precisa estar pronto
      antes) ainda não foi mapeada em detalhe — próxima exploração natural
      depois do item 1 acima.
   6. **`prims`** (JNI, ~95 arquivos) — escopo exato depende de quão mínimo
      o primeiro "hello world" pode ser (quantos métodos nativos de
      `java.base` um `System.out.println` mínimo realmente invoca).

## Destaque: o que falta para uma interface gráfica

Hoje existe **só** acesso de baixo nível ao framebuffer linear entregue pelo Limine, com
um console de texto de depuração desenhado por cima (fonte bitmap 8×8, usado para
espelhar a saída serial). Não há nenhuma camada gráfica real. Faltam, em camadas:

1. **Primitivas gráficas via syscall** — hoje só o próprio kernel desenha (para seu
   console de debug); nenhum processo de usuário pode desenhar um pixel, retângulo,
   linha, ou fazer *blit* de uma imagem.
2. **Driver de mouse** — não existe nenhum (nem PS/2 nem USB); só teclado PS/2.
3. **Compositor/gerenciador de janelas** — nenhum conceito de janela, z-order, clipping
   ou região de redesenho.
4. **Toolkit de widgets** (botões, texto, layout, despacho de evento) — inexistente.
5. **Dependência estrutural**: por decisão arquitetural (seção 10 do
   `docs/architecture/overview.md`), a GUI completa é responsabilidade de uma camada
   **Java** acima do kernel (roadmap item 10, "Java OS Platform") — não há plano de GUI
   nativa fora da JVM. Isso significa que o caminho crítico para qualquer interface
   gráfica passa **primeiro** pelo porte do HotSpot (destaque acima); não há atalho que
   contorne essa dependência.
