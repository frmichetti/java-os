# kernel-acpi Specification

## Purpose

Localiza e interpreta as tabelas ACPI entregues pelo firmware/bootloader
(RSDP, RSDT/XSDT, MADT), para que o kernel descubra a topologia real de
interrupção do hardware em vez de assumir os valores fixos corretos só para
QEMU/VirtualBox/VMware.

## Requirements

### Requirement: Localização e Validação do RSDP
O kernel SHALL localizar o RSDP (`Root System Description Pointer`) entregue
pelo bootloader, e SHALL validar sua soma de verificação antes de confiar em
qualquer campo dele.

#### Scenario: RSDP com soma de verificação válida é aceito
- **WHEN** o bootloader entrega um RSDP cuja soma de verificação (dos
  primeiros 20 bytes, e também dos 36 bytes da estrutura estendida quando a
  revisão indicar ACPI 2.0 ou posterior) bate
- **THEN** o kernel SHALL aceitar esse RSDP como válido, disponibilizando
  seus campos para localizar a RSDT/XSDT

#### Scenario: RSDP ausente ou com soma de verificação inválida é rejeitado sem travar o boot
- **WHEN** o bootloader não entrega nenhum RSDP, ou entrega um cuja soma de
  verificação não bate
- **THEN** o kernel SHALL prosseguir o boot normalmente, sem nenhuma
  informação de topologia vinda de ACPI disponível

### Requirement: Localização e Validação da Tabela MADT
A partir de um RSDP válido, o kernel SHALL localizar a tabela MADT
(`Multiple APIC Description Table`, assinatura `"APIC"`) através da RSDT
(revisão ACPI 1.0) ou XSDT (revisão ACPI 2.0 ou posterior, preferida quando
disponível), e SHALL validar a soma de verificação da MADT (e da própria
RSDT/XSDT) antes de confiar em qualquer entrada dela.

#### Scenario: MADT com soma de verificação válida é aceita
- **WHEN** a RSDT/XSDT referencia uma tabela com assinatura `"APIC"` cuja
  soma de verificação (de todos os seus bytes, cabeçalho incluído) bate
- **THEN** o kernel SHALL aceitar essa MADT como válida, disponibilizando
  suas entradas para consulta

#### Scenario: MADT ausente ou inválida é rejeitada sem travar o boot
- **WHEN** a RSDT/XSDT válida não contém nenhuma tabela com assinatura
  `"APIC"`, ou a soma de verificação da RSDT/XSDT ou da MADT não bate
- **THEN** o kernel SHALL prosseguir o boot normalmente, sem nenhuma
  informação de topologia vinda de ACPI disponível

### Requirement: Endereço do Primeiro I/O APIC Declarado na MADT
Quando uma MADT válida contém ao menos uma entrada de I/O APIC (tipo 1), o
kernel SHALL disponibilizar o endereço MMIO físico declarado na primeira
dessas entradas.

#### Scenario: Endereço de I/O APIC é extraído da primeira entrada correspondente
- **WHEN** uma MADT válida contém uma ou mais entradas de tipo 1 (I/O APIC)
- **THEN** o kernel SHALL disponibilizar o endereço MMIO físico declarado na
  primeira dessas entradas, na ordem em que aparecem na tabela

#### Scenario: Ausência de entrada de I/O APIC na MADT é reportada como indisponível
- **WHEN** uma MADT válida não contém nenhuma entrada de tipo 1 (I/O APIC)
- **THEN** o kernel SHALL reportar que nenhum endereço de I/O APIC foi
  descoberto via ACPI, sem travar o boot

### Requirement: Resolução do GSI Real de uma IRQ ISA Legada
Dada uma IRQ ISA legada (0-15), o kernel SHALL devolver o `GSI` (`Global
System Interrupt`) real ao qual ela está conectada na tabela de redireção
do I/O APIC — o próprio número da IRQ quando a MADT não declara nenhuma
substituição para ela, ou o `GSI` declarado por uma substituição de origem
de interrupção (tipo 2, "Interrupt Source Override") que a referencie,
quando existir.

#### Scenario: IRQ sem substituição declarada mapeia para o GSI de mesmo número
- **WHEN** uma MADT válida (ou nenhuma MADT disponível) não declara nenhuma
  substituição de origem de interrupção para a IRQ ISA consultada
- **THEN** o kernel SHALL devolver o `GSI` de mesmo número da IRQ consultada

#### Scenario: IRQ com substituição declarada mapeia para o GSI substituto
- **WHEN** uma MADT válida declara uma substituição de origem de interrupção
  (tipo 2) para a IRQ ISA consultada, com um `GSI` de destino diferente
- **THEN** o kernel SHALL devolver o `GSI` declarado por essa substituição,
  não o número original da IRQ

### Requirement: Ausência ou Invalidez de Dados ACPI Nunca Compromete o Boot
Nenhuma falha de localização ou validação de uma tabela ACPI (RSDP ausente,
soma de verificação inválida em qualquer tabela, tabela MADT malformada)
SHALL travar, reiniciar, ou interromper o boot do kernel — apenas a
informação de topologia correspondente fica indisponível.

#### Scenario: Qualquer falha de ACPI deixa o restante do boot inalterado
- **WHEN** qualquer uma das falhas acima ocorre durante a localização ou
  validação das tabelas ACPI
- **THEN** o kernel SHALL continuar o boot normalmente a partir desse ponto,
  com um diagnóstico reportado no console, mas sem pânico nem travamento
