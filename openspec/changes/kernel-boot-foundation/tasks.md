# Tasks

## 1. Repositório e Ambiente de Build

- [ ] 1.1 Inicializar o repositório Git na raiz do projeto (`git init`, `.gitignore` para `target/`, imagens de disco geradas, binários do Limine baixados) e verificar com `git status` que apenas os arquivos de código-fonte/configuração ficam rastreados.
- [ ] 1.2 Criar o workspace Cargo (`Cargo.toml` na raiz + crate `kernel/`) conforme a estrutura de repositório da seção "12. Estrutura de Repositórios" do design, e verificar que `cargo metadata` resolve o workspace sem erro.
- [ ] 1.3 Adicionar `rust-toolchain.toml` pinando um nightly compatível com `-Z build-std` e os componentes `rust-src`/`llvm-tools-preview`, e verificar com `rustup show` (dentro do diretório do projeto) que o toolchain correto é selecionado automaticamente.
- [ ] 1.4 Criar o target spec customizado `x86_64-java_os.json` (code-model kernel, sem redzone, `panic=abort`, sem SSE/MMX flutuante por padrão) e verificar que `cargo build --target x86_64-java_os.json -Z build-std=core,alloc -p kernel` compila um crate kernel vazio (`#![no_std]`, sem `#![no_main]` ainda) sem erros de configuração de target.
- [ ] 1.5 Provisionar localmente (ou documentar o provisionamento, se feito em container/CI) as ferramentas externas necessárias — QEMU (`qemu-system-x86_64`), firmware OVMF, e as ferramentas de build de imagem do Limine — e verificar com `qemu-system-x86_64 --version` e a presença do binário/ferramenta do Limine que ambos estão disponíveis no PATH.

## 2. Boot Mínimo via Limine

- [ ] 2.1 Adicionar a dependência da crate `limine` (protocolo de boot requests) ao crate `kernel`, declarar as boot requests mínimas (`BootloaderInfoRequest`, `MemoryMapRequest`) como estáticas, e verificar que o crate compila com `#![no_std]#![no_main]` e um ponto de entrada `kernel_main` vazio.
- [ ] 2.2 Escrever o script de linker (`linker.ld`) posicionando o kernel na metade superior do espaço de endereçamento (higher-half, conforme seção "2. Arquitetura de Memória" do design) e configurar `kernel/Cargo.toml`/`build.rs` ou `.cargo/config.toml` para usá-lo, verificando com `readelf -l` (ou equivalente) que o binário gerado carrega nos endereços esperados.
- [ ] 2.3 Baixar/vendorizar a versão pinada do Limine (binários de boot + ferramentas) decidida em `design.md` (Open Questions) e criar o arquivo de configuração do Limine (`limine.cfg`/`limine.conf`, conforme a versão escolhida) apontando para o binário do kernel.
- [ ] 2.4 Criar o alvo `xtask` responsável por montar a imagem de disco GPT/FAT bootável (kernel + arquivos do Limine) a partir do binário compilado, e verificar que `cargo xtask build-image` produz um arquivo de imagem sem erros.
- [ ] 2.5 Criar o alvo `xtask run` que inicia QEMU com `-bios <OVMF>` apontando para a imagem gerada, e verificar manualmente que o QEMU abre, o firmware UEFI carrega o Limine, e o controle chega ao `kernel_main` (ex.: via um `hlt` imediato observável com `info registers` no monitor do QEMU) — cobre o cenário "Boot bem-sucedido no QEMU com OVMF" da spec.
- [ ] 2.6 Implementar a verificação da revisão do protocolo base do Limine reportada nas boot requests; se incompatível, desviar para um caminho que reporta o erro (reutilizando a saída serial da tarefa 3.1) em vez de continuar — cobre o cenário "Incompatibilidade de protocolo de boot é detectada" da spec. Verificar simulando uma revisão não suportada e confirmando que o kernel não prossegue com o boot normal.

## 3. Relato de Boot e Mapa de Memória

- [ ] 3.1 Implementar um driver mínimo de console serial (porta COM1, UART 16550) com uma macro estilo `serial_println!`, e verificar com `cargo xtask run -- -serial stdio` que uma mensagem de teste aparece no terminal.
- [ ] 3.2 Implementar o banner de boot (nome/versão do kernel, confirmação de boot, nome e revisão de protocolo do bootloader a partir de `BootloaderInfoRequest`), formatado em um buffer fixo de stack via `core::fmt::Write` (sem heap, conforme design), e verificar que o banner aparece na saída serial ao rodar `cargo xtask run` — cobre o cenário "Banner de boot é emitido".
- [ ] 3.3 Implementar o parsing e a impressão do mapa de memória a partir de `MemoryMapRequest` (endereço base, tamanho e tipo de cada região, mais o total de memória usável), e verificar visualmente na saída serial que todas as regiões reportadas pelo QEMU aparecem com valores plausíveis — cobre o cenário "Mapa de memória é exibido no boot".
- [ ] 3.4 Tratar explicitamente o caso em que `MemoryMapRequest` não retorna resposta, imprimindo uma mensagem de ausência em vez de valores zerados/fabricados, e verificar com um teste unitário (via `custom_test_frameworks` ou função isolada testável fora do hardware) que a função de formatação produz a mensagem de ausência quando recebe `None` — cobre o cenário "Mapa de memória indisponível".

## 4. Halt e Panic

- [ ] 4.1 Implementar a rotina de halt definitivo (`cli; loop { hlt }`) ao final da sequência de boot bem-sucedida, e verificar no monitor do QEMU (`info registers` repetido) que a CPU permanece parada em `hlt` sem reset — cobre o cenário "Conclusão normal termina em halt limpo".
- [ ] 4.2 Implementar o `#[panic_handler]` para imprimir mensagem e localização do panic na saída serial e então entrar na mesma rotina de halt, e verificar forçando um `panic!()` de teste (temporário, removido após a verificação) que a mensagem aparece na serial e a CPU para sem reiniciar — cobre o cenário "Panic halta limpo com diagnóstico".

## 5. Teste Automatizado de Boot e CI

- [ ] 5.1 Criar o alvo `xtask test` (ou crate `tests/boot_smoke`) que executa `cargo xtask run` em modo headless redirecionando a saída serial para um arquivo, com um timeout limitado, e mata o processo QEMU ao final.
- [ ] 5.2 Implementar a verificação automatizada que lê a saída serial capturada e falha (`exit code != 0`) se os marcadores esperados do banner de boot e do mapa de memória (tarefas 3.2/3.3) não estiverem presentes, e passa (`exit code == 0`) quando estão — cobre o cenário "Smoke test de boot passa em um boot bem-sucedido".
- [ ] 5.3 Verificar o caminho de falha do smoke test simulando um boot quebrado (ex.: branch temporário que trava antes do banner) e confirmando que `cargo xtask test` retorna falha dentro do timeout em vez de travar indefinidamente — cobre o cenário "Smoke test de boot falha em um boot quebrado". Reverter a quebra simulada após confirmar.
- [ ] 5.4 Criar o workflow de GitHub Actions (`.github/workflows/kernel-ci.yml`) que instala QEMU/dependências em `ubuntu-latest`, builda o kernel e executa `cargo xtask test`, e verificar que o workflow passa em uma execução (local via `act` ou observando a execução real após push).

## 6. Documentação

- [ ] 6.1 Escrever `docs/architecture/overview.md` consolidando o conteúdo da seção "Arquitetura do Sistema (Referência)" de `design.md` como documentação permanente do projeto (fora de `openspec/`), e verificar que o arquivo existe e referencia corretamente as 15 áreas de arquitetura.
- [ ] 6.2 Escrever um `README.md` na raiz com instruções de build/execução/depuração (toolchain necessário, `cargo xtask build-image`, `cargo xtask run`, `cargo xtask test`, como anexar o GDB ao QEMU via `-s -S`), e verificar seguindo o README do zero (ambiente limpo ou container) que todos os comandos documentados funcionam como escrito.
