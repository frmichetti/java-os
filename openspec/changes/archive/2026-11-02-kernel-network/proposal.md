# Proposal

## Why

O roadmap do projeto (`docs/architecture/overview.md`, seção 5) prevê rede como o último item de "HAL e drivers essenciais" (item 6 do roadmap), depois de armazenamento (`kernel-storage`, concluída). Com console, teclado e disco já funcionando, rede é a peça que falta para o kernel trocar dados com o mundo exterior — pré-requisito eventual para qualquer coisa que dependa de rede mais adiante (atualização de software, serviços de rede da futura "Java OS Platform", item 10 do roadmap).

Pesquisa feita antes deste proposal: `cargo xtask run`/`cargo xtask test` não passam nenhuma flag `-netdev`/`-device`/`-net` ao QEMU, mas o monitor do QEMU (`info pci`, inspecionado num boot real) mostra que uma placa de rede **já existe por padrão** nesta configuração: um controlador Ethernet Intel (vendor `0x8086`, device `0x10d3` — família 82574L/e1000e). Esse modelo padrão não é garantido entre versões do QEMU nem é o mais documentado para um primeiro driver (a maioria dos tutoriais de OS-dev usa o Intel 82540EM, modelo `e1000` do QEMU, com um layout de registradores mais simples) — por isso esta change também fixa explicitamente o modelo via uma flag nova em `xtask`, em vez de depender do que o QEMU escolher por padrão (ver design.md).

## What Changes

- `xtask`: adiciona `-netdev user,id=n0 -device e1000,netdev=n0` à configuração do QEMU usada por `cargo xtask run`/`cargo xtask test`, fixando o modelo de placa de rede (Intel 82540EM, bem documentado) em vez de depender do padrão do QEMU.
- `kernel::pci` ganha a capacidade de **escrever** configuração (até agora só lia) — necessário para habilitar Bus Mastering no registrador de comando da placa de rede antes de qualquer DMA (descoberto durante a pesquisa deste proposal: ao contrário do controlador AHCI do disco de boot, já em uso pelo firmware antes do kernel assumir o controle, uma placa de rede sem nenhum tráfego anterior não tem garantia de já estar com Bus Mastering habilitado).
- Novo módulo `kernel::network`: driver da placa Intel 82540EM (`e1000`) — localizado via `kernel::pci`, com Bus Mastering habilitado explicitamente, inicialização dos anéis de descritores de transmissão e recepção, envio de um quadro Ethernet bruto, e consulta (*polling*, mesma decisão já tomada em `kernel-storage` para armazenamento) de quadros recebidos.
- Nova abstração de HAL: `kernel::hal::NetworkDevice`, implementada pelo driver — um contrato mínimo para "enviar um quadro" e "consultar o próximo quadro recebido", independente do controlador físico, análogo a `BlockDevice`/`KeyboardDevice` já existentes.
- Uma pequena demonstração de verificação (análoga às anteriores): envia um quadro Ethernet de broadcast simples (ex.: um ARP request, sem implementar o protocolo ARP de verdade — só os bytes brutos de um pacote de teste conhecido) e reporta pelos consoles existentes se o envio foi aceito pela placa, provando o mecanismo ponta a ponta sem nenhuma pilha de protocolo.

## Capabilities

### New Capabilities
- `kernel-network`: driver de placa de rede Ethernet (Intel 82540EM/`e1000`) — envio e recepção de quadros brutos por índice de descritor, sem nenhuma pilha de protocolo (ARP/IP/TCP/UDP/sockets).

### Modified Capabilities
- `kernel-hal`: adiciona a abstração `NetworkDevice` (rede), ao lado de `Console`, `KeyboardDevice` e `BlockDevice` já existentes — sem alterar os requisitos existentes.
- `kernel-pci`: adiciona a capacidade de escrever um registrador de configuração de um dispositivo já localizado (necessária para habilitar Bus Mastering) — sem alterar os requisitos de enumeração/leitura já existentes.

## Impact

- Código novo: `kernel/src/network.rs`; `kernel/src/hal.rs` ganha um novo trait; `kernel/src/pci.rs` ganha uma função de escrita de configuração.
- `xtask/src/main.rs`: `-netdev`/`-device e1000` adicionados à configuração do QEMU usada tanto por `run` quanto por `test` — primeira mudança de configuração de QEMU desde que o projeto começou (todas as changes anteriores reaproveitaram dispositivos já presentes por padrão na configuração existente).
- `kernel/src/main.rs`/`continue_boot`: nova chamada de inicialização (depois do escalonador e do I/O APIC já prontos, mesma ordem de dependências já estabelecida).
- Fora de escopo nesta change (ver design.md para detalhes e justificativa): qualquer pilha de protocolo (ARP, IP, TCP/UDP, sockets, DHCP), múltiplas placas de rede, E/S orientada a interrupção (mesma decisão de `kernel-storage`: só *polling* nesta primeira versão).
- Nenhuma mudança de comportamento em `kernel-interrupts`, `kernel-timer`, `kernel-syscalls`, `kernel-framebuffer-console`, `kernel-keyboard`, `kernel-pci` ou `kernel-storage` — só adições.
- `docs/architecture/overview.md`: seção 5 (HAL e Drivers) e item 6 do roadmap atualizados ao final da implementação (item 6 fica completo depois desta change).
