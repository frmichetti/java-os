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

### `PlatformMutex`/`PlatformMonitor` reais do porte `os_javaos`
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
  reais, e `wait(millis)` real só para `millis == 0` (sem timeout) — `millis != 0`
  continua `fatal()` explícito, já que nenhuma primitiva de espera com prazo existe
  no kernel. `PlatformEvent`/`PlatformParker` (`park_javaos.hpp`) continuam fora de
  escopo, pela mesma razão.
- Verificado em boot real por um programa C dedicado
  (`userland_c/javaos_pthread_test_program.c`, carregado como `/bin/jpthread.elf`):
  round-trip real de mutex (init/lock/trylock/unlock/destroy) e de condvar sem
  esperador (init/signal/broadcast/destroy). **Não verificado em boot real**:
  bloqueio real + despertar de `pthread_cond_wait` entre execuções distintas — não
  existe hoje nenhuma syscall de criação de thread em modo usuário para provar isso
  com segurança (chamar `cond_wait` sem ninguém para notificar de volta travaria o
  processo de teste para sempre).

## O que está previsto

### Próximo passo imediato
- **Primitiva de espera com prazo em `kernel::wait_queue`** (temporizador +
  despertar) — hoje toda espera bloqueante é indefinida, sem parâmetro de tempo;
  bloqueia `PlatformMonitor::wait(millis != 0)`/`PlatformEvent::park(millis)`/
  `park_nanos` (`os-javaos-platform-sync`) e qualquer `pthread_cond_timedwait`-like
  futuro.

### Médio prazo
- Isolamento de falha por processo — hoje uma exceção em modo usuário ainda derruba o
  kernel inteiro; depende de um modelo real de término/encerramento de processo.
- Isolar as tabelas de handle hoje globais (`pthread_handles`, `file_handles`, janela de
  `kernel::vmm`) por processo/espaço de endereçamento.
- Esquema de erro granular (`errno`-like) nas syscalls, e validação de ponteiro de
  usuário (hoje não validado, mesmo precedente de `sys_write`).
- Parsing de ACPI/MADT, para hardware real além de QEMU/VirtualBox/VMware.

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
real (`os-javaos-memory-pd`), e `PlatformMutex`/`PlatformMonitor` reais
(`os-javaos-platform-sync`). O que ainda falta, em ordem de dependência:

1. ~~Implementação de verdade dos `pd_*` do esqueleto `os_javaos`~~ — **concluído
   para memória** (`pd_reserve_memory`/`pd_commit_memory`/`pd_uncommit_memory`/
   `pd_release_memory`/`os::protect_memory` sobre as syscalls de VM já existentes,
   `os-javaos-memory-pd`). Os `pd_*` fora de memória (`pd_start_thread`,
   `pd_create_stack_guard_pages`, `pd_dll_unload`) continuam `fatal()`/stub — cobertos
   pelos itens 2 e seguintes.
2. ~~`PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/`PlatformParker` de verdade~~ —
   **concluído para `PlatformMutex`/`PlatformMonitor`** (`os-javaos-platform-sync`),
   incluindo as funções `pthread_mutex_*`/`pthread_cond_*` reais por trás deles e uma
   nova syscall (`SYSCALL_MUTEX_TRYLOCK`). `PlatformEvent`/`PlatformParker`
   (`park_javaos.hpp`) continuam fora de escopo — dependem de espera com prazo, que
   nenhuma primitiva do kernel oferece ainda (ver roadmap, "Próximo passo imediato").
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
5. **Isolamento de falha por processo** — hoje uma JVM que crashasse derrubaria o kernel
   inteiro junto; aceitável para um "hello world" de prova de conceito, não para um
   sistema de verdade.
6. Só depois disso: o próprio código-fonte do HotSpot (GC, JIT, interpretador,
   carregamento de classes via o `ClassLoader`) — ainda não escopado em nenhuma change.

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
