# Design

## Context

`kernel::pci` (de `kernel-storage`) já enumera o barramento 0 e lê configuração de qualquer função PCI — uma placa de rede é descoberta do mesmo jeito que o controlador AHCI foi (classe/subclasse/prog-if, BARs). `kernel::paging::Mapper`/`kernel::hhdm` já são usados (desde `kernel-keyboard`, repetido em `kernel-storage`) para mapear BARs MMIO fora da HHDM garantida do Limine. Pesquisa feita antes deste design (via `info pci` do monitor do QEMU, num boot real sem nenhuma flag de rede): a configuração atual do projeto (`-M q35`, sem `-netdev`/`-device`/`-net`) já expõe uma placa Ethernet por padrão — vendor `0x8086` (Intel), device `0x10d3` (família 82574L/e1000e) —, mas esse padrão não é fixado por nenhuma flag explícita, nem é o modelo mais simples/documentado para um primeiro driver. Ver `proposal.md` — Why/What Changes.

## Goals / Non-Goals

**Goals:**
- Fixar explicitamente, via `xtask`, um modelo de placa de rede bem documentado (Intel 82540EM, `-device e1000` do QEMU), em vez de depender do padrão do QEMU.
- Um driver mínimo funcional (`kernel::network`): inicialização dos anéis de descritores de transmissão/recepção, envio de um quadro, consulta de quadros recebidos, leitura do endereço MAC.
- Uma abstração de HAL (`NetworkDevice`) análoga a `BlockDevice`/`KeyboardDevice` já existentes.
- `kernel::pci` ganha escrita de configuração (necessária para Bus Mastering).

**Non-Goals (nesta change):**
- Qualquer pilha de protocolo — ARP, IP, TCP/UDP, sockets, DHCP. Esta change entrega só o envio/recepção de quadros Ethernet brutos (sequências de bytes), igual a como `kernel-storage` entregou só blocos brutos, sem sistema de arquivos.
- Múltiplas placas de rede — só a primeira encontrada na enumeração.
- E/S orientada a interrupção — mesma decisão já tomada em `kernel-storage`: só *polling*. Reforça a mesma lição aprendida em `kernel-keyboard` (uma IRQ externa de baixa frequência competindo com o timer do escalonador a 100 Hz mostrou uma limitação real de timing do QEMU/TCG) — evitar essa classe de problema inteiramente nesta change, já que nem envio nem recepção de um quadro de teste dependem de reagir a um evento assíncrono em tempo real.
- Negociação de velocidade/duplex, múltiplas filas, *checksum offloading*, VLAN — recursos reais do 82540EM fora do necessário para provar o mecanismo.
- Qualquer outro modelo de placa (realtek, virtio-net, a e1000e exposta por padrão) — ver Decisão abaixo.

## Decisions

### Fixar o modelo da placa via `xtask` (`-device e1000`), não depender do padrão do QEMU
A configuração atual do projeto não passa nenhuma flag de rede ao QEMU, mas o QEMU ainda assim expõe uma placa por padrão (confirmado via `info pci` em boot real: vendor `0x8086`, device `0x10d3`, família 82574L/e1000e). Diferente do controlador AHCI do disco de boot (`kernel-storage`) — que já vem por padrão no chipset Q35 de um jeito estável entre versões do QEMU, porque é o próprio controlador de disco da VM —, uma placa de rede "de brinde" é um detalhe de implementação do QEMU que pode mudar entre versões, e o modelo 82574L/e1000e tem um layout de registradores mais novo e menos documentado em tutoriais de OS-dev do que o clássico Intel 82540EM (o modelo `e1000` do QEMU). Por isso, `xtask` passa a adicionar explicitamente `-netdev user,id=n0 -device e1000,netdev=n0` — `-netdev user` é o backend de rede do próprio QEMU baseado em SLIRP (NAT em espaço de usuário), que não exige nenhuma configuração ou privilégio especial do host (ao contrário de uma interface TAP), igual ao que já acontecia "de graça" com o padrão atual do QEMU.

**Alternativa descartada**: usar o modelo padrão do QEMU (82574L/e1000e) sem nenhuma flag nova. Rejeitada por não ser uma escolha deliberada e documentada (poderia mudar silenciosamente numa atualização do QEMU) e por ter um layout de registradores menos simples para uma primeira implementação.

**Alternativa descartada**: `virtio-net`. Mais eficiente e mais simples em alguns aspectos (fila *virtio* padronizada), mas exige implementar o protocolo *virtio* genérico (negociação de *features*, *virtqueues*) antes mesmo de chegar à parte específica de rede — complexidade desproporcional para o primeiro driver de rede do projeto, e não documentado em hardware real (só existe em VMs), ao contrário do e1000 (chip real, amplamente documentado, existe em hardware de verdade).

### PCI: habilitar Bus Mastering explicitamente antes de qualquer DMA
O registrador de comando PCI (offset 0x04 do espaço de configuração) tem bits para habilitar acesso a espaço de memória (bit 1) e **Bus Mastering** (bit 2 — permite ao dispositivo iniciar transferências DMA). O controlador AHCI do disco de boot já estava em uso pelo firmware (UEFI) antes do kernel assumir controle, então esses bits já vinham habilitados; uma placa de rede sem nenhum uso anterior não tem essa garantia. `kernel::pci` ganha `write_config_dword` (até agora só havia leitura) para que `kernel::network::init` leia o registrador de comando, ligue os bits necessários, e escreva de volta, antes de programar qualquer endereço de descritor na placa.

**Alternativa descartada**: assumir que Bus Mastering já vem habilitado (como aconteceu, por coincidência de uso prévio pelo firmware, com o AHCI). Rejeitada por ser uma suposição frágil espec��fica deste ambiente de teste — sem Bus Mastering, a placa simplesmente não consegue ler/escrever os anéis de descritores na RAM, uma falha silenciosa e difícil de diagnosticar (descritores nunca processados, sem nenhum erro reportado pelo registrador de status).

### Driver e1000: um anel de recepção e um de transmissão, formato legado de descritor, *polling*
Depois de localizar a placa via `kernel::pci` e mapear seu `BAR0` (registradores MMIO — mesmo tratamento de mapeamento explícito já usado para o I/O APIC e o `ABAR` da AHCI), a inicialização segue a sequência documentada para o 82540EM: reset do dispositivo (`CTRL.RST`), configurar os anéis de descritores de recepção (`RDBAL`/`RDBAH`/`RDLEN`/`RDH`/`RDT`) e transmissão (`TDBAL`/`TDBAH`/`TDLEN`/`TDH`/`TDT`) — cada anel numa página própria (mesma lógica de "uma página por estrutura" já usada na AHCI), com um pequeno número de descritores (8, suficiente para esta primeira versão) cada um apontando para um buffer de página própria —, habilitar o link (`CTRL.SLU`) e os motores de recepção/transmissão (`RCTL.EN`/`TCTL.EN`). Enviar um quadro significa copiar seus bytes para o buffer do próximo descritor de transmissão livre, marcar o descritor (comprimento, flags `EOP`/`RS`), avançar `TDT`, e esperar (*polling*) o bit de conclusão (`DD`) no próprio descritor. Receber significa verificar periodicamente se o descritor apontado por `RDH`/próximo a processar tem o bit `DD` ligado; se sim, copiar o conteúdo do buffer e devolver o descritor à placa (avançando `RDT`).

**Alternativa descartada**: múltiplas filas ou descritores estendidos (recursos mais recentes do hardware real 82540EM). Desnecessário para provar o mecanismo com um único quadro de teste.

### `NetworkDevice`: `send_frame`/`poll_frame`, endereço MAC exposto
```rust
pub trait NetworkDevice {
    fn mac_address(&self) -> [u8; 6];
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), NetworkError>;
    fn poll_frame(&mut self, buf: &mut [u8]) -> Option<usize>;
}
```
`poll_frame` não-bloqueante (mesmo espírito de `KeyboardDevice::poll_event`): devolve `None` se não há quadro pendente, ou `Some(tamanho)` com o quadro copiado para `buf` (até `buf.len()` bytes). `send_frame` aceita quadros de tamanho variável (Ethernet varia de 64 a 1518 bytes), ao contrário de `BlockDevice` (blocos de tamanho fixo).

**Alternativa descartada**: uma API bloqueante para `poll_frame` (espera até haver um quadro). Rejeitada pela mesma razão de `KeyboardDevice`: o kernel ainda não tem nenhuma primitiva de bloqueio/espera de thread.

## Riscos / Trade-offs

- **[Risco] O modelo padrão de NIC do QEMU pode mudar em versões futuras, mas a flag `-device e1000` agora é explícita e fixa** → Mitigado pela própria decisão desta change (ver "Fixar o modelo da placa").
- **[Risco] Backend `-netdev user` (SLIRP) tem limitações conhecidas (sem suporte a alguns protocolos, desempenho menor que TAP)** → Aceitável: esta change só precisa provar que um quadro sai/chega pela placa, não desempenho nem compatibilidade de protocolo real.
- **[Risco] *Polling* de recepção pode perder quadros se a demonstração não consultar com frequência suficiente e o anel de recepção (só 8 descritores) encher** → Aceitável para a demonstração de um único quadro de teste; revisar se um consumidor real precisar de taxas de recepção mais altas.
- **[Trade-off] Um anel pequeno (8 descritores) e buffers de uma página cada, em vez de um alocador mais eficiente** → Mesma lógica já aceita em `kernel-storage`: o kernel não tem (nem precisa, nesta escala) de um alocador mais fino que páginas inteiras.

## Achados durante a implementação

- **Tudo funcionou de primeira, de novo, assim como `kernel-storage`**: igual ao driver AHCI (e ao contrário do teclado), o driver e1000 é inteiramente *polling* (nenhuma IRQ envolvida, por decisão explícita) — confirmado em boot real: a placa foi encontrada na primeira tentativa de enumeração (classe 0x02/subclasse 0x00), Bus Mastering habilitado com sucesso, a HBA/anéis inicializaram sem erro, o endereço MAC lido (`52:54:00:12:34:56`) é o prefixo padrão que o QEMU usa para placas `e1000`, e o quadro de teste foi aceito (bit `DD` do descritor de transmissão ligou) já na primeira execução.
- **Verificação adicional via captura de pacotes (tarefa 5.3)**: usando `-object filter-dump,id=f1,netdev=n0,file=<caminho>` para capturar o tráfego de saída da placa num arquivo pcap, inspecionado em hexadecimal, o quadro de teste foi encontrado exatamente como montado pelo kernel — `ff:ff:ff:ff:ff:ff` (destino broadcast), `52:54:00:12:34:56` (origem, idêntico ao MAC reportado pelo driver), `0x88b5` (EtherType de teste), seguido de preenchimento zerado até 64 bytes. Essa é uma confirmação mais forte do que só o bit `DD` do descritor: prova que os bytes exatos que o kernel construiu realmente saíram da placa pela rede emulada, não só que a placa marcou "concluído" internamente. (O capture também mostrou tráfego IPv6 de vizinhança gerado automaticamente pelo próprio backend SLIRP do QEMU ao ligar o link — esperado, sem relação com este driver.)
- **Prefixo de MAC confirma o modelo de placa esperado**: `52:54:00` é o OUI (prefixo de fabricante) que o QEMU usa para todas as interfaces de rede emuladas — mais uma confirmação (além da classe/subclasse PCI) de que a placa encontrada é de fato a `e1000` fixada via `xtask`, não a placa 82574L/e1000e que existia por padrão antes desta change.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: `cargo test -p kernel --lib` continua passando sem regressão (os novos testes de host cobrem só lógica pura — montagem de descritores, validação de tamanho de quadro), `cargo xtask test` (smoke test serial) continua passando sem regressão de comportamento esperado no console serial (a nova flag de rede não deve alterar nada do que o smoke test já verifica), e uma verificação manual real em QEMU confirma que a placa é encontrada, inicializada, e que o envio de um quadro de teste é aceito (bit `DD` do descritor de transmissão liga). Rollback é reverter os commits desta change (incluindo a flag nova em `xtask`); nenhum dado persistente é afetado.
