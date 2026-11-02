# Design

## Context

O kernel hoje (pós `kernel-memory-management`) desabilita interrupções (`cli`) como primeira instrução de `kernel_main` e nunca as reabilita — não existe IDT própria, nem GDT/TSS próprios (o kernel ainda roda sobre o GDT/TSS que o Limine deixou montado no boot, usado apenas implicitamente). Qualquer exceção de CPU hoje resultaria em triple fault (sem handler, sem double-fault stack), e qualquer interrupção de hardware que chegasse desviaria a execução para a IDT obsoleta do firmware/Limine — foi exatamente isso que motivou o `cli` permanente durante `kernel-memory-management` (ver `proposal.md` — Why). Ver `openspec/specs/{kernel-interrupts,kernel-timer}/spec.md` para o comportamento exigido.

## Goals / Non-Goals

**Goals:**
- IDT própria com handlers para as exceções de CPU essenciais (#PF, #GP, #DF), reportando diagnóstico e haltando em vez de arriscar um triple fault.
- GDT e TSS próprios do kernel (substituindo os do Limine), com uma entrada de IST dedicada para o handler de double fault.
- Local APIC inicializado e configurado como única fonte de interrupções de hardware (substituindo o PIC 8259 legado, que é desabilitado).
- Handler padrão para qualquer vetor de interrupção sem handler específico (reconhece e retorna, sem desviar para código externo ao kernel).
- Timer do Local APIC configurado para disparo periódico, com um contador de ticks exposto.
- Reabilitar interrupções com segurança no final da sequência de boot, removendo o `cli` permanente atual.

**Non-Goals (nesta change):**
- I/O APIC e roteamento de IRQs de dispositivos externos (teclado, disco, rede) — fica para a change de HAL/drivers (roadmap item 6), que é quando esses dispositivos passam a existir.
- Suporte a xAPIC legado (MMIO) — esta change exige x2APIC (ver Decisões); hardware sem x2APIC fica fora de escopo, consistente com o alvo já declarado do projeto (x86_64 moderno + QEMU/VMware/VirtualBox).
- Escalonador preemptivo usando o tick do timer — o contador de ticks é exposto, mas nada o consome ainda; isso é a change futura de Processos e Threads (roadmap item 4).
- SMP (múltiplos Local APICs) — o kernel continua single-core.

## Decisions

### GDT e TSS próprios, com uma entrada de IST para double fault
O kernel monta seu próprio GDT (descritores nulo, código e dados do kernel, e um descritor de TSS) e seu próprio TSS, com uma entrada de IST (`IST1`) apontando para uma stack dedicada ao handler de double fault. Carregados via `lgdt`/`ltr`, substituindo o GDT/TSS que o Limine deixa montado.

**Alternativa descartada**: reaproveitar o GDT/TSS que o Limine já deixa carregado (confirmado via GDB que `TR` já aponta para um TSS válido) e só adicionar uma entrada de IST nele. Descartada porque exigiria localizar e reinterpretar uma estrutura interna do Limine sem layout documentado/garantido entre versões — mais frágil do que montar uma própria, que é o padrão already estabelecido (o kernel já não depende de nenhuma estrutura do Limine além das boot requests).

### Stack do double fault em memória estática (não heap/paginação)
A stack dedicada ao handler de double fault é um array estático no `.bss` do kernel (`static mut [u8; N]`), não alocada via `kernel::heap`/`kernel::phys`. O handler de double fault precisa funcionar mesmo que o subsistema de memória esteja corrompido ou seja a própria causa do double fault (ex.: estouro de pilha); depender de alocação dinâmica aqui introduziria exatamente o tipo de falha que o handler existe para capturar.

### x2APIC obrigatório (sem fallback para xAPIC/MMIO)
O kernel requer suporte a x2APIC (detectado via `CPUID`), acessado inteiramente via MSRs (sem necessidade de mapear a região MMIO do xAPIC via `kernel::paging`). Hardware sem x2APIC não é suportado por esta change.

**Alternativa descartada**: suportar também o xAPIC legado (MMIO, `IA32_APIC_BASE` + offset). Descartada para reduzir escopo nesta primeira change de interrupções — x2APIC está disponível em todo hardware x86_64 dos últimos ~15 anos e em QEMU/VMware/VirtualBox (os alvos já declarados do projeto desde `kernel-boot-foundation`). Suporte a xAPIC fica registrado como extensão futura caso hardware mais antigo precise ser suportado.

### PIC 8259 legado desabilitado
O PIC 8259 legado é remapeado (para não colidir vetores com exceções de CPU) e então completamente mascarado (todas as linhas desabilitadas) durante a inicialização, antes de habilitar o Local APIC — garante que nenhuma IRQ legada chegue por um canal que o kernel não está mais tratando.

### Handlers com a ABI `extern "x86-interrupt"` do Rust
Os handlers de exceção e o handler padrão de interrupção são escritos como `extern "x86-interrupt" fn`, usando o suporte nativo do compilador (`#![feature(abi_x86_interrupt)]`) para gerar o prólogo/epílogo de uma interrupção (salvar/restaurar registradores, `iretq`) em vez de escrever esses trampolins em Assembly à mão.

**Alternativa descartada**: escrever os trampolins de entrada/saída de interrupção em Assembly puro. Mais consistente com "tudo que toca hardware é escrito à mão", mas a ABI `extern "x86-interrupt"` já É a forma de o compilador Rust gerar exatamente esse código de baixo nível sem depender de nenhuma crate externa — não é uma abstração de terceiros, é um recurso do próprio `rustc`, então o ganho de reescrever à mão não compensa o risco de um bug sutil no salvamento de registradores.

### Handler padrão gerado para os 256 vetores sem handler específico
Um handler genérico (reconhece a interrupção via EOI do Local APIC e retorna) é instalado em todos os vetores da IDT que não têm um handler específico (exceções e o vetor do timer), gerado por uma macro que produz uma função `extern "x86-interrupt"` distinta por vetor (necessário porque o número do vetor usado no EOI de algumas arquiteturas e para fins de diagnóstico precisa ser conhecido em tempo de compilação por handler).

## Risks / Trade-offs

- **[Risco] Erro na configuração de IST/TSS pode impedir a entrega do double fault, causando triple fault de qualquer forma** → Mitigação: verificar explicitamente (cenário da spec `kernel-interrupts`) provocando um estouro de pilha real no QEMU e confirmando que o handler ainda reporta diagnóstico antes de haltar.
- **[Risco] Exigir x2APIC pode não funcionar em alguma configuração específica de QEMU/VirtualBox mais antiga** → Mitigação: detectar via `CPUID` na inicialização e reportar um erro de boot explícito (reaproveitando o padrão de erro fatal já usado para `BaseRevision`/`HhdmRequest`) em vez de continuar com um Local APIC mal configurado.
- **[Trade-off] Sem I/O APIC nesta change** → Aceitável: nenhum dispositivo externo (teclado, disco) existe ainda no kernel; o timer do Local APIC não depende do I/O APIC.
- **[Trade-off] Handler padrão de interrupção não faz nada além de reconhecer (EOI)** → Aceitável como comportamento inicial; roadmap futuro (HAL/drivers) substitui vetores específicos por handlers reais conforme dispositivos forem adicionados.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: o smoke test de boot (`cargo xtask test`) passa com os novos marcadores de inicialização de IDT/APIC/timer, e os testes unitários cobrindo a lógica pura (ex.: construção de entradas da IDT, cálculo de divisores do timer) passam. Rollback é reverter os commits desta change.
