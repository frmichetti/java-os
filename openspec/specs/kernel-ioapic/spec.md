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
