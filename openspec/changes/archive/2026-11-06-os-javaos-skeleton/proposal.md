# Proposal

## Why

Todas as primitivas de kernel que o HotSpot precisa (memória virtual, `pthread_mutex`/`pthread_cond`, relógio, safepoint, carregamento de executável/biblioteca, E/S de arquivo, libc C, ponto flutuante, runtime C++) estão entregues (`jvm-posix-compat-strategy` até `cxx-minimal-runtime`). O próximo passo do item 8 do roadmap é o esqueleto `os_javaos` — a camada de abstração de SO (`src/hotspot/os/<plataforma>`) que o HotSpot expõe para cada porte, análoga a `os_linux`/`os_bsd`. Nenhuma linha de código do HotSpot foi integrada ainda.

## What Changes

- **Achado de pesquisa**: compilar o OpenJDK de verdade (via seu `configure`/`make`) exige um JDK de bootstrap já instalado (build de JDK N precisa de JDK N-1) e todo o build system (autoconf/make, detecção de plataforma) — infraestrutura bem mais pesada que qualquer coisa feita até aqui. Esta change **não** faz isso ainda (fica para uma sub-change futura).
- **Achado de pesquisa**: a interface real que um porte implementa (`src/hotspot/share/runtime/os.hpp`, 1187 linhas na versão vendorizada) já depende de `utilities/globalDefinitions.hpp`/`exceptions.hpp`/`ostream.hpp` e outras partes do código compartilhado do HotSpot — não é possível compilar um `os_javaos.cpp` real "isolado" sem esses headers reais. Por isso, o código-fonte do OpenJDK (`src/hotspot` — não o repositório inteiro) é vendorizado localmente só para fornecer os headers reais via `-I`, sem rodar o `configure`/`make` do OpenJDK.
- Vendorização local de `src/hotspot` do OpenJDK (`jdk-27-ga`, a tag GA mais recente no momento desta pesquisa), não versionado no Git — mesmo tratamento de `boot/limine`/`vendor/newlib-*`.
- Novos diretórios `src/hotspot/os/javaos/` e `src/hotspot/os_cpu/javaos_zero/` (dentro da árvore vendorizada), modelados a partir de `os/bsd`+`os_cpu/bsd_zero` e `os/linux`+`os_cpu/linux_zero` (usados como referência cruzada, nunca copiados literalmente) — implementando a assinatura real de cada função exigida pela interface, com corpos propositalmente mínimos/stub (a maioria devolve erro ou chama `abort()`; só o necessário para o arquivo compilar de verdade contra os headers reais).
- A variante **Zero** do HotSpot (interpretador puro, sem JIT nem assembly específico de arquitetura) é reaproveitada como está — `src/hotspot/cpu/zero` já existe no OpenJDK e não precisa de nenhum código novo nosso, evitando a parte mais difícil de um porte (gerar código de máquina).
- Verificação: compilar os novos arquivos `os_javaos`/`os_cpu/javaos_zero` diretamente com o wrapper `scripts/cross/x86_64-elf-g++` contra os headers reais vendorizados (via `-I`) — **não** através do `configure`/`make` do próprio OpenJDK, que fica para uma sub-change futura. **Resultado esperado explicitamente incerto**: dado o tamanho e o grau de acoplamento do código compartilhado do HotSpot, é aceitável (e deve ser documentado como achado, não escondido) que nem todos os arquivos cheguem a compilar nesta primeira tentativa — tasks.md rastreia o progresso real arquivo por arquivo, não promete 100% de antemão.

## Capabilities

### New Capabilities

- `os-javaos-skeleton`: a camada de abstração de SO do HotSpot para o Java OS (`os_javaos`/`os_cpu/javaos_zero`) — os arquivos existem, seguem a assinatura real da interface `os.hpp`/`osThread.hpp` etc. do HotSpot, e compilam contra os headers reais do OpenJDK vendorizado usando o toolchain já existente do projeto.

### Modified Capabilities

(nenhuma)

## Impact

- **Código afetado**: novo diretório vendorizado `vendor/jdk-27-ga/src/hotspot` (não versionado no Git); novos arquivos em `os/javaos`/`os_cpu/javaos_zero` dentro dessa árvore vendorizada; possivelmente um pequeno script de build (`scripts/cross/build-os-javaos-skeleton.sh` ou similar) que invoca `x86_64-elf-g++` com os `-I` corretos para cada arquivo.
- **Dependências novas**: código-fonte do OpenJDK (`src/hotspot`, não o repositório inteiro) vendorizado localmente — nenhuma ferramenta de build nova (reaproveita `scripts/cross/x86_64-elf-g++` já existente).
- **Compatibilidade**: puramente aditivo — nenhum código Rust/C/C++ já existente muda.
- **Documentação**: `docs/architecture/overview.md` (seção 6) e `ROADMAP.md` precisam de uma entrada registrando o escopo real entregue (quais arquivos compilam, quais não, por quê) — não uma alegação genérica de "esqueleto pronto".
