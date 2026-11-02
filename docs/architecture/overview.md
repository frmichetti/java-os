# Arquitetura do Java OS — Visão Geral

> Este documento consolida, como documentação permanente do projeto, a arquitetura
> técnica de referência definida originalmente em
> `openspec/changes/kernel-boot-foundation/design.md` (seção "Arquitetura do Sistema
> (Referência)"). Ele cobre as 15 áreas de arquitetura do Java OS. Apenas as áreas
> 1 (Kernel, incluindo interrupções/timer), 2 (Memória, exceto isolamento por
> processo e huge pages), e 12-14 (repositório/build/testes) estão implementadas até
> o momento (changes `kernel-boot-foundation`, `kernel-memory-management` e
> `kernel-interrupts-timer`); as demais ficam registradas aqui para orientar changes
> futuras.

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
  fault, double fault — este último com stack dedicada via uma entrada de IST no TSS,
  para continuar funcionando mesmo diante de um estouro da stack corrente). Um handler
  padrão (reconhece via EOI do Local APIC e retorna) cobre qualquer vetor de IRQ sem
  handler específico. Interrupções via Local APIC em modo **x2APIC** (MSR-based,
  obrigatório — sem fallback para xAPIC/MMIO); o PIC 8259 legado é desabilitado. O
  timer do Local APIC é calibrado contra o PIT 8254 e configurado para disparo
  periódico, mantendo um contador de ticks (`kernel::timer`) ainda não consumido por
  nenhum escalonador. Limitação conhecida: a stack do kernel (fornecida pelo Limine)
  não tem guard page — ver `KNOW_ISSUES.md`.
- **Concorrência no kernel**: inicialmente single-core; SMP é adiado para uma change
  futura, mas as estruturas de dados já nascem pensando em `Sync`/`Send`.

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
  do kernel está reservado a partir de `0xffff_9000_0000_0000` (índice 288).
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

## 3. Gerenciamento de Processos e Threads (futuro)

- Processos do kernel representam unidades de isolamento; threads são as unidades de
  escalonamento dentro de um processo.
- Escalonador planejado: preemptivo, baseado em timer (APIC timer).
- "Processo" no Java OS pode significar tanto um processo nativo do kernel quanto uma
  aplicação Java rodando dentro de uma JVM isolada.

## 4. System Calls (futuro)

- Interface prevista: `syscall`/`sysret` (x86_64), por desempenho.
- ABI e numeração de syscalls serão definidos na change que implementar a primeira
  syscall real.
- Separação esperada entre syscalls "de sistema" e a superfície exposta à JVM.

## 5. HAL e Drivers (futuro)

- HAL prevista como traits Rust (`trait BlockDevice`, `trait Console`, etc.).
- Primeiros drivers esperados, em ordem: console serial (já implementado), console de
  framebuffer, timer/APIC, teclado, armazenamento (AHCI/NVMe), rede.

## 6. Estratégia de Portabilidade do OpenJDK (futuro — maior risco técnico do projeto)

- Portar o HotSpot completo para um kernel próprio e não-POSIX é a tarefa de maior
  risco do projeto, pois o HotSpot assume um ambiente hospedeiro rico (libc, pthreads,
  mmap/mprotect POSIX, sinais Unix, `/proc`, etc.).
- Caminhos a avaliar em uma change dedicada: (1) camada de compatibilidade POSIX
  mínima no kernel; (2) porte direto das camadas de abstração de SO do próprio
  HotSpot; (3) JVM alternativa minimalista para bootstrap inicial em paralelo ao
  porte completo.

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
4. **Processos e threads + escalonador**: espaços de endereçamento por processo,
   troca de contexto, escalonador preemptivo.
5. **System calls**: ABI `syscall`/`sysret`, primeiras chamadas.
6. **HAL e drivers essenciais**: console/framebuffer, teclado, armazenamento, timer,
   (depois) rede.
7. **Pesquisa e decisão de estratégia de porte do OpenJDK/HotSpot** (change dedicada).
8. **Porte inicial da JVM**: primeira JVM minimamente funcional rodando um "hello
   world" Java sobre o Java OS.
9. **Modelo híbrido de execução de JVM**: runtime compartilhado vs. JVM isolada por
   aplicação.
10. **Java OS Platform (Java)**: gerenciamento de arquivos, serviços de sistema, rede,
    APIs próprias, interface gráfica inicial.
11. **Gaming Platform (Java)**: launcher, ROM/Emulator/Save/Controller managers,
    primeiros emuladores próprios.
12. **Hardening multi-hardware**: validação em hardware real além de
    QEMU/VMware/VirtualBox; SMP; drivers adicionais.
