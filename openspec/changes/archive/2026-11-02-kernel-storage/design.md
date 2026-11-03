# Design

## Context

O kernel não tem hoje nenhum código de acesso a PCI: nenhuma leitura de configuração, nenhuma enumeração de barramento. Isso é necessário porque um controlador de armazenamento moderno (AHCI ou NVMe) é um dispositivo PCI — ao contrário do I/O APIC (`kernel-keyboard`), que vive num endereço MMIO fixo conhecido de antemão, o endereço do controlador de armazenamento só é conhecido depois de enumerar o barramento PCI e ler seus registradores de configuração (incluindo os BARs, que apontam para a região MMIO real do dispositivo). `kernel::port` já expõe `inb`/`outb` (8 bits); o acesso à configuração PCI via portas 0xCF8/0xCFC exige operações de 32 bits, ainda não existentes. `kernel::paging::Mapper` e `kernel::hhdm` já são usados (desde `kernel-keyboard`) para mapear páginas MMIO físicas arbitrárias fora da HHDM garantida do Limine — o mesmo padrão se aplica aqui, já que a HHDM só cobre memória usável/reclamável, não o BAR de um dispositivo PCI. A imagem de disco usada pelo próprio boot (`xtask`, `-M q35 -drive file=...,if=ide`) já é servida pelo controlador AHCI (`ich9-ahci`) embutido no chipset Q35 do QEMU — o Q35 não tem controlador IDE legado, então `if=ide` nessa máquina já é atendido via AHCI sem nenhuma mudança na configuração de boot existente. Ver `proposal.md` — Why/What Changes para a motivação completa.

## Goals / Non-Goals

**Goals:**
- Um driver mínimo de acesso a configuração PCI (`kernel::pci`): leitura de registradores de configuração e enumeração do barramento 0, suficiente para localizar o controlador AHCI do chipset do QEMU.
- Um driver de armazenamento AHCI (`kernel::storage`) funcional: inicialização da HBA e de uma porta com dispositivo SATA conectado, leitura e escrita de setores por LBA.
- Uma abstração de HAL (`BlockDevice`) análoga a `Console`/`KeyboardDevice` já existentes.
- Uma demonstração mínima provando o mecanismo ponta a ponta, sem sistema de arquivos.

**Non-Goals (nesta change):**
- NVMe — fica para uma change futura caso seja necessário (a escolha por AHCI nesta change está documentada abaixo).
- Múltiplos discos, múltiplas portas AHCI simultâneas, ou qualquer dispositivo PCI além do controlador de armazenamento (rede fica para uma change futura, reaproveitando `kernel::pci`).
- NCQ (Native Command Queuing) — só um comando pendente por vez, emitido sempre no slot de comando 0.
- E/S orientada a interrupção — mesmo já existindo `kernel::ioapic`, esta primeira versão usa *polling* (espera ativa) pela conclusão do comando, mais simples de implementar e verificar corretamente numa primeira passada; revisar quando houver necessidade real de não bloquear a CPU durante E/S.
- Enumeração de múltiplos barramentos PCI via *bridges* — só o barramento 0 é varrido, suficiente para o chipset ICH9 do QEMU/Q35 (onde o controlador AHCI já vive no barramento 0).
- BIOS/OS handoff da AHCI (registrador `BOHC`) — o QEMU não implementa posse por BIOS para o controlador AHCI emulado; pular esse handshake não tem efeito observável no alvo atual.
- Qualquer sistema de arquivos (FAT32, ext2, etc.) — fica para uma change futura, consumindo o `BlockDevice` desta.

## Decisions

### AHCI, não NVMe, como primeiro controlador de armazenamento
A imagem de disco de boot já é servida via AHCI pelo chipset Q35 do QEMU (ver Context) — nenhuma mudança na configuração de `xtask`/`boot/limine.conf` é necessária, e o próprio disco de boot já serve como alvo de verificação real (ele contém a tabela de partição GPT e a partição EFI usada pelo boot, com a assinatura `0x55AA` num offset conhecido do primeiro setor). AHCI também é significativamente mais documentado em fontes de referência de OS-dev do que NVMe para uma primeira implementação mínima (fila de comandos mais simples: um único *command list* de 32 slots por porta, sem o conceito de múltiplas filas de submissão/conclusão independentes do NVMe).

**Alternativa descartada**: NVMe. Mais moderno e mais relevante para SSDs reais, mas exige um protocolo de inicialização mais elaborado (fila administrativa separada das filas de E/S, registradores de *doorbell* próprios) para um ganho que não se aplica ao ambiente de teste atual (QEMU/Q35 expõe AHCI por padrão, não NVMe, sem configuração adicional). Revisar quando houver motivo real (ex.: hardware real com só NVMe).

### PCI: mecanismo legado de portas (0xCF8/0xCFC), enumeração só do barramento 0
Acesso à configuração PCI via o "Configuration Mechanism #1": escrever um endereço (barramento, dispositivo, função, offset) em `CONFIG_ADDRESS` (porta 0xCF8, 32 bits) e ler/escrever o valor em `CONFIG_DATA` (porta 0xCFC, 32 bits). Suportado por toda CPU/chipset x86 relevante, incluindo o QEMU — não exige descobrir o endereço MMIO do mecanismo mais novo (*PCI Express Enhanced Configuration Access Mechanism*, que por sua vez precisa ser descoberto via ACPI MCFG, uma dependência que este projeto ainda não tem). A enumeração varre só o barramento 0 (256 combinações de dispositivo×função), suficiente para encontrar o controlador AHCI do ICH9 do QEMU/Q35, que vive nesse barramento.

**Alternativa descartada**: PCI Express Enhanced Configuration Access Mechanism (ECAM), via MMIO. Mais moderno e necessário para acessar registradores de configuração estendidos (além dos primeiros 256 bytes), mas exige descobrir o endereço base do ECAM via a tabela ACPI MCFG — o kernel ainda não tem nenhum parsing de ACPI (mesma decisão já tomada em `kernel-keyboard` para o I/O APIC: evitar introduzir parsing de ACPI antes de precisar dele de verdade). O mecanismo legado de portas já dá acesso a tudo que esta change precisa (vendor/device ID, classe, BARs).

### AHCI: inicialização mínima de uma porta, um comando por vez, `READ`/`WRITE DMA EXT`
Depois de localizar a HBA via `kernel::pci` e mapear seu `ABAR` (BAR5, uma região MMIO — mesmo tratamento de mapeamento explícito via `kernel::paging::Mapper` já usado para o I/O APIC, já que a HHDM não garante cobri-lo), a inicialização segue a sequência padrão documentada para AHCI: parar o motor de comando da primeira porta implementada com um dispositivo SATA conectado (`PxSSTS`/`PxSIG`) se já estiver rodando, alocar (via o alocador de frames físicos já existente, uma página inteira por estrutura — o alinhamento de 4 KiB de uma página satisfaz os alinhamentos exigidos pela AHCI, de 1 KiB/256 B/128 B) a lista de comandos (32 slots), a área de recebimento de FIS, e a tabela de comando do slot 0, e então ligar o motor de comando (`PxCMD.FRE`, depois `PxCMD.ST`). Emitir um comando significa montar um FIS de registrador Host-to-Device (`READ DMA EXT` ou `WRITE DMA EXT`, LBA de 48 bits, contagem de 1 setor) no slot 0, apontar uma única entrada PRDT para o buffer de 512 bytes do chamador, ligar o bit correspondente em `PxCI`, e esperar (*polling*) até a CPU ver `PxCI` zerar esse bit (comando aceito e concluído) ou um timeout/erro em `PxTFD`/`PxIS`.

**Alternativa descartada**: usar todos os 32 slots de comando para permitir múltiplos comandos pendentes (NCQ). Rejeitada por ser significativamente mais complexa (requer rastrear qual slot está livre, lidar com conclusões fora de ordem) para um ganho que não existe ainda nesta fase (nenhum consumidor emite E/S concorrente).

### `BlockDevice`: `read_block`/`write_block` por LBA, tamanho de bloco fixo de 512 bytes
```rust
pub trait BlockDevice {
    fn block_size(&self) -> usize;
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError>;
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError>;
}
```
512 bytes é o tamanho de setor lógico padrão para discos emulados pelo QEMU (e a imensa maioria dos discos reais até a adoção de "Advanced Format" de 4096 bytes, fora de escopo aqui). `buf` deve ter exatamente `block_size()` bytes; um tamanho incorreto é um erro explícito (`BlockError`), nunca uma leitura/escrita parcial silenciosa.

**Alternativa descartada**: uma API que aceita qualquer `&[u8]`/offset de byte arbitrário (como um arquivo comum). Rejeitada por esconder a granularidade real do hardware (setores) atrás de uma abstração que pareceria suportar E/S não alinhada — um driver de bloco bruto deve deixar isso explícito; uma camada de conveniência por cima (ex.: um sistema de arquivos) pode ser construída depois.

## Riscos / Trade-offs

- **[Risco] Mecanismo legado de PCI (portas) não vê registradores de configuração estendidos (além do offset 256)** → Aceitável: tudo que esta change precisa (vendor/device ID, classe/subclasse/prog-if, BARs) vive nos primeiros 64 bytes do espaço de configuração, bem dentro do limite do mecanismo legado.
- **[Risco] Varrer só o barramento 0 não encontra dispositivos atrás de uma PCI bridge em hardware real variado** → Aceitável no alvo atual (QEMU/Q35, onde o controlador AHCI já vive no barramento 0); revisar junto do item 12 do roadmap (hardware real).
- **[Risco] *Polling* bloqueia a CPU durante toda a duração de uma leitura/escrita de disco (potencialmente muito mais lenta que um tick de timer)** → Aceitável nesta primeira versão (Non-Goal explícito de E/S por interrupção); nenhum consumidor atual depende de não bloquear durante E/S. Revisar quando houver um caso de uso real que precise disso (ex.: um escalonador com múltiplas threads fazendo E/S concorrente).
- **[Trade-off] Alocar uma página inteira (4 KiB) para cada estrutura AHCI (lista de comandos, FIS, tabela de comando), mesmo as menores exigindo só 256 ou 128 bytes** → Aceitável: o kernel não tem (nem precisa, nesta escala) de um alocador de memória física mais fino que páginas inteiras; o desperdício é desprezível (poucos KiB) e evita introduzir um alocador novo só para isto.

## Achados durante a implementação

- **Tudo funcionou de primeira, sem nenhum dos problemas de timing encontrados em `kernel-keyboard`**: ao contrário da IRQ1 do teclado (que exigiu uma investigação extensa sobre um artefato de timing do QEMU/TCG sob alta frequência de interrupção concorrente), o driver AHCI opera inteiramente por *polling* (nenhuma IRQ envolvida, por decisão explícita desta change) — confirmado em boot real (QEMU/TCG padrão, sem KVM): o controlador AHCI foi encontrado na primeira tentativa de enumeração (barramento 0, classe/subclasse/prog-if corretos), a HBA e a porta 0 inicializaram sem erro, e a leitura do LBA 0 confirmou a assinatura `0x55AA` corretamente já na primeira execução.
- **Verificação manual de escrita (tarefa 5.3)**: uma escrita seguida de leitura no **LBA 100** (dentro da área reservada entre a tabela GPT — LBAs 1-33 — e o início da partição EFI em LBA 2048, ver `xtask::PART_START_SECTOR` — garantidamente não usada por nenhum sistema de arquivos da imagem de boot) confirmou que o conteúdo lido de volta é byte a byte idêntico ao escrito. Testada temporariamente dentro de `kernel::storage_demo::run()` e removida do código final (a demonstração permanente é só leitura, ver design.md e tasks.md) — o mecanismo de escrita está, portanto, verificado, mesmo não fazendo parte do fluxo de boot repetido pelos testes automatizados.
- **Hipótese do `ABAR` como BAR de 32 bits confirmada**: o controlador AHCI do chipset ICH9 do QEMU/Q35 reporta o BAR5 como um endereço de 32 bits simples (sem o bit de "64-bit memory space" ligado) — a simplificação assumida em design.md (mascarar `bar & 0xFFFF_FFF0` e tratar como endereço completo) se mostrou correta sem ajustes.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: `cargo test -p kernel --lib` continua passando sem regressão (os novos testes de host cobrem só lógica pura — montagem de FIS/comando, construção do endereço de configuração PCI), `cargo xtask test` (smoke test serial) continua passando sem nenhuma mudança de comportamento esperado, e uma verificação manual real em QEMU confirma a leitura do primeiro setor do próprio disco de boot (incluindo a assinatura `0x55AA`) e uma escrita seguida de leitura num setor de teste fora da área usada pelo boot (para não corromper a imagem). Rollback é reverter os commits desta change; nenhum dado persistente é afetado.
