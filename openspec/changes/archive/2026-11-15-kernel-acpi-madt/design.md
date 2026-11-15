# Design

## Context

Hoje, `kernel::ioapic::IOAPIC_BASE_PHYS = 0xFEC0_0000` é um valor fixo
codificado, documentado desde `kernel-keyboard` como "o padrão de qualquer
plataforma PC-compatível... sem parsing de ACPI/MADT" — e
`kernel::keyboard` roteia a IRQ1 assumindo que ela mapeia 1:1 para GSI1 na
tabela de redireção. Ambos corretos para QEMU/VirtualBox/VMware (os únicos
alvos até aqui), mas não garantidos em hardware real.

O Local APIC em si **já** é descoberto via hardware de verdade
(`kernel::apic::init`, MSR `IA32_APIC_BASE`) — esta change não mexe nele.
x2APIC continua obrigatório, sem fallback para xAPIC/MMIO (decisão já
tomada em `kernel-interrupts-timer`, fora de escopo aqui).

O bootloader (Limine, protocolo base revisão 6 — `BaseRevision::MAX_SUPPORTED`
em `kernel/src/main.rs`) já é capaz de entregar o ponteiro RSDP
(`limine::request::RsdpRequest`), mas o kernel nunca pede essa informação
hoje. Pela documentação da própria crate `limine` (v0.6.5): para a revisão
de protocolo em uso (6, não exatamente 3), o endereço do RSDP entregue já é
um ponteiro **virtual** diretamente utilizável — não precisa de tradução
via HHDM (diferente do endereço MMIO do I/O APIC, que é físico e precisa de
`kernel::hhdm::phys_to_virt`).

## Goals / Non-Goals

**Goals:**
- Descobrir o endereço MMIO real de um I/O APIC via MADT, quando disponível.
- Descobrir o `GSI` real de uma IRQ ISA legada (hoje só a IRQ1, teclado) via
  substituições de origem de interrupção da MADT, quando declaradas.
- Preservar exatamente o comportamento de hoje (endereço fixo, mapeamento
  identidade) quando ACPI está ausente, inválido, ou não declara a
  informação procurada — nunca uma regressão para QEMU/VirtualBox/VMware.

**Non-Goals:**
- Múltiplos I/O APICs — só a primeira entrada de tipo 1 da MADT é usada;
  hardware multi-soquete/servidor com mais de um I/O APIC fica fora de
  escopo (o próprio `kernel::ioapic::route_irq` já assume um único I/O
  APIC, decisão anterior não revisitada aqui).
- Enumeração de outros processadores/Local APICs (tipos 0 e 9 da MADT) —
  o kernel é de um único núcleo (SMP é item de roadmap separado, de prazo
  mais longo); `kernel::apic::local_apic_id()` já obtém o ID do próprio
  núcleo em execução via MSR, sem precisar da MADT para isso.
- Substituição de endereço do Local APIC (tipo 5) — o endereço do Local
  APIC já vem de `IA32_APIC_BASE` (MSR), uma fonte mais direta e autoritativa
  que não precisa de ACPI.
- Origens de interrupção não mascarável (tipos 3 e 4, NMI) — o kernel não
  roteia NMIs hoje.
- Qualquer tabela ACPI além da MADT (FADT, HPET, MCFG, SRAT, etc.) — MCFG
  (necessária para PCIe ECAM) já foi identificada como um item separado,
  deferido, em `kernel-storage/design.md`; fica para uma change própria.
- Interpretação de AML/DSDT/SSDT (bytecode ACPI) — um subsistema à parte,
  ordens de magnitude maior, não necessário para o objetivo desta change
  (só descobrir topologia de interrupção).

## Decisions

### Parsing manual, sem depender de uma crate externa de ACPI

Mesma disciplina já usada por `kernel::fat32`/`kernel::elf`/`kernel::dylib`:
as estruturas ACPI relevantes (cabeçalho de tabela genérico, RSDP, MADT e
suas entradas) são pequenas e bem documentadas publicamente — um parser
manual, testável em host com buffers construídos à mão, evita trazer uma
dependência externa (`acpi`/`acpi-tables` no crates.io) só para um
subconjunto pequeno do padrão. **Alternativa rejeitada**: usar a crate
`acpi` — rejeitada por trazer uma superfície muito maior do que o
necessário (ela cobre FADT/HPET/NFIT/etc.), inconsistente com a disciplina
já estabelecida neste projeto de parsers manuais para formatos de
hardware/arquivo bem documentados.

### Novo módulo `kernel::acpi`, estado global análogo a `kernel::clock`

`kernel::acpi::init()` é chamada uma única vez (logo após
`kernel::hhdm::init`, antes de `kernel::apic::init`/`kernel::ioapic::init`/
`kernel::keyboard::init` — todos que precisam da topologia descoberta) e
popula um estado global simples:
- `IOAPIC_ADDRESS: AtomicU64` (0 = nenhum descoberto; endereços MMIO reais
  nunca são zero)
- As substituições de origem de interrupção são poucas (tipicamente 0-4 em
  qualquer MADT real) — guardadas num array de tamanho fixo pequeno
  (`[Option<InterruptSourceOverride>; 16]`, indexado pela IRQ ISA de
  origem — no máximo 16 IRQs legadas existem, então o próprio índice já
  serve de chave, sem precisar de busca linear nem alocação).

Lógica pura (parsing de cabeçalho, validação de soma de verificação,
decodificação de uma entrada da MADT) fica em funções livres, testáveis em
host com bytes construídos à mão — só a localização do RSDP em si (via
`RsdpRequest` do Limine) e a leitura das tabelas apontadas por ele (ler
memória arbitrária apontada pelo RSDP/RSDT/XSDT) dependem de ponteiros
reais, mesma categoria de "só verificável em boot real" já aceita para
`Mapper::map`/`AddressSpace::new`/etc.

### `resolve_isa_irq_gsi` sempre devolve um valor (nunca `Option`)

Ao contrário de `ioapic_address()` (que devolve `Option<u64>`, já que "não
descoberto" é um estado real e distinto), `resolve_isa_irq_gsi(irq: u8) ->
u32` sempre devolve um `GSI` — `irq as u32` quando nenhuma substituição foi
declarada (ou nenhuma MADT está disponível), ou o `GSI` da substituição
quando existir. Isso reflete a própria semântica do padrão ACPI (ausência
de substituição **significa** identidade), não uma contingência do kernel —
`kernel::keyboard` não precisa de nenhuma lógica condicional extra, só troca
`route_irq(1, ...)` por `route_irq(kernel::acpi::resolve_isa_irq_gsi(1),
...)`.

### Validação de soma de verificação, não parsing "melhor esforço"

Toda tabela (RSDP, RSDT/XSDT, MADT) tem sua soma de verificação validada
antes de qualquer campo ser confiado — uma tabela corrompida ou um ponteiro
RSDP inválido é tratado exatamente como "ACPI indisponível" (mesmo
contingência), nunca lido parcialmente. Consistente com a disciplina já
usada por `kernel::fat32` (confirma a assinatura `0x55 0xAA`/`"FAT32   "`
antes de interpretar qualquer campo) e por `kernel::elf`/`kernel::dylib`
(todo deslocamento validado contra os limites do arquivo antes de ler).

## Risks / Trade-offs

- **[Risco] Um RSDP/MADT malformado, mas que passa na soma de verificação
  por coincidência, poderia levar a um endereço de I/O APIC ou GSI
  incorretos** → Mitigação: a mesma soma de verificação é o mecanismo de
  integridade que o próprio padrão ACPI define e todo sistema operacional
  real usa — não há verificação adicional mais forte possível sem a própria
  tabela estar corrompida de propósito; aceito como risco residual padrão
  da indústria.
- **[Risco] Boot real nunca testado de verdade neste projeto até agora
  (só QEMU/VirtualBox/VMware)** → Mitigação parcial: esta change só pode
  ser verificada em boot real num ambiente onde o RSDP de fato aponta para
  uma MADT com uma entrada de I/O APIC em endereço diferente de
  `0xFEC0_0000`, ou com uma substituição de IRQ — nenhuma das três
  plataformas-alvo atuais exercita esses dois caminhos (ambas "I/O APIC
  descoberto" / "substituição aplicada"), só o caminho de contingência.
  Ver tasks.md, "Verificação em boot real" — o caminho de contingência é
  verificável normalmente; os dois caminhos novos exigem hardware real ou
  uma MADT fabricada à mão para teste (fora do escopo de boot automatizado
  deste projeto).

## Migration Plan

Puramente aditivo para as três plataformas-alvo já suportadas (nenhuma
delas perde o comportamento de contingência) — o único caminho novo
(endereço de I/O APIC ou GSI vindos de verdade da MADT) só é exercitado
quando o hardware real declarar uma topologia diferente da hoje assumida.
Nenhum passo de migração necessário.

## Resultado real da implementação

**Testes de host**: `cargo test -p kernel --lib` passou com 316 testes (288
de antes desta change + 28 novos de `kernel::acpi` — validação de soma de
verificação, cabeçalho genérico, RSDP v1/v2, decodificação de entradas da
MADT, resolução de GSI).

**Boot real** (`cargo xtask test`): confirmado, mas **a suposição de risco
registrada acima não se confirmou** — o QEMU/OVMF usado neste projeto expõe
uma MADT real e válida, não só o caminho de contingência:
```
acpi: MADT valida - I/O APIC em 0xfec00000 - 5 substituicao(oes) de IRQ legada
```
O endereço descoberto coincide com o de contingência (esperado — é também o
padrão real do QEMU), mas as 5 substituições são genuínas: `IRQ0→GSI2`
(remapeamento clássico do PIT, universal em ACPI de PC-compatível),
`IRQ5/9/10/11→GSI` de mesmo número (declaradas explicitamente, mesmo sendo
identidade). **IRQ1 (teclado) não tem substituição** — `resolve_isa_irq_gsi
(1)` devolve `1`, então o comportamento do teclado permanece idêntico,
mesmo com o caminho de descoberta via ACPI genuinamente exercitado (prova
de que esse caminho funciona de ponta a ponta, não só o de contingência).
Todo programa de teste já existente reportou exatamente o mesmo padrão de
resultado de antes desta change.

### Dois achados reais graves, encontrados e corrigidos durante a verificação

1. **`read_validated_table` rejeitava toda tabela real não-trivial**: a
   implementação original lia 36 bytes (`SDT_HEADER_LEN`) para "espreitar"
   o comprimento declarado antes de reler a tabela inteira, mas chamava
   `SdtHeader::parse` sobre esse mesmo probe de 36 bytes — e `parse` já
   exige que o buffer contenha a tabela **inteira** (checagem de
   auto-consistência `bytes.len() < declared_len`). Qualquer tabela real
   maior que 36 bytes (a XSDT real do QEMU tem 84) falhava nessa checagem
   imediatamente, fazendo `read_validated_table` devolver `None` mesmo com
   um RSDP/XSDT perfeitamente válidos — mascarado, do ponto de vista do
   log de boot, como "ACPI indisponível" genérico. Investigado com uma
   instrumentação temporária (bytes brutos do cabeçalho real), confirmando
   a assinatura (`"XSDT"`) e o comprimento declarado (84) corretos, só a
   checagem de auto-consistência rejeitando por engano. **Corrigido**:
   nova `SdtHeader::peek_declared_length` (só exige os 8 primeiros bytes,
   sem a checagem de auto-consistência) usada exclusivamente para a
   primeira leitura; `SdtHeader::parse` continua reservada para validar a
   tabela já relida por completo.
2. **`kernel::acpi::init` chamava código que aloca no heap antes do heap
   existir**: a mensagem de diagnóstico final usava `alloc::format!`/
   `alloc::string::String`, mas `kernel::acpi::init` roda logo após
   `kernel::hhdm::init` — bem antes de `kernel::heap::init` (~90 linhas
   depois em `main.rs`). Qualquer alocação nessa janela do boot aborta
   imediatamente com `"ERRO FATAL: falha de alocacao de heap"`, antes
   mesmo do banner de boot aparecer. **Corrigido**: a mensagem passou a
   usar `kernel::fmtbuf::StackBuf` (buffer de pilha com `core::fmt::Write`,
   mesmo mecanismo já usado pelo próprio banner de boot em `main.rs`, que
   roda na mesma janela sem heap), nunca `alloc::format!`.

### Achado real menor: polaridade/modo de disparo de uma substituição não são propagados

O campo de *flags* de uma entrada de substituição de origem de interrupção
(polaridade/modo de disparo) existe na estrutura ACPI real, mas
`decode_interrupt_source_override_entry` não o extrai, e `kernel::ioapic::
redirection_entry` continua fixo em ativa-alta/borda independentemente de
qualquer substituição. Nenhuma das 5 substituições reais observadas em
boot (acima) usa polaridade/disparo diferente do padrão ISA, então isso
não foi exercitado como um problema real — documentado aqui como uma
simplificação deliberada em relação ao desenho original (ver tasks.md,
tarefa 4.2), não uma lacuna escondida: propagar esses bits fica para
quando um cenário real (ex.: uma IRQ de dispositivo PCI roteada por
substituição, ativa-baixa/nível) precisar deles.
