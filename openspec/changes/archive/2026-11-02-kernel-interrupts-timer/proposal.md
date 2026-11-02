# Proposal

## Why

O kernel hoje não tem IDT própria: `kernel_main` começa com `cli` e nunca a reabilita, um contorno introduzido durante `kernel-memory-management` depois de descobrir (via depuração) que uma janela de execução mais longa com interrupções habilitadas podia desviar a CPU para a IDT obsoleta deixada pelo firmware/Limine. Esse contorno funciona apenas enquanto o kernel não precisa reagir a nada em tempo real — mas bloqueia qualquer trabalho futuro do roadmap (`docs/architecture/overview.md`, item 3) que dependa de interrupções: escalonador preemptivo, drivers que esperam IRQs, e até diagnosticar corretamente uma exceção de CPU (hoje qualquer page fault ou GPF durante o boot ainda causaria um triple fault silencioso, já que não há handler nenhum instalado).

## What Changes

- Construir uma IDT própria com handlers para as exceções de CPU essenciais (page fault, general protection fault, double fault), cada um reportando diagnóstico (vetor, código de erro, endereço de falta quando aplicável) pelo canal serial já existente e então entrando na mesma rotina de halt definida em `kernel-boot`, em vez de arriscar um triple fault.
- Dar ao handler de double fault uma stack própria via IST (Interrupt Stack Table), para que ele continue funcionando mesmo diante de um estouro de pilha do kernel.
- Inicializar o Local APIC (substituindo a dependência do estado de interrupção deixado pelo firmware) e instalar um handler padrão para qualquer vetor de interrupção sem handler específico, em vez de deixar interrupções inesperadas sem tratamento.
- Configurar o timer do APIC para disparar periodicamente e manter um contador de ticks.
- Remover o `cli` permanente no início de `kernel_main` agora que existe uma IDT real, habilitando interrupções com segurança.

## Capabilities

### New Capabilities
- `kernel-interrupts`: a IDT do kernel, os handlers de exceção de CPU (incluindo double fault com stack dedicada) e o handler padrão para interrupções de hardware sem tratamento específico.
- `kernel-timer`: a configuração do timer do APIC para disparo periódico e o contador de ticks mantido pelo seu handler.

### Modified Capabilities
- Nenhuma — o contrato de halt já definido em `kernel-boot` (`openspec/specs/kernel-boot/spec.md`, "Comportamento Definido de Halt") já cobre "erro irrecuperável durante o boot" de forma genérica; os novos handlers de exceção apenas passam a acionar esse mesmo contrato a partir de uma causa nova, sem mudar o que ele exige.

## Impact

- **Novos módulos no crate `kernel`**: IDT/handlers de exceção, inicialização do Local APIC, e o timer — somando-se aos módulos de `kernel-boot-foundation` e `kernel-memory-management`.
- **Mudança de comportamento em `kernel_main`**: o `cli` permanente no início é substituído por "configurar IDT e APIC, depois habilitar interrupções" — mudança interna, não afeta o comportamento de boot já especificado.
- **Nenhuma dependência externa nova esperada**: segue a filosofia atual do projeto (IDT e handlers escritos à mão, sem crates como `x86_64` ou `pic8259`, consistente com as decisões já tomadas em `kernel-memory-management`).
- **Saída de boot (serial)**: o banner existente ganha linhas adicionais confirmando a inicialização da IDT/APIC/timer; o smoke test de boot (`cargo xtask test`) pode precisar de novos marcadores.
- **Desbloqueia trabalho futuro**: escalonador preemptivo (roadmap item 4) e qualquer driver que dependa de IRQs (roadmap item 6) passam a ter a infraestrutura de interrupções que já pressupõem.
