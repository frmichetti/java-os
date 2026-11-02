# kernel-interrupts Specification

## Purpose

Fornece a IDT do kernel, os handlers de exceção de CPU e o tratamento padrão de interrupções de hardware, substituindo a ausência de qualquer IDT própria por uma infraestrutura de interrupções segura e diagnosticável.

## Requirements

### Requirement: Tratamento de Exceções de CPU
O kernel SHALL instalar handlers para as exceções de CPU essenciais (page fault, general protection fault, double fault) que reportam diagnóstico pelo canal de saída de boot e então entram no comportamento de halt já definido pela capability `kernel-boot`, em vez de permitir um triple fault.

#### Scenario: Page fault é diagnosticado em vez de causar triple fault
- **WHEN** o kernel acessa um endereço de memória não mapeado ou sem permissão suficiente, gerando uma exceção de page fault
- **THEN** o kernel SHALL reportar o vetor da exceção, o código de erro e o endereço que causou a falta, e então entrar em halt, em vez de sofrer um triple fault

#### Scenario: General protection fault é diagnosticado em vez de causar triple fault
- **WHEN** o kernel executa uma operação que viola a proteção do processador (ex.: acesso indevido a um segmento ou instrução privilegiada), gerando uma exceção de general protection fault
- **THEN** o kernel SHALL reportar o vetor da exceção e o código de erro, e então entrar em halt, em vez de sofrer um triple fault

### Requirement: Double Fault com Stack Dedicada
O handler de double fault SHALL executar em uma stack própria (independente da stack corrente do kernel), para continuar funcionando mesmo quando a causa do double fault for um estouro da stack do kernel.

#### Scenario: Double fault por estouro de stack ainda é diagnosticado
- **WHEN** um estouro da stack do kernel provoca uma exceção de double fault
- **THEN** o handler de double fault SHALL executar normalmente (usando sua própria stack dedicada) e reportar o diagnóstico antes de entrar em halt, em vez de resultar em um triple fault por falha ao entregar a exceção

### Requirement: Page Fault com Stack Dedicada
O handler de page fault SHALL executar em uma stack própria (independente da stack corrente do kernel), para continuar sendo entregue de forma confiável mesmo quando a causa do page fault for um estouro da stack do kernel.

#### Scenario: Page fault causado por estouro de stack ainda é entregue ao handler
- **WHEN** um estouro da stack do kernel provoca uma exceção de page fault
- **THEN** a CPU SHALL conseguir entregar a exceção ao handler de page fault (usando sua própria stack dedicada), que reporta o diagnóstico normalmente, em vez de a falha escalar para double fault por a CPU não conseguir empilhar o frame de exceção na stack corrente (já invadida pela guard page)

### Requirement: Tratamento Padrão de Interrupções de Hardware
Qualquer interrupção de hardware que chegue a um vetor sem handler específico registrado SHALL ser reconhecida por um handler padrão, em vez de deixar o kernel vulnerável a desviar a execução para código obsoleto do firmware/bootloader.

#### Scenario: Interrupção de hardware sem handler específico não desvia a execução
- **WHEN** uma interrupção de hardware é entregue a um vetor da IDT sem handler específico registrado
- **THEN** o handler padrão SHALL reconhecer a interrupção e retornar o controle ao fluxo de execução do kernel no ponto em que foi interrompido, sem desviar para qualquer código fora do próprio kernel
