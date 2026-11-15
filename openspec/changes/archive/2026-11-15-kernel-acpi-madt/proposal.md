# Proposal

## Why

Duas hipóteses de hardware fixas, documentadas desde `kernel-keyboard`/
`kernel-interrupts-timer` como "suficiente para os alvos atuais, sem parsing
de ACPI/MADT", bloqueiam o kernel de rodar em hardware real:

1. `kernel::ioapic::IOAPIC_BASE_PHYS = 0xFEC0_0000` — o endereço MMIO padrão
   de qualquer PC-compatível e o valor usado pelo I/O APIC emulado de QEMU/
   VirtualBox/VMware, mas não necessariamente o endereço real num chipset
   físico.
2. `kernel::keyboard`'s `route_irq(1, ...)` assume que a IRQ1 (teclado PS/2)
   mapeia 1:1 para a entrada 1 da tabela de redireção do I/O APIC (GSI1) —
   verdade nas três plataformas-alvo atuais, mas hardware real
   frequentemente remapeia IRQs legadas via uma entrada "Interrupt Source
   Override" da MADT.

Isso já é o próximo passo imediato documentado no `ROADMAP.md`
("Parsing de ACPI/MADT, para hardware real além de QEMU/VirtualBox/
VMware"). O Limine já entrega o ponteiro RSDP ao kernel (visível em
`docs/architecture/overview.md`, "o que o bootloader entrega"), mas o
kernel nunca pede essa informação (`kernel/src/main.rs` não declara
`limine::request::RsdpRequest`) nem faz nenhum parsing de tabela ACPI hoje.

## What Changes

- Nova capacidade `kernel::acpi`: localiza e valida o RSDP (via
  `RsdpRequest` do Limine), localiza a RSDT/XSDT a partir dele, localiza e
  valida a MADT (tabela `"APIC"`) a partir da RSDT/XSDT, e enumera suas
  entradas de interesse — endereço de um I/O APIC (tipo 1) e substituições
  de origem de interrupção legada (tipo 2, "Interrupt Source Override").
- `kernel::ioapic::init` passa a usar o endereço do I/O APIC descoberto pela
  MADT (quando disponível e válido), em vez do endereço fixo
  `0xFEC0_0000` — que continua sendo o valor de contingência (ver
  "Why não é uma mudança disruptiva" abaixo).
- `kernel::keyboard`'s roteamento da IRQ1 passa a resolver o GSI real através
  de uma eventual substituição MADT para a IRQ1, em vez de assumir sempre
  GSI1 — sem efeito observável quando não existe substituição (o caso de
  QEMU/VirtualBox/VMware hoje).
- **Não é uma mudança disruptiva**: se o RSDP estiver ausente, ou qualquer
  tabela (RSDP/RSDT/XSDT/MADT) falhar a validação de soma de verificação, ou
  a MADT não tiver nenhuma entrada de I/O APIC, o kernel usa exatamente o
  comportamento de hoje (endereço fixo, mapeamento de IRQ identidade) — as
  três plataformas-alvo atuais continuam funcionando sem nenhuma mudança
  observável, já que também expõem ACPI válido (a mudança passa a
  *consumi-lo*, sem exigi-lo).

## Capabilities

### New Capabilities
- `kernel-acpi`: localização e validação de RSDP/RSDT/XSDT/MADT, e extração
  do endereço de I/O APIC e de substituições de origem de interrupção
  legada declaradas na MADT.

### Modified Capabilities
- `kernel-ioapic`: o endereço MMIO do I/O APIC e o mapeamento IRQ→GSI de
  interrupções legadas passam a ser descobertos via `kernel-acpi` quando
  disponível, com o comportamento de hoje como contingência.

## Impact

- `kernel/src/main.rs`: nova `RsdpRequest` do Limine; nova chamada
  `kernel::acpi::init()` logo após `kernel::hhdm::init`, antes de
  `kernel::apic::init()`/`kernel::ioapic::init()`/`kernel::keyboard::init()`.
- `kernel/src/ioapic.rs`: `IOAPIC_BASE_PHYS` vira um valor de contingência,
  não mais o único usado; `init` recebe o endereço efetivo a usar.
- `kernel/src/keyboard.rs`: resolve o GSI da IRQ1 via `kernel::acpi` antes
  de chamar `route_irq`.
- Novo módulo `kernel/src/acpi.rs`.
- Nenhuma mudança em `userland_c/` ou em qualquer *syscall* — puramente
  interno ao kernel, sem superfície nova exposta a modo usuário.
