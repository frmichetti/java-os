# Design

## Context

Não existe nenhum código hoje — apenas o briefing do projeto (`first-prompt.md`) e este diretório OpenSpec recém-criado. Ver `proposal.md` — Why para a motivação. Este documento cumpre dois papéis:

1. **Arquitetura de referência do Java OS** (seção "Arquitetura do Sistema (Referência)") — cobre as 15 áreas pedidas no briefing original, para servir de mapa estável para todas as changes futuras. A maior parte dessas áreas **não é implementada nesta change**; são decisões de arquitetura documentadas antecipadamente para que o kernel inicial já nasça compatível com o caminho que o resto do sistema vai seguir.
2. **Decisões de implementação desta change** (seção "Decisões desta Change") — o que efetivamente é construído agora: o boot do kernel e a base de testes, cobrindo a capability `kernel-boot` definida em `specs/kernel-boot/spec.md`.

Restrições conhecidas do ambiente: alvo único x86_64, sem suporte a 32 bits ou ARM64 nesta etapa; boot exclusivamente UEFI (sem BIOS/CSM); hardware alvo = desktop/notebook x86_64 reais + QEMU/VMware/VirtualBox. O ambiente de desenvolvimento atual não tem Rust, QEMU nem Limine instalados — isso faz parte do escopo de setup desta change.

## Goals / Non-Goals

**Goals:**
- Registrar uma arquitetura técnica completa e coerente para o Java OS, identificando riscos antes de escrever código de JVM.
- Entregar um kernel Rust `no_std` x86_64 que faz boot via UEFI/Limine no QEMU, imprime informações de boot e mapa de memória, e para de forma limpa.
- Entregar uma base de testes automatizados (smoke test de boot) reproduzível localmente e em CI.
- Deixar a estrutura de repositório pronta para receber, em changes futuras, memória, processos/threads, syscalls, HAL/drivers e o porte do OpenJDK sem necessidade de reestruturação.

**Non-Goals (nesta change):**
- Implementar gerenciador de memória virtual, escalonador, system calls reais, drivers de dispositivo, ou isolamento de processos — apenas documentar a arquitetura pretendida.
- Portar ou compilar qualquer parte do OpenJDK/HotSpot.
- Implementar GUI, rede, gerenciamento de arquivos, ou qualquer parte da Java OS Platform ou da Gaming Platform.
- Suportar BIOS legado, 32 bits, ARM64, ou hardware fora de x86_64/QEMU/VMware/VirtualBox.

---

## Arquitetura do Sistema (Referência)

> Esta seção documenta as decisões arquiteturais de longo prazo do Java OS. Apenas a subseção 1 (Kernel) e, de forma mínima, a subseção 2 (Memória) e 12-14 (repositório/build/testes) são implementadas nesta change. As demais (3 em diante) ficam registradas para orientar changes futuras e não possuem código associado ainda.

### 1. Arquitetura do Kernel

- **Linguagem/plataforma**: Rust `no_std` + Assembly x86_64 para os trechos que exigem controle direto (entrada de boot, troca de contexto, IDT/GDT stubs, instruções privilegiadas sem equivalente seguro em Rust).
- **Modelo de boot**: UEFI exclusivamente, via bootloader **Limine** (protocolo Limine boot protocol), que entrega ao kernel: mapa de memória, framebuffer, RSDP/ACPI, informações de módulos carregados, e outras boot requests. Limine é escolhido (em vez de escrever um bootloader próprio ou usar GRUB/Multiboot2) porque já resolve a complexidade de UEFI + long mode + paginação inicial, é mantido ativamente, e expõe exatamente as informações que o kernel precisa via uma API de "boot requests" simples de consumir do Rust.
- **Modelo de execução do kernel**: monolítico modular — um único espaço de endereçamento privilegiado (anel 0) contendo gerenciamento de memória, escalonador, drivers e IPC, organizado em crates Rust internas por subsistema (não microkernel). Escolhido sobre microkernel porque reduz a complexidade de IPC entre serviços do kernel nesta fase inicial; a reavaliação para isolar subsistemas críticos (ex.: drivers) fica registrada como questão em aberto para quando houver HAL/drivers reais.
- **Interrupções e exceções**: IDT própria, handlers para exceções de CPU (page fault, GPF, double fault com stack própria via IST) e para IRQs via APIC (não PIC 8259, dado hardware alvo moderno x86_64).
- **Concorrência no kernel**: inicialmente single-core (boot em um único AP); SMP (múltiplos cores) é adiado para uma change futura, mas o layout de memória e as estruturas de dados (ex.: alocadores) devem ser desenhados desde já pensando em `Sync`/`Send` para não exigir reescrita quando SMP for adicionado.

### 2. Arquitetura de Memória

- **Paginação**: 4-level paging (x86_64 padrão, 48-bit virtual address space), páginas de 4 KiB para uso geral, com possibilidade futura de huge pages (2 MiB) para regiões do kernel.
- **Layout de endereço virtual**: higher-half kernel — o kernel reside na metade superior do espaço de endereçamento (ex.: a partir de `0xffff_8000_0000_0000`), deixando a metade inferior livre para espaços de endereçamento de processos de usuário (relevante já para quando a JVM isolada por aplicação for introduzida).
- **Alocador físico**: um frame allocator inicial simples (bitmap ou lista encadeada sobre as regiões `usable` do memory map do Limine) — suficiente para esta change, que não aloca dinamicamente além do necessário para reportar informações de boot.
- **Alocador de heap do kernel**: necessário minimamente nesta change apenas se estruturas como `Vec`/`String` forem usadas para formatar a saída de boot; candidato é um allocator simples (bump ou linked-list) sobre uma região fixa reservada no mapa de memória, documentado explicitamente como provisório.
- **Isolamento de memória entre processos** (futuro, não implementado aqui): cada processo de usuário/aplicação Java terá seu próprio espaço de endereçamento (CR3 próprio), com o kernel mapeado de forma idêntica na metade superior em todos eles.

### 3. Gerenciamento de Processos e Threads (futuro)

- Processos do kernel (futuros) representam unidades de isolamento (um processo = um espaço de endereçamento); threads são as unidades de escalonamento dentro de um processo.
- Escalonador planejado: preemptivo, baseado em timer (APIC timer), com uma fila multinível ou round-robin simples na primeira versão, evoluindo conforme necessário para dar prioridade a threads de JVM/UI.
- Modelo híbrido de JVM (ver seção 7) implica que "processo" no Java OS pode significar tanto um processo nativo do kernel quanto uma aplicação Java rodando dentro de uma JVM isolada — a distinção exata entre "processo do SO" e "aplicação Java" é uma decisão a refinar quando esta capability for proposta.

### 4. System Calls (futuro)

- Interface prevista: `syscall`/`sysret` (x86_64) em vez de interrupção software (`int 0x80`), por desempenho.
- ABI e numeração de syscalls serão definidos na change que implementar o primeiro syscall real; nesta change o kernel não expõe nenhuma system call (não há espaço de usuário ainda).
- Separação esperada entre syscalls "de sistema" (usadas por serviços nativos do Java OS Platform) e a superfície exposta à JVM/aplicações Java, que provavelmente passa por uma camada de tradução em vez de syscalls diretas do bytecode.

### 5. HAL e Drivers (futuro)

- Camada de abstração de hardware (HAL) prevista como um conjunto de traits Rust (`trait BlockDevice`, `trait Console`, etc.) implementadas por drivers concretos, permitindo trocar/mockar dispositivos em testes.
- Primeiros drivers esperados, em ordem: console serial (já necessário nesta change), console de framebuffer, timer/APIC, teclado, armazenamento (AHCI/NVMe), rede.
- Nesta change, o único "driver" implementado é a saída serial (porta COM1, 16550 UART), usada para reportar boot e mapa de memória, e opcionalmente uma saída em framebuffer fornecida pelo Limine.

### 6. Estratégia de Portabilidade do OpenJDK (futuro — maior risco técnico do projeto)

- Avaliação preliminar (sem implementação): portar o HotSpot completo para um kernel próprio e não-POSIX é a tarefa de maior risco do projeto inteiro, porque o HotSpot assume um ambiente hospedeiro rico (libc, pthreads, mmap/mprotect POSIX, sinais Unix, `/proc`, etc.) que o Java OS não fornece nativamente.
- Caminhos possíveis a avaliar em uma change dedicada, antes de qualquer porte real:
  1. Implementar uma camada de compatibilidade POSIX mínima no kernel (subset de libc + pthreads + mmap) suficiente para hospedar o HotSpot com o menor número possível de patches — reduz esforço de porte, aumenta superfície do kernel.
  2. Portar diretamente as camadas de abstração de SO do próprio HotSpot (`os_linux.cpp`/`os_posix.cpp` equivalentes) para uma "os_javaos.cpp", sem camada POSIX intermediária — mais trabalho inicial, kernel permanece enxuto.
  3. Considerar uma JVM alternativa mais simples de portar para bootstrap inicial (ex.: um interpretador/JVM minimalista embarcado, tipo um subconjunto compatível com a especificação da JVM) enquanto o porte completo do HotSpot avança em paralelo — reduz risco de bloquear o projeto inteiro no porte do HotSpot.
- Esta decisão **não é tomada nesta change**; fica registrada como o maior risco técnico do projeto (ver Riscos) e será o foco de uma change de pesquisa/arquitetura dedicada antes de qualquer código de JVM.

### 7. Arquitetura da JVM (futuro)

- Modelo de execução híbrido avaliado no briefing: (a) um runtime compartilhado para serviços controlados do próprio Java OS (processos de sistema, confiáveis, podem compartilhar heap/classloading para reduzir overhead), e (b) uma JVM isolada por aplicação de terceiros (processo próprio, heap própria, permissões e limites de memória configuráveis).
- Implicações para o kernel: a JVM isolada precisa dos primitivos de processo/memória da seção 2-3 (espaço de endereçamento próprio, limites de memória) antes de poder existir — por isso este modelo híbrido não pode ser decidido em definitivo até que processos/threads sejam implementados.
- GC e JIT do HotSpot assumem controle fino sobre proteção de páginas (`mprotect`) e sinais (para GC "write barriers" via proteção de página, por exemplo) — isso retroalimenta a seção 6 (estratégia de porte) e a seção 4 (system calls precisam expor controle de proteção de memória equivalente a `mprotect`).

### 8. Modelo de Execução das Aplicações (futuro)

- Aplicações Java tradicionais (JAR) devem rodar dentro da JVM isolada por aplicação, com um verificador de permissões por aplicação (acesso a arquivos, rede, dispositivos) mediado pelo Java OS Platform, não diretamente pelo kernel.
- Aplicações "nativas" do Java OS (serviços de sistema, platform, gaming) podem rodar no runtime compartilhado quando forem consideradas confiáveis/parte do sistema.

### 9. APIs Java Próprias (futuro)

- Previstas APIs Java específicas do Java OS (fora do `java.*`/`javax.*` padrão) para: gerenciamento de aplicações, sistema de arquivos, dispositivos, rede/comunicação, processos Java, atualizações/configurações, e as APIs da Gaming Platform (launcher, ROM manager, emulator manager, save manager, controller manager).
- Essas APIs são a superfície que a Java OS Platform e a Gaming Platform (ambas em Java) usam para falar com o kernel — provavelmente implementadas como bindings finos sobre as system calls da seção 4, com a lógica de negócio em Java.

### 10. Arquitetura Gráfica (futuro)

- Caminho inicial de saída gráfica: framebuffer linear fornecido pelo Limine (modo gráfico configurado pelo bootloader antes de passar o controle ao kernel) — evita escrever um driver de GPU nesta fase.
- GUI completa (Java OS Platform) é responsabilidade de uma camada Java acima do kernel, desenhando sobre o framebuffer exposto por uma API do kernel/HAL; não há toolkit gráfico definido ainda.

### 11. Arquitetura de Emulação (futuro)

- Gaming Platform prevê múltiplos emuladores "próprios" (não integração de emuladores de terceiros via binário externo), implementados em Java, cada um como um módulo plugável gerenciado pelo Emulator Manager.
- Dependências esperadas: acesso a arquivos (ROMs), input de controles (Controller Manager faz a ponte com drivers de input do kernel), e possivelmente aceleração 2D/3D futura — nenhuma dessas dependências existe ainda nesta change.

### 12. Estrutura de Repositórios

Monorepo único (mais simples de versionar em conjunto nesta fase do projeto do que múltiplos repositórios), organizado como workspace Cargo na raiz:

```
java-os/
├── Cargo.toml                  # workspace raiz
├── rust-toolchain.toml         # pino do toolchain Rust (nightly)
├── x86_64-java_os.json         # target spec customizado
├── kernel/                     # crate do kernel (no_std)
│   ├── Cargo.toml
│   └── src/
├── boot/                       # integração com Limine (config, scripts de imagem)
├── xtask/                      # automação de build/run/test (cargo-xtask)
├── tests/
│   └── boot_smoke/             # teste automatizado de boot (QEMU)
├── docs/
│   └── architecture/           # este design.md evolui para cá após merge
├── .github/workflows/          # CI
└── openspec/                   # já existe (planejamento)
```

Componentes futuros (runtime Java/OpenJDK port, Java OS Platform, Gaming Platform) entram como diretórios/subprojetos irmãos de `kernel/` quando forem propostos, mantendo o monorepo único enquanto isso for viável.

### 13. Pipeline de Compilação

- **Kernel**: `cargo build` com target customizado (`x86_64-java_os.json`), `-Z build-std` (necessário para `no_std` em target customizado), toolchain nightly pinado via `rust-toolchain.toml`.
- **Imagem de boot**: um passo de `xtask` monta uma imagem de disco GPT/FAT com a estrutura esperada pelo Limine (binário do kernel + `limine.cfg` + arquivos do Limine), usando as ferramentas do próprio Limine.
- **Execução/teste**: `xtask run` levanta QEMU com `-bios OVMF.fd` apontando para a imagem gerada; `xtask test` roda o smoke test de boot (headless, com timeout, capturando serial).
- **CI (GitHub Actions)**: job único nesta change — build do kernel + `xtask test` em runner Linux com QEMU instalado via apt; falha de build ou de smoke test bloqueia o merge.

### 14. Estratégia de Testes

- **Nesta change**: apenas testes de boot de caixa-preta (smoke test via QEMU + captura de serial), já que não há lógica interna isolável em unidades ainda além do parsing do memory map.
- **Testes unitários no kernel** `no_std`: avaliar `custom_test_frameworks` do Rust para lógica pura (ex.: formatação do mapa de memória, parsing de boot requests) que não dependa de hardware — fica como extensão natural desta base de testes, não implementado nesta change por não haver ainda lógica não-trivial o suficiente para justificar.
- **Testes futuros** (memória, processos, syscalls): devem reusar a mesma infraestrutura de `xtask`/QEMU estabelecida aqui, estendendo os smoke tests com novos cenários por capability.

### 15. Roadmap Completo

1. **Esta change — `kernel-boot-foundation`**: arquitetura completa documentada; boot UEFI/Limine funcional no QEMU; relato de boot e mapa de memória; smoke test automatizado + CI.
2. **Gerenciamento de memória**: alocador físico robusto, alocador de heap do kernel definitivo, paginação dinâmica, mapeamento/desmapeamento sob demanda.
3. **Interrupções e timer**: IDT completa, APIC/APIC timer, tratamento de exceções de CPU.
4. **Processos e threads + escalonador**: espaços de endereçamento por processo, troca de contexto, escalonador preemptivo.
5. **System calls**: ABI `syscall`/`sysret`, primeiras chamadas (I/O básico, gerenciamento de memória de processo).
6. **HAL e drivers essenciais**: console/framebuffer, teclado, armazenamento, timer, (depois) rede.
7. **Pesquisa e decisão de estratégia de porte do OpenJDK/HotSpot** (change dedicada, sem código): avaliar as três opções da seção 6, decidir caminho antes de prosseguir.
8. **Porte inicial da JVM**: primeira JVM minimamente funcional rodando um "hello world" Java sobre o Java OS.
9. **Modelo híbrido de execução de JVM**: runtime compartilhado vs. JVM isolada por aplicação, permissões por aplicação.
10. **Java OS Platform (Java)**: gerenciamento de arquivos, serviços de sistema, rede, APIs próprias, interface gráfica inicial.
11. **Gaming Platform (Java)**: launcher, ROM/Emulator/Save/Controller managers, primeiros emuladores próprios.
12. **Hardening multi-hardware**: validação em desktop/notebook x86_64 reais além de QEMU/VMware/VirtualBox; SMP; drivers adicionais conforme necessidade.

---

## Decisões desta Change

Decisões técnicas concretas para o marco executável `kernel-boot` (ver `specs/kernel-boot/spec.md` para o comportamento exigido):

- **Target e toolchain**: Rust nightly pinado (via `rust-toolchain.toml`) + target JSON customizado `x86_64-java_os` (`code-model: kernel`, sem redzone, `panic = "abort"`), compilado com `-Z build-std=core,alloc`. Alternativa descartada: usar um target built-in (`x86_64-unknown-none`) — insuficiente porque não dá controle sobre code model e features de CPU necessárias no boot.
- **Bootloader**: Limine (binário pré-compilado + protocolo Rust via crate `limine`), consumido via boot requests estáticas (`BootloaderInfoRequest`, `MemoryMapRequest`, possivelmente `FramebufferRequest`). Alternativa descartada: `bootloader` crate (Rust puro) — menos madura para UEFI puro no momento desta decisão; GRUB/Multiboot2 — exige BIOS/CSM ou configuração UEFI mais complexa para o mesmo resultado.
- **Saída de boot**: console serial (porta COM1, UART 16550) como canal primário de verificação automatizada, por ser trivialmente capturável pelo QEMU (`-serial stdio` ou redirecionado a arquivo) sem depender de captura de framebuffer. O framebuffer do Limine pode ser usado como saída visual complementar, mas não é a fonte de verdade para o smoke test.
- **Alocação**: nenhuma alocação dinâmica obrigatória nesta change — formatação de strings de boot/memória é feita com buffers de tamanho fixo em stack (`core::fmt::Write` sobre um buffer `[u8; N]`) para evitar a necessidade de um heap allocator antes mesmo do primeiro boot funcionar. Um heap allocator mínimo fica como extensão natural, não bloqueante.
- **Halt**: `hlt` em loop com interrupções desabilitadas (`cli; loop { hlt }`) tanto no caminho de sucesso quanto no `panic_handler`, satisfazendo o requisito de "Comportamento Definido de Halt" da spec.
- **Automação de build/teste**: um crate `xtask` (padrão da comunidade Rust, evita dependência de `make`/shell scripts não portáveis) orquestra: build do kernel → montagem da imagem com as ferramentas do Limine → execução em QEMU → (no modo teste) captura de serial para arquivo + verificação de padrões esperados + timeout.
- **CI**: GitHub Actions, runner `ubuntu-latest`, instala QEMU e dependências do Limine via `apt`, executa `cargo xtask test`.

## Risks / Trade-offs

- **[Risco maior do projeto] Porte do OpenJDK/HotSpot para um kernel não-POSIX é de complexidade muito alta** → Mitigação: não decidir a estratégia de porte nesta change; tratar como change de pesquisa dedicada (seção 6) antes de qualquer código de JVM; manter a opção de uma JVM minimalista de bootstrap como plano B explícito caso o porte do HotSpot se mostre inviável no prazo desejado.
- **[Risco] Modelo monolítico modular do kernel pode dificultar isolamento de drivers/JVM mais tarde** → Mitigação: já organizar o kernel em crates internas por subsistema desde esta change, mesmo rodando tudo em anel 0, para que uma futura migração parcial a um modelo mais isolado (ex.: drivers em espaço separado) não exija reescrita completa.
- **[Risco] Ambiente de desenvolvimento atual não tem Rust/QEMU/Limine instalados** → Mitigação: a primeira tarefa de implementação é provisionar e documentar o ambiente (toolchain, QEMU, OVMF, Limine), e isso entra na definição de pronto do primeiro marco — sem isso, nada mais pode ser validado localmente.
- **[Risco] Ausência de hardware físico x86_64/UEFI para teste nesta etapa** → Mitigação: aceitar QEMU+OVMF como ambiente de verificação primário nesta change (é inclusive o que a spec exige); validação em hardware físico real fica para uma etapa posterior do roadmap (item 12), fora do escopo desta change.
- **[Trade-off] Escolher Limine em vez de escrever bootloader próprio** → Reduz drasticamente o escopo desta change e o risco de bugs em código de boot de baixo nível, ao custo de uma dependência externa; aceitável porque escrever um bootloader UEFI próprio não agrega valor diferencial ao objetivo do projeto (rodar Java).
- **[Trade-off] Nenhuma alocação dinâmica nesta change** → Simplifica o primeiro marco (não precisa de heap allocator correto para reportar boot/memória), mas significa que a formatação de saída fica limitada a buffers fixos; aceitável como passo inicial, heap allocator entra na change de gerenciamento de memória.

## Migration Plan

Não há sistema em produção para migrar — este é o primeiro código do projeto. "Deploy" aqui significa: repositório inicializado como projeto Git, estrutura de workspace criada, CI configurada e passando no smoke test de boot. Não há rollback aplicável além de reverter commits; nenhum dado ou estado externo é afetado.

## Open Questions

- ~~Qual código-fonte de referência exato do Limine (versão/tag) será pinado~~ **Resolvido na implementação**: branch `v11.x-binary` do repositório oficial (`github.com/limine-bootloader/limine`), fixada no commit `5be26a73d7b7b4d4477d18be94e1d16e615adf56` (vendorizada em `boot/limine/`, ignorada pelo Git). É a branch binária estável mais recente — a série `v12.x` ainda não teve uma branch `-binary` publicada. Crate Rust correspondente: `limine` v0.6.5 (crates.io).
- Se o framebuffer do Limine será usado como saída visual nesta mesma change (complementar ao serial) ou adiado — não afeta os requisitos da spec, que exige apenas saída serial como fonte de verdade testável.
- Formato exato do arquivo de configuração do Limine (`limine.cfg` vs. `limine.conf` da versão mais recente) — depende da versão pinada do Limine, decidir na implementação.
