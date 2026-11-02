# Proposal

## Why

O kernel resultante de `kernel-boot-foundation` não tem heap nem gerenciamento de memória dinâmica: toda formatação de boot usa buffers fixos de stack e não existe nenhuma forma de alocar memória física, criar estruturas com `alloc` (`Vec`, `String`, `Box`), ou mapear/desmapear páginas virtuais. Isso bloqueia o próximo item do roadmap (`docs/architecture/overview.md`, item 2 — "Gerenciamento de memória") e qualquer trabalho futuro de interrupções/timer, processos/threads, HAL/drivers ou porte do OpenJDK, que dependem de poder alocar memória em tempo de execução.

## What Changes

- Implementar um alocador de frames físicos (bitmap) sobre as regiões `usable` do memory map do Limine, reaproveitando o parsing já existente de `kernel::memmap_report` (tarefa 3.3 de `kernel-boot-foundation`).
- Implementar um heap allocator do kernel (`#[global_allocator]`), usando a HHDM (higher-half direct map, fornecida pelo Limine via `HhdmRequest`) para acessar memória física diretamente — sem depender de paginação própria do kernel — liberando o uso de `alloc::{Vec, String, Box}` no restante do kernel.
- Implementar gerenciamento próprio de tabelas de página x86_64 (4 níveis): mapear e desmapear páginas virtuais arbitrárias com flags de permissão (leitura/escrita/execução, usuário/supervisor), sobre as estruturas de paginação que o Limine já deixa ativas ao entregar o controle ao kernel.
- Reportar no banner de boot (saída serial já existente) estatísticas básicas do alocador físico (total de frames, frames livres) após a inicialização do gerenciador de memória.

## Capabilities

### New Capabilities
- `kernel-physical-memory`: alocação e liberação de frames físicos (unidades de 4 KiB) a partir das regiões usáveis do memory map do bootloader, incluindo rastreio de quantos frames estão livres/em uso.
- `kernel-heap`: um heap allocator do kernel (`GlobalAlloc`) que permite o uso de tipos de `alloc` (`Vec`, `String`, `Box`, etc.) em qualquer parte do kernel, crescendo sob demanda a partir de frames físicos.
- `kernel-paging`: mapeamento e desmapeamento de páginas virtuais de 4 KiB para frames físicos arbitrários, com controle de flags de permissão, sobre a paginação de 4 níveis do x86_64.

### Modified Capabilities
- Nenhuma — o comportamento de boot definido em `kernel-boot` (`openspec/specs/kernel-boot/spec.md`) não muda; esta change apenas adiciona capacidades novas que rodam depois da sequência de boot já especificada.

## Impact

- **Novos módulos no crate `kernel`**: alocador físico, heap allocator, e gerenciamento de tabelas de página, somando-se aos módulos existentes (`serial`, `fmtbuf`, `memmap_report`).
- **Primeiro uso real de `alloc`**: o build-std já inclui `alloc` desde `kernel-boot-foundation`, mas nenhum código o usa em tempo de execução (não há `#[global_allocator]`); esta change registra o primeiro allocator global do projeto.
- **Nenhuma dependência externa nova esperada**: segue a filosofia atual do projeto (implementações mínimas e diretas, sem crates de terceiros para o núcleo do kernel), mas a decisão final de usar ou não um crate de allocator (ex.: `linked_list_allocator`) fica para `design.md`.
- **Saída de boot (serial)**: o banner existente ganha uma linha adicional com estatísticas do alocador físico; o smoke test de boot (`cargo xtask test`) pode precisar de um novo marcador para validar essa saída.
- **Sem impacto em CI/infra**: reaproveita o mesmo pipeline de build/teste (`cargo xtask build-image`/`run`/`test`) estabelecido em `kernel-boot-foundation`.
