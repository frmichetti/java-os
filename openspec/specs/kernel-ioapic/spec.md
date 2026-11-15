# kernel-ioapic Specification

## Purpose

Roteia interrupções de hardware externas (dispositivos como teclado, armazenamento e rede) para o Local APIC, já que o PIC 8259 legado está desabilitado e o Local APIC sozinho não recebe IRQs de dispositivos externos.

## Requirements

### Requirement: Roteamento de IRQ Externa para o Local APIC
O kernel SHALL rotear uma IRQ de hardware externa especificada para um vetor da IDT entregue ao Local APIC, de forma que a interrupção do dispositivo chegue à CPU.

#### Scenario: IRQ roteada é entregue ao vetor configurado
- **WHEN** um dispositivo externo cuja IRQ foi roteada para um vetor específico sinaliza sua interrupção
- **THEN** a CPU SHALL receber essa interrupção no vetor configurado, em vez de a interrupção ser perdida ou entregue a um vetor diferente

### Requirement: IRQ Não Roteada Não Chega à CPU
Uma IRQ de hardware externa que não tenha sido explicitamente roteada SHALL permanecer mascarada, para que dispositivos ainda não suportados não gerem interrupções em vetores não preparados para tratá-las.

#### Scenario: Dispositivo sem roteamento configurado não interrompe a CPU
- **WHEN** um dispositivo externo cuja IRQ nunca foi roteada pelo kernel sinaliza sua interrupção
- **THEN** essa interrupção SHALL permanecer mascarada e não SHALL chegar à CPU

### Requirement: Endereço do I/O APIC é Descoberto via ACPI Quando Disponível
O kernel SHALL usar o endereço MMIO do I/O APIC descoberto via ACPI/MADT
(`kernel-acpi`) quando essa informação estiver disponível, em vez de um
endereço fixo codificado — reservando o endereço fixo só como contingência.

#### Scenario: Endereço descoberto via ACPI é usado quando disponível
- **WHEN** o kernel descobre, via ACPI/MADT, o endereço MMIO de um I/O APIC
- **THEN** o kernel SHALL mapear e acessar o I/O APIC nesse endereço, não no
  endereço fixo de contingência

#### Scenario: Endereço fixo de contingência é usado quando ACPI está indisponível
- **WHEN** o kernel não descobre, via ACPI/MADT, nenhum endereço de I/O APIC
  (ACPI ausente, inválido, ou sem entrada de I/O APIC)
- **THEN** o kernel SHALL mapear e acessar o I/O APIC no mesmo endereço fixo
  já usado antes desta capacidade existir, sem nenhuma mudança de
  comportamento observável para as plataformas já suportadas

### Requirement: Roteamento de IRQ Legada Respeita Substituição Declarada via ACPI
Ao rotear uma IRQ ISA legada para um vetor, o kernel SHALL rotear o `GSI`
real resolvido via `kernel-acpi` (que já reflete qualquer substituição de
origem de interrupção declarada na MADT), não necessariamente o `GSI` de
mesmo número da IRQ.

#### Scenario: IRQ legada sem substituição continua roteada pelo mesmo número
- **WHEN** o kernel roteia uma IRQ ISA legada para a qual nenhuma
  substituição foi declarada
- **THEN** o roteamento SHALL usar o `GSI` de mesmo número da IRQ — nenhuma
  mudança de comportamento observável para as plataformas já suportadas

#### Scenario: IRQ legada com substituição é roteada pelo GSI substituto
- **WHEN** o kernel roteia uma IRQ ISA legada para a qual uma substituição
  de origem de interrupção foi declarada na MADT
- **THEN** o roteamento SHALL usar o `GSI` declarado por essa substituição,
  entregando a interrupção ao vetor configurado mesmo que o hardware
  real remapeie essa IRQ para uma entrada diferente da tabela de redireção
