# Proposal

## Why

O inventário de `jvm-posix-compat-strategy` (ver `docs/architecture/overview.md`, seção 6) identifica "E/S de arquivo (`open`/`read`, para carregar classes do class path)" como um gap: `kernel::storage`/`hal::BlockDevice` (change `kernel-storage`, arquivada) só dão acesso a blocos brutos por LBA — não existe nenhum conceito de arquivo, diretório ou caminho. O próprio design.md de `kernel-storage` já registrou isso como Non-Goal explícito: *"Qualquer sistema de arquivos (FAT32, ext2, etc.) — fica para uma change futura, consumindo o `BlockDevice` desta."* Esta é essa change futura, e fecha mais um gap do item 8 do roadmap (seção 15), depois de `kernel-safepoint-signal`.

A imagem de disco que o próprio projeto já monta (`xtask`, via `sgdisk`+`mformat`+`mcopy`) já é GPT com uma única partição EFI formatada em FAT32 de verdade, contendo o binário do kernel e o bootloader — uma partição real e utilizável para verificar o driver em boot real, sem precisar inventar ou formatar nada novo.

## What Changes

- O kernel passa a conseguir localizar a partição FAT32 do disco (lendo e interpretando a tabela de partição GPT via `kernel::storage`/`hal::BlockDevice` já existente — sem hardcodar o LBA inicial que o `xtask` usa para montar a imagem) e montar essa partição como um sistema de arquivos FAT32, oferecendo uma operação de "ler um arquivo inteiro, dado seu caminho absoluto" (ex.: `/boot/kernel`), resolvendo diretórios e a cadeia de clusters do arquivo via a FAT.
- **Decisão de escopo** (ver design.md para alternativas descartadas): somente leitura (nenhuma escrita/criação/exclusão) e somente nomes curtos 8.3 (sem nomes longos VFAT) — suficiente para a necessidade real motivadora (ler classes do class path) e para os arquivos já presentes na imagem de boot (`KERNEL`, `BOOTX64.EFI`, `BOOT`, todos nomes 8.3 válidos nativamente), evitando a complexidade de escrita segura num FAT real e do algoritmo de reconstrução de nomes longos VFAT.
- A exposição disso como *syscalls* (`open`/`read`) a modo usuário fica para uma sub-change futura — mesma disciplina incremental já seguida por todas as sub-changes anteriores do item 8 (`kernel-wait-queue`, `kernel-posix-virtual-memory`, `kernel-pthread-sync`, `kernel-wall-clock`, `kernel-safepoint-signal`: primitiva em modo kernel primeiro, exposição via *syscall* depois). Esta change prova o mecanismo com um demo em modo kernel (mesmo padrão de `storage_demo`/`vmm_demo`), lendo um arquivo real da própria partição de boot.
- **BREAKING**: nenhuma — puramente aditivo, novos módulos, nenhuma mudança em `kernel::storage`/`hal::BlockDevice`.

## Capabilities

### New Capabilities
- `kernel-filesystem`: montagem de um sistema de arquivos FAT32 (somente leitura, nomes curtos 8.3) sobre um `BlockDevice`, localizando a partição via a tabela GPT, e leitura de um arquivo inteiro dado seu caminho absoluto.

### Modified Capabilities
(nenhuma — `kernel-storage`/`hal::BlockDevice` não mudam; esta change só os consome)

## Impact

- **Código afetado**: novos módulos `kernel::gpt` (parsing da tabela de partição GPT — lógica pura testável em host, separada da leitura de blocos real) e `kernel::fat32` (parsing do BPB/*boot sector*, entradas de diretório 8.3 e cadeia de clusters via FAT — mesma separação pura/real já usada por `kernel::vmm`/`kernel::phys`); um novo demo em modo kernel (`kernel::fat32_demo` ou nome equivalente) lendo um arquivo real da partição de boot, análogo a `kernel::storage_demo`.
- **Specs**: nova capability `kernel-filesystem`. `kernel-storage`/`kernel-pci` não mudam.
- **Dependências**: reaproveita `kernel::storage::read_block`/`hal::BlockDevice` (já existentes) — nenhuma dependência nova.
- **Compatibilidade**: puramente aditivo. Nenhum comportamento existente muda; `kernel::storage` continua idêntico.
