# Proposal

## Why

O próximo passo do item 8 do roadmap (`ROADMAP.md`, "Destaque: o que falta para a JVM rodar") é o esqueleto `os_javaos` do HotSpot — e o HotSpot é C++. Hoje não existe absolutamente nenhum suporte a C++ no Java OS: só há uma libc C (newlib, change `libc-minimal-port`) e um wrapper de cross-compilador só para C (`scripts/cross/x86_64-elf-gcc`). Sem runtime C++, nenhum arquivo `.cpp` do HotSpot consegue sequer linkar — este gap precisa ser fechado antes de vendorizar qualquer código-fonte do HotSpot.

## What Changes

- Novo wrapper `scripts/cross/x86_64-elf-g++` (mesmo padrão de `x86_64-elf-gcc`): GCC do host (host e alvo são ambos x86_64) com `-ffreestanding -nostdlib -fno-pie -no-pie -fno-stack-protector`, as mesmas flags de sem-SSE já usadas pelo wrapper de C, e três flags específicas de C++ — `-fno-exceptions -fno-rtti -fno-use-cxa-atexit`.
- Novo runtime C++ mínimo (`userland_c/cxx_runtime.cpp`): `operator new`/`operator new[]`/`operator delete`/`operator delete[]` (incluindo as variantes com tamanho) delegando para `malloc`/`free` da libc C já portada, e `__cxa_pure_virtual` como stub defensivo (chamada a método virtual puro nunca sobrescrito é um bug de programação, não um estado recuperável).
- **Achado confirmado experimentalmente nesta sessão**: com `-fno-exceptions -fno-rtti -fno-use-cxa-atexit`, um `.cpp` com herança e método virtual compila e linka só precisando desses símbolos — nenhuma dependência de exceções, RTTI ou `__cxa_atexit`/`__dso_handle` (construtores/destrutores globais) aparece.
- Verificação em boot real: um programa C++ de teste (classe com herança, método virtual, `new`/`delete`) carregado via o carregador ELF já existente (`kernel::elf`/`spawn_from_elf`), confirmando despacho virtual correto e que `new`/`delete` realmente passam pela libc.

## Capabilities

### New Capabilities

- `cxx-runtime`: o runtime C++ mínimo disponível para programas de modo usuário do Java OS — alocação/liberação de objetos via `new`/`delete` (sobre a libc C já existente) e despacho de método virtual, sem exceções, RTTI, nem construtores/destrutores de objetos globais.

### Modified Capabilities

(nenhuma)

## Impact

- **Código afetado**: novo wrapper `scripts/cross/x86_64-elf-g++`; novo arquivo `userland_c/cxx_runtime.cpp`; `xtask` ganha uma etapa de build para compilar/linkar o programa C++ de verificação e copiá-lo para a imagem de disco; `kernel_main` ganha o carregamento desse executável de teste.
- **Dependências**: nenhuma nova além do GCC do host (já exigido desde `libc-minimal-port`) — nenhum compilador C++ dedicado precisa ser construído.
- **Compatibilidade**: puramente aditivo — nenhum código C ou Rust existente muda.
- **Documentação**: `docs/architecture/overview.md` (seção 6) e `ROADMAP.md` precisam de uma entrada registrando este runtime C++ mínimo como entregue, e atualizando o próximo passo (esqueleto `os_javaos` em si, agora com libc C e runtime C++ disponíveis — ainda sem `libstdc++`/STL, exceções ou RTTI).
