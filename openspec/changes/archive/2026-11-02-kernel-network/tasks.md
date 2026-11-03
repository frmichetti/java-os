# Tasks

## 1. Pré-requisitos: escrita de configuração PCI e flag de rede no `xtask`

- [x] 1.1 Adicionar `kernel::pci::write_config_dword(bus, device, function, offset, value)` (reaproveita `config_address`, já existente, para escrever em `CONFIG_DATA` depois de selecionar o registrador em `CONFIG_ADDRESS`). Como depende de portas de I/O reais, verificar que `cargo kernel-build` compila; o comportamento real (Bus Mastering habilitado) é confirmado em boot na tarefa 5.3.
- [x] 1.2 Adicionar `-netdev user,id=n0 -device e1000,netdev=n0` à configuração do QEMU em `xtask` (tanto `run_qemu` quanto `run_test`), fixando o modelo da placa de rede (ver design.md). Verificar rodando `cargo xtask test`: o smoke test continua passando sem nenhuma mudança de comportamento esperado no console serial.

## 2. `kernel::network`: estruturas puras (descritor de transmissão, validação de quadro)

- [x] 2.1 Implementar como função pura a montagem dos 16 bytes de um descritor de transmissão legado do e1000 (endereço do buffer, comprimento, e os bits de comando `EOP`/`IFCS`/`RS` sempre ligados — ver design.md). Verificar com testes unitários em host: o endereço do buffer aparece nos bytes corretos (little-endian), o comprimento aparece nos bytes corretos, os bits de comando esperados estão ligados, e o byte de status (`STA`) começa zerado (ainda não processado pela placa).
- [x] 2.2 Implementar como função pura a validação do tamanho de um quadro Ethernet (64 a 1518 bytes — ver design.md). Verificar com testes unitários em host: um tamanho dentro do intervalo é aceito, tamanhos abaixo de 64 e acima de 1518 são rejeitados explicitamente.

## 3. `kernel::network`: driver real (inicialização da placa, anéis de descritores)

- [x] 3.1 Implementar a localização da placa via `kernel::pci::enumerate_bus0` (classe 0x02 "Network Controller", subclasse 0x00 "Ethernet"), habilitar Bus Mastering e espaço de memória no registrador de comando PCI (tarefa 1.1), mapear `BAR0` via `kernel::paging::Mapper` (mesmo tratamento do I/O APIC/`ABAR` já usado em changes anteriores), resetar o dispositivo (`CTRL.RST`) e ler o endereço MAC dos registradores `RAL0`/`RAH0`. Como depende de MMIO/configuração PCI reais, verificar que `cargo kernel-build` compila; a inicialização real é confirmada em boot na tarefa 5.3.
- [x] 3.2 Implementar a inicialização dos anéis de descritores de recepção e transmissão (8 descritores cada, um buffer de página própria por descritor — ver design.md): alocar as estruturas, programar `RDBAL`/`RDBAH`/`RDLEN`/`RDH`/`RDT` e `TDBAL`/`TDBAH`/`TDLEN`/`TDH`/`TDT`, e habilitar o link (`CTRL.SLU`) e os motores de recepção/transmissão (`RCTL.EN`/`TCTL.EN`). Como depende de MMIO real, verificar que `cargo kernel-build` compila; confirmado em boot na tarefa 5.3.

## 4. `kernel::hal::NetworkDevice` e inicialização de alto nível

- [x] 4.1 Adicionar `kernel::hal::NetworkDevice` (trait com `mac_address`, `send_frame`, `poll_frame`, ver design.md) e o tipo `kernel::network::E1000Nic` implementando-o: `send_frame` valida o tamanho (tarefa 2.2), monta o descritor (tarefa 2.1) no próximo slot livre do anel de transmissão, avança `TDT`, e espera (*polling*) o bit `DD`; `poll_frame` verifica o bit `DD` do próximo descritor de recepção a processar, copia o conteúdo se houver, e devolve o descritor à placa (avança `RDT`). Verificar que `cargo kernel-build` compila.
- [x] 4.2 Implementar `kernel::network::init(frame_allocator) -> Result<(), NetworkError>`: localiza a placa e a inicializa (tarefas 3.1/3.2), guarda o `E1000Nic` resultante num global atrás de `Spinlock` (mesmo padrão de `kernel::storage::DISK`). Trata explicitamente, sem travar o boot, a ausência de uma placa de rede na enumeração — mesmo espírito de "indisponibilidade tratada explicitamente" já usado em `kernel-framebuffer-console`/`kernel-storage`. Verificar que `cargo kernel-build` compila.

## 5. Integração: demonstração, inicialização em `kernel_main` e verificação de ponta a ponta

- [x] 5.1 Implementar `kernel::network_demo::run()` (operação única, análoga a `kernel::storage_demo`): monta um quadro Ethernet de teste simples (destino broadcast `ff:ff:ff:ff:ff:ff`, origem o MAC da própria placa, um EtherType de teste, preenchido até o tamanho mínimo de 64 bytes — sem implementar nenhum protocolo de verdade), envia via `kernel::network::send_frame`, e reporta pelos consoles existentes se o envio foi aceito. Verificar que `cargo kernel-build` compila.
- [x] 5.2 Em `continue_boot` (`kernel/src/main.rs`), depois do escalonador e do I/O APIC já estarem prontos: chamar `kernel::network::init(&mut frame_allocator)` e, se bem-sucedido, `kernel::network_demo::run()`; se `init` falhar (sem placa de rede encontrada), reportar pelo console serial e prosseguir o boot normalmente, sem travar. Verificar com `cargo xtask test` (já usando a flag de rede da tarefa 1.2) que o boot continua passando sem regressão no console serial.
- [x] 5.3 Validar em boot real (QEMU) que a placa é encontrada, inicializada (MAC reportado), e que o quadro de teste da tarefa 5.1 é aceito (bit `DD` do descritor de transmissão liga). Validar adicionalmente, se possível, usando a opção `-object filter-dump,id=f1,netdev=n0,file=<caminho>` do QEMU para capturar o tráfego de saída da placa num arquivo e confirmar que os bytes exatos do quadro de teste realmente saíram pela placa (documentar aqui o resultado). **Se a placa não for encontrada na enumeração, ou a inicialização falhar**, investigar antes de prosseguir (candidatos mais prováveis: classe/subclasse PCI diferentes do esperado para o modelo `e1000` do QEMU, Bus Mastering não realmente habilitado, ou `BAR0` mal mapeado — ver design.md) e documentar o achado real em design.md, como já é costume neste projeto.
- [x] 5.4 Confirmar `cargo test -p kernel --lib` (suíte completa de host) sem regressão, incluindo os novos testes das tarefas 2.1 e 2.2.
- [x] 5.5 Atualizar `docs/architecture/overview.md`: seção "5. HAL e Drivers" (PCI com escrita, e o driver de rede e1000 implementados) e o item 6 do roadmap, marcando-o como **concluído** (console, framebuffer, timer/APIC, I/O APIC, teclado, armazenamento e rede — todos os drivers essenciais previstos agora implementados). Verificar lendo o arquivo atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para as modificações propostas.
