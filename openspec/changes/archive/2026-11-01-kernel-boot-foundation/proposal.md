# Proposal

## Why

O Java OS é um sistema operacional de 64 bits construído do zero, cujo objetivo de longo prazo é executar aplicações Java como plataforma principal (runtime, serviços de sistema, GUI e uma plataforma de jogos estão todos planejados sobre ele). Nada disso é possível sem um kernel capaz de inicializar de fato, reportar seu próprio estado e ser testado automaticamente — hoje não existe código, estrutura de repositório, nem decisões de toolchain registradas em lugar nenhum além do briefing do projeto (`first-prompt.md`). Antes de qualquer compromisso com o porte da JVM ou com serviços de nível de SO, o projeto precisa (1) de uma arquitetura técnica escrita cobrindo o sistema de ponta a ponta, para servir de referência estável aos próximos passos, e (2) de um kernel mínimo e verificável de forma independente que prove que o caminho de boot escolhido (UEFI + Limine + Rust `no_std` em x86_64) realmente funciona nos alvos de virtualização suportados.

## What Changes

- Estabelecer a arquitetura técnica completa do Java OS como documento de design: arquitetura do kernel, arquitetura de memória, modelo de processos/threads, superfície de system calls, estratégia de HAL/drivers, estratégia de porte do OpenJDK/HotSpot, arquitetura da JVM, modelo de execução de aplicações, APIs nativas do Java OS, arquitetura gráfica, arquitetura de emulação, estrutura de repositórios, pipeline de build, estratégia de testes e o roadmap completo do projeto. Esta parte é apenas documentação/planejamento — nenhum código de JVM, GUI ou emulação é escrito nesta change.
- Identificar e documentar antecipadamente os principais riscos técnicos, em particular a complexidade de portar uma JVM completa (OpenJDK/HotSpot) para um kernel próprio e não-POSIX, e as implicações disso no desenho futuro das APIs do kernel.
- Montar a estrutura inicial de repositório (layout de workspace para kernel, futuros componentes de runtime/platform/gaming, documentação, CI) conforme descrito no documento de arquitetura.
- Implementar o primeiro marco executável: um kernel Rust (`no_std`) x86_64 que inicializa via UEFI usando o bootloader Limine no QEMU, imprime informações de boot e memória (mapa de memória, informações básicas de CPU/boot) em uma saída visível (console serial e/ou framebuffer), e para de forma limpa em vez de travar.
- Estabelecer uma base de testes automatizados para o kernel: um teste de fumaça (smoke test) de boot que executa a imagem do kernel no QEMU (headless) e valida a saída de boot esperada, integrado a um comando local repetível e a um workflow de CI.
- Documentar instruções de build/execução/depuração (Cargo + Limine + QEMU/OVMF + GDB) para que o marco seja reproduzível por qualquer pessoa que clone o repositório.

## Capabilities

### New Capabilities
- `kernel-boot`: A sequência de boot UEFI do kernel via Limine em x86_64 — entrada a partir do bootloader, inicialização básica de hardware/CPU, obtenção e exibição de informações de boot e do mapa de memória, e um comportamento definido de halt/panic. Esta é a única capability com comportamento de spec exigido nesta change; todo o restante descrito acima é saída de arquitetura/planejamento, ainda não implementado.

### Modified Capabilities
- Nenhuma — esta é a primeira change do projeto; não existem specs anteriores para modificar.

## Impact

- **Nova estrutura de repositório**: cria o layout inicial do workspace (crate(s) do kernel, scripts de build, documentação, configuração de CI) onde hoje não existe nenhum (o projeto hoje contém apenas `first-prompt.md`).
- **Dependências de toolchain**: introduz um toolchain Rust nightly/pinned (para `no_std` + target customizado), o bootloader Limine (como dependência de build/submódulo), OVMF (firmware UEFI para QEMU), o próprio QEMU, e GDB para depuração. Nenhuma dessas ferramentas está instalada neste ambiente hoje e todas precisam ser provisionadas durante a implementação.
- **CI/CD**: introduz um workflow de GitHub Actions que compila o kernel e executa o smoke test automatizado de boot no QEMU a cada push/PR.
- **Trabalho futuro desbloqueado por esta change**: o documento de arquitetura produzido aqui se torna a referência para todas as changes subsequentes (gerenciador de memória, escalonador, system calls, HAL/drivers, porte do OpenJDK, integração da JVM, Java OS Platform, Gaming Platform) — esses itens permanecem fora do escopo desta change e serão propostos como changes incrementais separadas, construídas sobre as capabilities que este documento define.
