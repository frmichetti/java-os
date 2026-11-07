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
`kernel-fpu-context-switch`, `cxx-minimal-runtime`, `os-javaos-skeleton` e
`os-javaos-memory-pd`.

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
- 22 chamadas de sistema no total hoje — todo erro ainda colapsa num único sentinela
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
  `reent`/`malloc` (via `sbrk`)/E-S de arquivo/término de processo. **Sem ponto
  flutuante** (`libm` inteira e a conversão `float`/`strtod` ficaram de fora — a ABI
  x86_64 não compila função alguma com `float`/`double` por valor sob SSE desabilitado,
  achado real durante a implementação) (`libc-minimal-port`).
- Verificado em boot real com um programa C de verdade: aloca memória, escreve no
  console, lê um arquivo do disco e termina — tudo via a libc portada.

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

## O que está previsto

### Próximo passo imediato
- **`PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/`PlatformParker` de verdade** —
  os headers (`mutex_javaos.hpp`/`park_javaos.hpp`) compilam usando os tipos
  `pthread_mutex_t`/`pthread_cond_t` que o newlib já declara, mas as funções
  `pthread_mutex_*`/`pthread_cond_*` em si ainda não têm implementação ligada às
  syscalls de mutex/condvar que o kernel já expõe (sem wrapper em
  `userland_c/glue.c`) — mesmo padrão de `javaos_vm.h`/`glue.c` recém-criado para as
  syscalls de VM (`os-javaos-memory-pd`). Alternativa continua valendo:
  vendorizar/instalar um JDK de bootstrap e tentar o `configure` real do OpenJDK
  apontando para este porte, para descobrir o próximo nível de dependências reais.

### Médio prazo
- Reconstruir `libm`/conversão float (`strtod`/`atof`) da libc agora que SSE está
  disponível (deliberadamente adiado por `libc-minimal-port`).
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
dinâmica, E/S de arquivo, uma libc C mínima, ponto flutuante via SSE, um runtime C++
mínimo (`new`/`delete`, despacho virtual), um esqueleto `os_javaos`/`os_cpu/javaos_zero`
que compila contra a interface real do HotSpot, e agora os `pd_*` de memória desse
esqueleto com comportamento real (`os-javaos-memory-pd`). O que ainda falta, em ordem
de dependência:

1. ~~Implementação de verdade dos `pd_*` do esqueleto `os_javaos`~~ — **concluído
   para memória** (`pd_reserve_memory`/`pd_commit_memory`/`pd_uncommit_memory`/
   `pd_release_memory`/`os::protect_memory` sobre as syscalls de VM já existentes,
   `os-javaos-memory-pd`). Os `pd_*` fora de memória (`pd_start_thread`,
   `pd_create_stack_guard_pages`, `pd_dll_unload`) continuam `fatal()`/stub — cobertos
   pelos itens 2 e seguintes.
2. **`PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/`PlatformParker` de verdade** —
   os headers (`mutex_javaos.hpp`/`park_javaos.hpp`) compilam usando os tipos
   `pthread_mutex_t`/`pthread_cond_t` que o newlib já declara, mas as funções
   `pthread_mutex_*`/`pthread_cond_*` em si ainda não têm implementação ligada às
   syscalls de mutex/condvar que o kernel já expõe (sem wrapper em `userland_c/glue.c`).
3. **`libm`/conversão float completa na libc** — o HotSpot usa `double`/`float`
   extensivamente (são tipos primitivos Java); a libc atual não tem `sin`/`cos`/`sqrt`/
   `strtod` reais (excluídos deliberadamente, não um bug).
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
