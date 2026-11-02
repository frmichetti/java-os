# kernel-boot Specification

## Purpose

Define o comportamento de boot observável do kernel do Java OS: como ele sobe sob UEFI via Limine em x86_64, quais informações de boot e memória precisa expor, como deve se comportar ao final dessa sequência, e como esse comportamento é verificado automaticamente.

## Requirements

### Requirement: Boot UEFI via Limine
O kernel SHALL inicializar com sucesso em sistemas x86_64 com firmware UEFI, usando o protocolo de boot Limine, sem exigir suporte a Legacy BIOS/CSM.

#### Scenario: Boot bem-sucedido no QEMU com OVMF
- **WHEN** a imagem do kernel é iniciada no QEMU configurado com firmware OVMF (UEFI)
- **THEN** o controle chega ao ponto de entrada do kernel e ele começa a executar sua sequência de boot sem triple fault, reset ou travamento antes de reportar qualquer saída

#### Scenario: Incompatibilidade de protocolo de boot é detectada
- **WHEN** a revisão do protocolo base Limine reportada pelo bootloader não é suportada pelo kernel
- **THEN** o kernel SHALL recusar a continuidade do boot normal e SHALL reportar a incompatibilidade em vez de prosseguir com comportamento indefinido

### Requirement: Relato de Informações de Boot
Após a entrada, o kernel SHALL reportar informações de identificação de boot através de um canal de saída visível antes de realizar qualquer outro trabalho observável pelo usuário.

#### Scenario: Banner de boot é emitido
- **WHEN** o kernel atinge o estágio de relato de boot
- **THEN** ele SHALL emitir, no console serial, no mínimo: o identificador de nome/versão do kernel, confirmação de que o boot teve sucesso, e o nome do bootloader e a revisão de protocolo reportados pelo Limine

### Requirement: Relato do Mapa de Memória
O kernel SHALL obter o mapa de memória do sistema a partir do bootloader e reportá-lo em formato legível por humanos.

#### Scenario: Mapa de memória é exibido no boot
- **WHEN** o kernel recebe o mapa de memória a partir do boot request de memory map do Limine
- **THEN** ele SHALL imprimir, para cada região de memória, endereço base, tamanho e tipo (ex.: usável, reservada, reclamável pelo bootloader, framebuffer), e SHALL imprimir o total de memória usável em bytes

#### Scenario: Mapa de memória indisponível
- **WHEN** o bootloader não fornece uma resposta de mapa de memória
- **THEN** o kernel SHALL reportar a ausência explicitamente, em vez de imprimir valores fabricados ou zerados

### Requirement: Comportamento Definido de Halt
Ao final da sequência de relato de boot, ou caso ocorra um erro irrecuperável durante o boot, o kernel SHALL entrar em um estado de halt definido e não destrutivo, em vez de travar silenciosamente, reiniciar, ou continuar em execução indefinida.

#### Scenario: Conclusão normal termina em halt limpo
- **WHEN** o kernel termina de reportar as informações de boot e memória sem erros
- **THEN** ele SHALL entrar em um loop de halt ocioso (interrupções desabilitadas ou thread parada) e SHALL NOT reiniciar, reconfigurar ou retornar do seu ponto de entrada

#### Scenario: Panic termina em halt limpo com diagnóstico
- **WHEN** o handler de panic do kernel é invocado por qualquer motivo durante o boot
- **THEN** ele SHALL imprimir a mensagem de panic e sua localização no console serial e então entrar no mesmo estado de halt definido, sem disparar reinicialização ou triple fault

### Requirement: Verificação Automatizada de Boot
O projeto SHALL fornecer um teste automatizado e não-interativo que inicializa a imagem do kernel em um emulador e verifica se a sequência de boot teve sucesso, executável tanto localmente quanto em CI.

#### Scenario: Smoke test de boot passa em um boot bem-sucedido
- **WHEN** o teste automatizado de boot executa o kernel em QEMU headless com OVMF
- **THEN** ele SHALL capturar a saída serial, validar que o banner de boot esperado e os marcadores do mapa de memória estão presentes, e encerrar com status de sucesso

#### Scenario: Smoke test de boot falha em um boot quebrado
- **WHEN** o kernel não atinge o estágio de relato de boot (ex.: trava antes de qualquer saída, ou sofre crash) dentro de um timeout limitado
- **THEN** o teste automatizado de boot SHALL encerrar com status de falha, em vez de travar indefinidamente ou reportar sucesso falso
