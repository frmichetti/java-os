# Spec Delta

## ADDED Requirements

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
