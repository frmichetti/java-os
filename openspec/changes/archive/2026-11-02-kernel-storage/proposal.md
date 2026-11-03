# Proposal

## Why

O roadmap do projeto (`docs/architecture/overview.md`, seção 5) prevê armazenamento como o próximo driver essencial depois do teclado (`kernel-keyboard`, concluída). Hoje o kernel não consegue ler nenhum dado de disco — tudo que existe até agora é memória RAM e os dois canais de diagnóstico (serial, framebuffer) mais a entrada de teclado. Sem armazenamento, não há como carregar programas, dados, nem (futuramente) o runtime da JVM a partir de um disco real; o único "programa" executável hoje é um binário embutido no próprio kernel (`kernel::user_demo`).

Investigação do código existente mostrou que, assim como o teclado (um dispositivo externo com IRQ e MMIO fora da HHDM garantida), um controlador de armazenamento moderno (AHCI/NVMe) é um dispositivo **PCI**, descoberto por enumeração do barramento — o kernel não tem hoje nenhum código de acesso a configuração PCI. Essa descoberta via PCI é, portanto, um pré-requisito novo desta change, análogo ao que o I/O APIC foi para o teclado.

## What Changes

- Novo módulo `kernel::pci`: acesso à configuração PCI via o mecanismo legado de portas 0xCF8/0xCFC (Configuration Mechanism #1, suportado por toda CPU/chipset x86 relevante, incluindo QEMU) e enumeração do barramento 0 (suficiente para encontrar o controlador AHCI do chipset ICH9 do QEMU/Q35 — ver design.md), devolvendo vendor/device ID, classe/subclasse/prog-if e os BARs de cada função encontrada.
- Novo módulo `kernel::storage`: driver de bloco AHCI (SATA) — inicialização da HBA (Host Bus Adapter) encontrada via `kernel::pci`, inicialização da primeira porta implementada com um dispositivo SATA conectado, e leitura/escrita de setores por LBA via comandos ATA (`READ DMA EXT`/`WRITE DMA EXT`) emitidos através da fila de comandos da AHCI, com espera (*polling*) pela conclusão — sem interrupção dedicada nesta primeira versão (ver design.md).
- Nova abstração de HAL: `kernel::hal::BlockDevice`, implementada pelo driver AHCI — um contrato mínimo para "ler e escrever blocos de tamanho fixo por índice (LBA)", independente do controlador físico, análogo ao `KeyboardDevice` já existente (entrada) e ao `Console` (saída).
- Uma pequena demonstração de verificação (análoga às anteriores): lê o primeiro setor do disco de boot (que já contém a tabela de partição GPT e a partição EFI usada pelo próprio boot) e reporta pelos consoles existentes um resumo do que foi lido (ex.: a assinatura de boot sector 0x55AA), provando o mecanismo ponta a ponta sem depender de nenhum sistema de arquivos.

## Capabilities

### New Capabilities
- `kernel-pci`: acesso à configuração PCI (leitura de registradores, enumeração de barramento) — pré-requisito para qualquer dispositivo PCI futuro (armazenamento nesta change; rede em uma change futura).
- `kernel-storage`: driver de armazenamento AHCI — inicialização da HBA/porta e leitura/escrita de setores por LBA.

### Modified Capabilities
- `kernel-hal`: adiciona a abstração `BlockDevice` (armazenamento em blocos), ao lado de `Console` e `KeyboardDevice` já existentes — sem alterar os requisitos existentes.

## Impact

- Código novo: `kernel/src/pci.rs`, `kernel/src/storage.rs`; `kernel/src/hal.rs` ganha um novo trait.
- `kernel/src/port.rs`: ganha `inl`/`outl` (operações de 32 bits), exigidas pelo acesso à configuração PCI (0xCF8/0xCFC) — as primitivas de 8 bits existentes (`inb`/`outb`) não bastam.
- `kernel/src/main.rs`/`continue_boot`: nova chamada de inicialização (depois do escalonador e do I/O APIC já prontos, mesma ordem de dependências já estabelecida).
- Fora de escopo nesta change (ver design.md para detalhes e justificativa): NVMe, múltiplos discos/portas, enfileiramento de múltiplos comandos simultâneos (NCQ), E/S orientada a interrupção (fica só *polling* por ora, mesmo já existindo `kernel::ioapic`), e qualquer sistema de arquivos — esta change entrega só o driver de bloco bruto (ler/escrever setores por LBA).
- Nenhuma mudança de comportamento em `kernel-interrupts`, `kernel-timer`, `kernel-syscalls`, `kernel-framebuffer-console` ou `kernel-keyboard` — só adições.
- `docs/architecture/overview.md`: seção 5 (HAL e Drivers) e item 6 do roadmap atualizados ao final da implementação, como de costume.
