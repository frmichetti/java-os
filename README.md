# Java OS

Sistema operacional de 64 bits construído do zero em Rust, com o objetivo de longo
prazo de rodar aplicações Java como plataforma principal. Veja
`docs/architecture/overview.md` para a arquitetura completa e
`openspec/changes/kernel-boot-foundation/` para o histórico de planejamento do
primeiro marco (boot do kernel via UEFI/Limine).

## Pré-requisitos

Este projeto é desenvolvido em **WSL2 (Ubuntu)** — o pipeline de build (montagem de
imagem GPT/FAT para o Limine, `readelf`, etc.) depende de ferramentas Unix. No Windows,
use o WSL2; em Linux nativo, os mesmos passos se aplicam diretamente.

1. **Rust nightly**: instale via [rustup](https://rustup.rs/) — a versão exata e os
   componentes (`rust-src`, `llvm-tools-preview`) são pinados em `rust-toolchain.toml`
   e selecionados automaticamente pelo `rustup`/`cargo` dentro do diretório do projeto.

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal
   source "$HOME/.cargo/env"
   rustup show   # baixa e seleciona automaticamente o toolchain pinado
   ```

2. **Ferramentas de sistema** (build de imagem, QEMU, OVMF, depuração):

   ```sh
   sudo apt update
   sudo apt install -y build-essential qemu-system-x86 ovmf xorriso mtools dosfstools gdisk gdb
   ```

3. **Limine** (bootloader), vendorizado localmente — não é versionado no Git
   (ver `.gitignore`), então precisa ser baixado e compilado uma vez:

   ```sh
   mkdir -p boot
   git clone https://github.com/limine-bootloader/limine.git --branch v11.x-binary --depth=1 boot/limine
   make -C boot/limine
   ```

   A versão pinada (branch `v11.x-binary`, commit `5be26a73d7b7b4d4477d18be94e1d16e615adf56`)
   está documentada em `openspec/changes/kernel-boot-foundation/design.md` (Open Questions).

## Build, execução e teste

Todos os comandos abaixo são rodados a partir da raiz do repositório.

| Comando | O que faz |
|---|---|
| `cargo kernel-build` | Compila só o kernel para o target bare-metal `x86_64-java_os.json`. |
| `cargo xtask build-image` | Compila o kernel e o executável de teste do carregador ELF, e monta a imagem de disco bootável (GPT + partição EFI FAT32) em `boot/dist/java-os.img`. |
| `cargo xtask run` | Builda a imagem e abre o QEMU (UEFI via OVMF) rodando o kernel. |
| `cargo xtask run -- <args do qemu>` | Como acima, repassando argumentos extras para o `qemu-system-x86_64` (ex.: `-- -serial stdio`). |
| `cargo xtask test` | Smoke test de boot: builda a imagem, roda o QEMU headless com timeout, captura a saída serial e falha se o banner de boot / mapa de memória não aparecerem. |
| `cargo test -p kernel --lib` | Testes unitários da lógica pura do kernel (ex.: formatação do mapa de memória), rodados no host — **não** usa `--target`, pois testa o crate como biblioteca comum. |

> `xtask` precisa rodar para o *host* (ele chama `sgdisk`/`mtools`/`qemu`), por isso
> nunca passe `--target x86_64-java_os.json` para `cargo build -p xtask` — isso é
> tratado automaticamente, já que não há target padrão global configurado em
> `.cargo/config.toml` (ver `[alias] kernel-build` lá para o comando completo do
> kernel).

### Exemplo: ver a saída de boot no terminal

```sh
cargo xtask run -- -serial stdio -display none
```

## Executável de teste do carregador ELF

`cargo xtask build-image` também compila `elf_test_program` (crate separada
do workspace, `elf_test_program/`) para o target de usuário
`x86_64-java_os_user.json` e o copia para `::/bin/test.elf` na partição FAT32
da imagem — usado por `kernel::scheduler::spawn_from_elf` (ver spec
`kernel-elf-loader`) para verificar o carregador de executáveis ELF64
estáticos em boot real, lendo um binário real do disco em vez de um programa
embutido no kernel.

Para verificar manualmente que o binário foi copiado corretamente para a
imagem (bytes idênticos ao compilado):

```sh
cargo xtask build-image
mcopy -i boot/dist/java-os.img@@1048576 ::/bin/test.elf /tmp/test_readback.elf
cmp target/x86_64-java_os_user/release/elf_test_program /tmp/test_readback.elf
```

## Depurando com GDB

1. Em um terminal, suba o QEMU parado no primeiro ciclo de instrução, com o gdbstub
   habilitado na porta 1234 (`-s` = `-gdb tcp::1234`, `-S` = não inicia a CPU até o
   GDB mandar `continue`):

   ```sh
   cargo xtask run -- -s -S -display none -serial stdio
   ```

2. Em outro terminal, conecte o GDB ao binário com símbolos e ao QEMU:

   ```sh
   gdb -q target/x86_64-java_os/debug/kernel \
       -ex "target remote :1234" \
       -ex "break kernel_main" \
       -ex "continue"
   ```

   O GDB deve parar exatamente no início de `kernel_main` (`kernel/src/main.rs`), de
   onde dá para inspecionar registradores (`info registers`), dar `step`/`next`, etc.

## Estrutura do repositório

Ver `docs/architecture/overview.md`, seção "12. Estrutura de Repositórios", para o
layout completo e o racional de cada diretório.
