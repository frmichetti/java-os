# Arquitetura do Java OS — Visão Geral

> Este documento consolida, como documentação permanente do projeto, a arquitetura
> técnica de referência definida originalmente em
> `openspec/changes/kernel-boot-foundation/design.md` (seção "Arquitetura do Sistema
> (Referência)"). Ele cobre as 15 áreas de arquitetura do Java OS. Apenas as áreas
> 1 (Kernel), parcialmente a 2 (Memória), e 12-14 (repositório/build/testes) estão
> implementadas até o momento (change `kernel-boot-foundation`); as demais ficam
> registradas aqui para orientar changes futuras.

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
- **Interrupções e exceções**: IDT própria, handlers para exceções de CPU (page fault,
  GPF, double fault com stack própria via IST) e para IRQs via APIC (não PIC 8259).
- **Concorrência no kernel**: inicialmente single-core; SMP é adiado para uma change
  futura, mas as estruturas de dados já nascem pensando em `Sync`/`Send`.

## 2. Arquitetura de Memória

- **Paginação**: 4-level paging (x86_64 padrão, 48-bit virtual address space), páginas
  de 4 KiB, com possibilidade futura de huge pages (2 MiB).
- **Layout de endereço virtual**: higher-half kernel — o kernel reside na metade
  superior do espaço de endereçamento, deixando a metade inferior livre para
  processos de usuário. O binário do kernel em si é linkado no topo dos 2 GiB
  superiores (`0xffffffff80000000`), exigido pelo `code-model=kernel`.
- **Alocador físico**: um frame allocator inicial simples sobre as regiões `usable`
  do memory map do Limine.
- **Alocador de heap do kernel**: ainda não implementado nesta fase — toda formatação
  de saída de boot usa buffers fixos de stack (`core::fmt::Write` sobre `[u8; N]`).
- **Isolamento de memória entre processos** (futuro): cada processo de
  usuário/aplicação Java terá seu próprio espaço de endereçamento (CR3 próprio).

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
│       ├── lib.rs              # módulos testáveis no host (serial, fmtbuf, memmap_report)
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
2. **Gerenciamento de memória**: alocador físico robusto, alocador de heap do kernel
   definitivo, paginação dinâmica, mapeamento/desmapeamento sob demanda.
3. **Interrupções e timer**: IDT completa, APIC/APIC timer, tratamento de exceções.
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
