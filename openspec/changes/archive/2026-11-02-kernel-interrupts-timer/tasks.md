# Tasks

## 1. GDT e TSS Próprios

- [x] 1.1 Definir as estruturas `repr(C)` de descritor de segmento (entrada de GDT) e de TSS (64-bit), e construir estaticamente um GDT mínimo (nulo, código do kernel, dados do kernel, descritor de TSS) e um TSS com `IST1` apontando para uma stack estática dedicada ao double fault, com teste unitário cobrindo a codificação correta dos bits de um descritor de código/dados e de um descritor de TSS.
- [x] 1.2 Carregar o novo GDT via `lgdt` e o novo TSS via `ltr`, recarregando os seletores de segmento para os do novo GDT, e verificar manualmente no QEMU (monitor, `info registers`) que `GDT`/`TR` passam a apontar para as estruturas do próprio kernel em vez das deixadas pelo Limine.

## 2. IDT e Handlers de Exceção de CPU (`kernel-interrupts`)

- [x] 2.1 Definir a estrutura `repr(C)` de entrada da IDT (gate descriptor) e construir uma IDT estática de 256 entradas, com teste unitário cobrindo a codificação correta de um gate descriptor (offset do handler, seletor de código, índice de IST, tipo de gate, DPL).
- [x] 2.2 Implementar os handlers `extern "x86-interrupt"` para page fault, general protection fault e double fault (este último configurado na IDT para usar `IST1`), cada um formatando e emitindo pelo canal serial o vetor, o código de erro e, para page fault, o endereço de falta (via `CR2`), e então entrando na rotina de halt já existente; carregar a IDT via `lidt`. Verificar forçando temporariamente cada exceção no QEMU e confirmando a mensagem de diagnóstico seguida de halt limpo (sem reset) — cobre "Tratamento de Exceções de CPU". Reverter cada provocação temporária após verificar.
- [x] 2.3 Verificar especificamente o cenário de double fault causado por um estouro real da stack do kernel (ex.: recursão infinita temporária) e confirmar que o handler ainda executa e reporta diagnóstico (usando a stack dedicada `IST1`) — cobre "Double Fault com Stack Dedicada". Reverter a provocação após verificar.

## 3. Local APIC e Tratamento Padrão de Interrupções (`kernel-interrupts`)

- [x] 3.1 Implementar a detecção de suporte a x2APIC via `CPUID`, abortando o boot com um erro explícito (reaproveitando o padrão de erro fatal já usado para `BaseRevision`/`HhdmRequest` em `kernel-boot-foundation`) quando não suportado, com teste unitário cobrindo a lógica pura de interpretação dos bits relevantes a partir de um valor de `CPUID` sintético.
- [x] 3.2 Implementar o remapeamento e mascaramento completo do PIC 8259 legado, e a inicialização do Local APIC em modo x2APIC (habilitação via `IA32_APIC_BASE`, configuração do Spurious Interrupt Vector Register). Verificar manualmente no QEMU (`-d int`) que nenhuma IRQ legada do PIC chega mais após a inicialização.
- [x] 3.3 Implementar e instalar, via uma macro geradora, um handler padrão para todos os vetores da IDT sem handler específico, que reconhece a interrupção via EOI do Local APIC (MSR x2APIC) e retorna sem efeito colateral. Verificar habilitando interrupções e confirmando no QEMU que o kernel continua executando normalmente (não trava, não desvia para código externo) — cobre "Tratamento Padrão de Interrupções de Hardware".

## 4. Timer do APIC (`kernel-timer`)

- [x] 4.1 Implementar a configuração do timer do Local APIC (contagem inicial, divisor, modo periódico, vetor dedicado) e seu handler (incrementa um contador de ticks e emite EOI), com teste unitário cobrindo a lógica pura de cálculo do divisor/contagem inicial a partir de uma frequência alvo — cobre parte de "Timer Periódico via APIC".
- [x] 4.2 Expor o contador de ticks publicamente e reportá-lo pelo canal serial (ex.: periodicamente ou ao final da sequência de boot). Verificar rodando `cargo xtask run -- -serial stdio -display none` por alguns segundos e confirmando que o contador aumenta de forma consistente com o tempo decorrido e os disparos observados — cobre "Timer Periódico via APIC" e "Contagem de Ticks".

## 5. Integração Final

- [x] 5.1 Remover o `cli` permanente do início de `kernel_main`, substituindo pela sequência completa de inicialização (GDT/TSS, IDT, desabilitar PIC, inicializar APIC e timer) seguida de `sti` ao final, e verificar com `cargo xtask test` que o smoke test de boot continua passando com interrupções habilitadas.
- [x] 5.2 Revisar as specs `kernel-interrupts` e `kernel-timer` e garantir que todo cenário testável sem hardware real tenha um teste unitário de host, rodando `cargo test -p kernel --lib` e confirmando que todos passam.
- [x] 5.3 Atualizar `docs/architecture/overview.md` (seção "1. Arquitetura do Kernel" — Interrupções e exceções) para refletir o que foi efetivamente implementado nesta change, e verificar que o documento corresponde ao estado real do código.
- [x] 5.4 Rodar a regressão completa do zero (`rm -rf target boot/dist`, `cargo kernel-build`, `cargo test -p kernel --lib`, `cargo xtask test`) e confirmar que tudo passa antes de considerar a change pronta para archive.

## Workflow follow-up

- Arquivar a change (`/opsx:archive`) depois que a revisão do usuário estiver satisfeita.
- Verificar que as specs principais `openspec/specs/{kernel-interrupts,kernel-timer}/spec.md` foram criadas corretamente após o archive.
