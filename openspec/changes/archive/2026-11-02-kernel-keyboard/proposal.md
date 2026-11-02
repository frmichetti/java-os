# Proposal

## Why

O roadmap do projeto (`docs/architecture/overview.md`, seção 5) prevê, como próximo driver depois do console de framebuffer (`kernel-framebuffer-console`, concluída), um driver de teclado — o primeiro dispositivo de **entrada** do kernel. Hoje o kernel não tem nenhuma forma de receber entrada do usuário: tudo que existe é saída (console serial, framebuffer). Sem teclado, o Java OS não pode evoluir para nada interativo (nem um shell simples, nem futuramente a JVM/aplicações).

Investigação do código existente (`kernel::apic`, `kernel::pic`) revelou uma lacuna que bloqueia qualquer driver orientado a IRQ de um dispositivo externo: o PIC 8259 legado é completamente desabilitado e mascarado desde `kernel-interrupts-timer`, e não existe nenhum driver de I/O APIC — o Local APIC sozinho só trata sua própria interrupção de timer interna (que não depende de roteamento externo). Hoje, literalmente nenhuma IRQ de hardware externo (teclado, e futuramente armazenamento/rede) tem caminho para chegar à CPU. Esta change inclui um driver mínimo de I/O APIC como pré-requisito, decidido com o usuário por ser a opção que entrega um teclado de verdade orientado a IRQ (em vez de polling) e deixa a base pronta para os próximos drivers externos do roadmap.

## What Changes

- Novo módulo `kernel::ioapic`: driver mínimo de I/O APIC (acesso MMIO ao registrador de seleção/janela de dados, programação de entradas da tabela de redirecionamento), usado para rotear a IRQ1 (teclado) ao Local APIC. Endereço base do I/O APIC assumido como o padrão fixo de plataforma PC-compatível (`0xFEC00000`), sem parsing de tabelas ACPI/MADT — simplificação documentada em design.md, consistente com o escopo atual do projeto (QEMU/VirtualBox/VMware; hardware real fica para o item 12 do roadmap).
- Novo módulo `kernel::keyboard`: driver do controlador PS/2 (portas 0x60/0x64) e do teclado no primeiro canal — inicialização do controlador, handler de IRQ1 que lê o scancode (set 1) e o traduz para um evento de tecla (pressionar/soltar + caractere ASCII via layout US QWERTY fixo), e uma fila de eventos simples para quem for consumir (ex.: um futuro shell).
- Nova abstração de HAL: `kernel::hal::KeyboardDevice`, implementada pelo driver de teclado, análoga ao `trait Console` já existente (saída) — um contrato mínimo para "algo que produz eventos de tecla", independente do dispositivo físico.
- Uma pequena demonstração de verificação (análoga ao `kernel::user_demo` de `kernel-syscalls`): eco dos eventos de tecla recebidos nos consoles existentes (serial + framebuffer, via `serial_println!`), só para prova de funcionamento ponta a ponta — não é um shell real.

## Capabilities

### New Capabilities
- `kernel-ioapic`: roteamento de interrupções de hardware externas (I/O APIC) para o Local APIC — pré-requisito para qualquer IRQ de dispositivo externo (teclado nesta change; armazenamento/rede em changes futuras).
- `kernel-keyboard`: driver de teclado PS/2 — inicialização do controlador, tratamento de IRQ1, tradução de scancodes para eventos de tecla com caractere ASCII (layout US QWERTY).

### Modified Capabilities
- `kernel-hal`: adiciona a abstração `KeyboardDevice` (entrada), ao lado do `Console` (saída) já existente — sem alterar o requisito de `Console`.

## Impact

- Código novo: `kernel/src/ioapic.rs`, `kernel/src/keyboard.rs`; `kernel/src/hal.rs` ganha um novo trait.
- `kernel/src/main.rs`/`continue_boot`: nova chamada de inicialização (I/O APIC depois do Local APIC; teclado depois da IDT/escalonador estarem prontos), seguindo a mesma ordem de dependências já estabelecida para os drivers existentes.
- `kernel/src/idt.rs`: um novo vetor de IRQ (teclado) registrado, mesmo padrão já usado pelo timer.
- Nenhuma mudança de comportamento em `kernel-interrupts`, `kernel-timer`, `kernel-syscalls` ou `kernel-framebuffer-console` — só adições.
- `docs/architecture/overview.md`: seção 5 (HAL e Drivers) e item 6 do roadmap atualizados ao final da implementação, como de costume.
