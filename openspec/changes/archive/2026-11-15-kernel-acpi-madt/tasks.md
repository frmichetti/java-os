# Tasks

## 1. Lógica pura de validação de tabela ACPI

- [x] 1.1 Adicionar `fn checksum_is_valid(bytes: &[u8]) -> bool` (soma de
      todos os bytes, módulo 256, deve dar zero — mesma regra para RSDP,
      RSDT/XSDT, e MADT) a `kernel::acpi` — função pura, testável sem
      hardware. Verificar com `cargo test -p kernel --lib`, testes
      cobrindo: buffer cuja soma bate (aceito), buffer cuja soma não bate
      (rejeitado), buffer vazio (caso de borda).
- [x] 1.2 Adicionar structs/funções de leitura do cabeçalho genérico de
      tabela ACPI (assinatura de 4 bytes, comprimento `u32`, demais campos
      do cabeçalho de 36 bytes) a partir de um `&[u8]`, devolvendo `None`
      se o buffer for menor que o cabeçalho ou menor que o comprimento
      declarado — função pura. Verificar com `cargo test -p kernel --lib`,
      testes cobrindo: cabeçalho válido com assinatura/comprimento
      esperados, buffer menor que o cabeçalho, comprimento declarado maior
      que o buffer disponível. **Achado real durante a implementação**: o
      lifetime elidido de `&self -> &[u8]` nos métodos de `SdtHeader<'a>`
      amarra ao empréstimo de `&self`, não ao parâmetro `'a` da struct —
      corrigido anotando explicitamente `&'a [u8]` em cada método
      (`signature`/`whole_table`/`body`), senão o compilador rejeita devolver
      um slice que sobrevive ao valor `SdtHeader` local que o produziu.

## 2. RSDP

- [x] 2.1 Adicionar `RsdpRequest` do Limine a `kernel/src/main.rs` (mesmo
      padrão de `HhdmRequest`/`FramebufferRequest` já existentes) —
      verificar que `cargo kernel-build` continua compilando. **Confirmado**
      com `cargo xtask build-image`.
- [x] 2.2 `kernel::acpi`: dado o ponteiro de `RsdpResponse` (já virtual,
      sem tradução HHDM — ver design.md), validar a assinatura
      (`"RSD PTR "`) e a soma de verificação (20 bytes para revisão 0, 36
      bytes quando a revisão indicar ACPI 2.0+) antes de extrair o
      ponteiro para a RSDT (campo de 32 bits) ou XSDT (campo de 64 bits,
      só presente/confiável na revisão 2+). Documentar claramente a parte
      que só é verificável em boot real (leitura de ponteiro real) vs. a
      parte pura (validação de um buffer já em mãos) — igual já feito para
      `kernel::fat32`/`Mapper`. **Confirmado**: `parse_rsdp_v1`/
      `parse_rsdp_v2_extension` são funções puras (testadas com buffers
      construídos à mão em `cargo test -p kernel --lib`); só `init` (que
      lê o ponteiro real) depende de boot real.

## 3. RSDT/XSDT e localização da MADT

- [x] 3.1 Localizar a RSDT ou XSDT (preferindo XSDT quando a revisão do
      RSDP indicar ACPI 2.0+ e o ponteiro for não-nulo), validar sua soma
      de verificação, e percorrer suas entradas (ponteiros de 32 bits na
      RSDT, de 64 bits na XSDT) procurando a assinatura `"APIC"` — devolve
      o ponteiro para a MADT encontrada, ou `None`.
- [x] 3.2 Validar a soma de verificação da própria MADT antes de aceitar
      qualquer entrada dela. **Confirmado**: `read_validated_table`
      (compartilhada por RSDT/XSDT e MADT — mesma lógica de "ler cabeçalho,
      descobrir comprimento real, reler e validar soma de verificação
      inteira") usada para as duas, mais a checagem da assinatura `"APIC"`
      em `find_madt`.

## 4. Entradas da MADT: I/O APIC e substituição de IRQ legada

- [x] 4.1 Decodificar uma entrada de tipo 1 (I/O APIC) da MADT (`ID`,
      endereço MMIO de 32 bits, `GSI` base) — função pura sobre um
      `&[u8]` da entrada, testável com bytes construídos à mão. Verificar
      com `cargo test -p kernel --lib`.
- [x] 4.2 Decodificar uma entrada de tipo 2 (substituição de origem de
      interrupção) da MADT (`bus`, IRQ de origem, `GSI` de destino,
      *flags* de polaridade/modo de disparo) — função pura. Verificar com
      `cargo test -p kernel --lib`. **Achado real, simplificação em relação
      à proposta original**: o campo de *flags* de polaridade/modo de
      disparo é lido pela struct ACPI real, mas `decode_interrupt_source_
      override_entry` não extrai nem propaga esses bits — `kernel::ioapic::
      redirection_entry` continua fixo em ativa-alta/borda (nenhuma
      substituição real exercitada por nenhuma plataforma-alvo hoje usa
      polaridade/disparo diferentes do padrão ISA); extrair e propagar
      esses bits fica para quando um cenário real precisar, evitando
      complexidade especulativa.
- [x] 4.3 Percorrer todas as entradas da MADT (cada entrada tem seu próprio
      campo de comprimento — tipo e comprimento desconhecidos são
      ignorados, não um erro) extraindo: o endereço MMIO da primeira
      entrada de tipo 1 encontrada; todas as substituições de tipo 2,
      indexadas pela IRQ de origem (no máximo 16). Verificar com `cargo
      test -p kernel --lib`, teste cobrindo uma MADT fabricada à mão com
      uma entrada de cada tipo (mais um tipo desconhecido, para confirmar
      que é ignorado sem quebrar o laço). **Confirmado**:
      `walk_madt_entries_finds_ioapic_and_overrides_among_mixed_entries`,
      mais um teste cobrindo um comprimento de entrada inconsistente
      (`walk_madt_entries_stops_on_an_inconsistent_length_without_panicking`,
      achado real não antecipado explicitamente na tarefa — o laço precisa
      parar com segurança, não só "ignorar", quando o comprimento declarado
      ultrapassa o restante do buffer).

## 5. API pública e inicialização

- [x] 5.1 `pub fn init(rsdp_response: Option<&RsdpResponse>)` (ou
      equivalente): orquestra as tarefas 2-4, populando o estado global
      (`IOAPIC_ADDRESS`, array de substituições) — nunca entra em pânico
      nem trava em nenhuma falha de validação (RSDP ausente/inválido,
      RSDT/XSDT sem MADT, MADT inválida), só deixa o estado global
      refletindo "nada descoberto". Reporta um diagnóstico no console
      serial em cada caso (mesmo padrão de `kernel::storage`/
      `kernel::network` para subsistemas opcionais). **Achado real,
      simplificação de assinatura**: `init` recebe `Option<*const u8>` (o
      ponteiro já extraído de `RsdpResponse::address` pelo chamador em
      `main.rs`), não a resposta do Limine inteira — evita acoplar
      `kernel::acpi` ao tipo da crate `limine` diretamente.
- [x] 5.2 `pub fn ioapic_address() -> Option<u64>` e `pub fn
      resolve_isa_irq_gsi(irq: u8) -> u32` (esta última nunca `Option` —
      ver design.md) expostas a partir do estado global. Verificar com
      `cargo test -p kernel --lib`.
- [x] 5.3 Chamar `kernel::acpi::init()` em `kernel/src/main.rs` logo após
      `kernel::hhdm::init`, antes de `kernel::apic::init()`. **Confirmado**
      com `cargo xtask build-image`.

## 6. Integração em `kernel::ioapic`/`kernel::keyboard`

- [x] 6.1 `kernel::ioapic::init`: usa `kernel::acpi::ioapic_address()` se
      `Some`, senão o `IOAPIC_BASE_PHYS` fixo de hoje (renomeado/documentado
      como valor de contingência, não removido) — verificar com `cargo
      test -p kernel --lib` que os testes existentes de `redirection_entry`
      continuam passando sem alteração (lógica pura, não afetada).
      **Confirmado**: 314 testes passando (288 de antes + 26 novos de
      `kernel::acpi`), nenhum teste de `ioapic::tests` precisou de ajuste.
- [x] 6.2 `kernel::keyboard`: troca `route_irq(1, vector, apic_id)` por
      `route_irq(kernel::acpi::resolve_isa_irq_gsi(1), vector, apic_id)` —
      verificar que compila; comportamento idêntico ao de hoje quando não
      há substituição declarada (sempre o caso em QEMU/VirtualBox/VMware).
      **Confirmado** com `cargo xtask build-image`.

## 7. Verificação em boot real

- [x] 7.1 Rodar `cargo xtask test` em QEMU e confirmar: (a) `acpi: ...`
      (diagnóstico novo) aparece no log de boot reportando o que foi
      descoberto (endereço de I/O APIC e quantas substituições, se
      alguma); (b) todo programa de teste já existente continua reportando
      exatamente os mesmos resultados de antes desta change — prova de que
      o caminho de contingência (QEMU) preserva o comportamento observável
      de hoje; (c) nenhuma `EXCECAO`/pânico novo relacionado a ACPI.

      **Dois achados reais graves, corrigidos durante a verificação**:
      1. `read_validated_table` chamava `SdtHeader::parse` sobre o *probe*
         de 36 bytes (lido só para descobrir o comprimento real da
         tabela) — mas `SdtHeader::parse` já exige que o buffer contenha a
         tabela **inteira**, então rejeitava sempre que o comprimento real
         excedia 36 bytes (o caso comum — a XSDT real do QEMU tem 84
         bytes). Isso fazia toda RSDT/XSDT real parecer inválida,
         mascarado como "ACPI indisponível" (o diagnóstico `acpi: RSDT/
         XSDT ausente ou invalida` aparecia mesmo com um RSDP/XSDT
         perfeitamente válidos). Investigado com uma instrumentação
         temporária (bytes brutos do cabeçalho + valores intermediários),
         removida depois de confirmada a causa. **Corrigido**: nova
         `SdtHeader::peek_declared_length` (só exige os primeiros 8 bytes,
         sem a checagem de auto-consistência de `parse`) usada na
         primeira leitura; `SdtHeader::parse` continua só para a segunda
         leitura (já do tamanho completo). Teste novo
         `peek_declared_length_reads_the_length_field_even_from_a_short_probe_buffer`
         reproduz exatamente o cenário do bug.
      2. `kernel::acpi::init` usava `alloc::format!`/`alloc::string::String`
         na mensagem de diagnóstico final — mas `kernel::acpi::init` é
         chamada logo após `kernel::hhdm::init`, bem **antes** de
         `kernel::heap::init` (main.rs, ~90 linhas depois) — qualquer
         alocação nesse ponto do boot falha com "ERRO FATAL: falha de
         alocacao de heap", abortando o boot inteiro antes mesmo do
         banner aparecer. **Corrigido**: a mensagem usa
         `kernel::fmtbuf::StackBuf` (mesmo padrão já usado pelo banner de
         boot em `main.rs`, que roda na mesma janela sem heap), nunca
         `alloc::format!`.

      **Confirmado após as duas correções**: `cargo test -p kernel --lib`
      com 316 testes passando (314 de antes + 2 novos de
      `peek_declared_length`); `cargo xtask test` com todo programa de
      teste reportando exatamente o mesmo padrão de resultado de antes
      desta change.
- [x] 7.2 Confirmar no mesmo log que o endereço de I/O APIC efetivamente
      usado (log de `kernel::ioapic::init`, já existente ou estendido
      nesta tarefa) bate com o que `kernel::acpi` descobriu via MADT do
      QEMU real (não necessariamente igual a `0xFEC0_0000` — documentar o
      valor real observado, qualquer que seja, como confirmação de que o
      caminho "descoberto via ACPI" está de fato sendo exercitado em QEMU,
      não só o de contingência).

      **Confirmado, e de forma mais rica do que o design.md antecipava**:
      `acpi: MADT valida - I/O APIC em 0xfec00000 - 5 substituicao(oes) de
      IRQ legada` — o QEMU/OVMF usado neste projeto expõe uma MADT real e
      válida (contrariando a suposição de risco do design.md, "nenhuma das
      três plataformas-alvo atuais exercita... os dois caminhos novos"). O
      endereço descoberto coincide com o de contingência (`0xfec00000`,
      esperado — é o padrão real de qualquer PC-compatível, incluindo
      QEMU), mas as 5 substituições são reais e distintas: `IRQ0→GSI2`
      (remapeamento clássico do PIT), `IRQ5/9/10/11→GSI` de mesmo número
      (declaradas explicitamente, mesmo sendo identidade — comum para
      sinalizar polaridade/disparo de IRQs PCI legadas). **IRQ1 (teclado)
      não tem substituição declarada** — `resolve_isa_irq_gsi(1)` devolve
      `1` (identidade), confirmando que o comportamento do teclado não
      muda nesta plataforma, mesmo com o caminho de descoberta via ACPI
      genuinamente exercitado (não só o de contingência).

## 8. Documentação

- [x] 8.1 Adicionar a este change (`design.md`, "Resultado real da
      implementação") a confirmação do que foi verificado (testes de host +
      boot real, incluindo o valor real de endereço de I/O APIC observado
      em QEMU) e o relato de qualquer achado real durante a implementação.
- [x] 8.2 Atualizar `ROADMAP.md` — mover "Parsing de ACPI/MADT" para a
      seção de implementado, documentando que o caminho de contingência
      continua sendo o único exercitado pelas plataformas-alvo atuais
      (nenhuma delas expõe uma MADT com endereço diferente do padrão ou
      com substituição de IRQ1 — ver design.md, "Riscos"), e apontar o
      próximo item real como próximo passo — verificar que o documento
      permanece consistente. **Achado real, correção em relação ao
      planejado**: ao contrário do antecipado, o QEMU/OVMF real *exercita*
      o caminho de descoberta via ACPI (não só o de contingência) — o
      texto final reflete isso, não a suposição original. "Próximo passo
      imediato" ficaria vazio (nenhum item de endurecimento de
      infraestrutura pendente) — substituído por um apontamento direto ao
      item 6 de "Destaque: o que falta para a JVM rodar" (examinar o
      código-fonte do HotSpot em si), em vez de deixar a seção vazia.

## Workflow follow-up

- Arquivar esta change (`openspec archive kernel-acpi-madt`) depois que a
  verificação em boot real (tarefas 7.1-7.2) e `cargo test -p kernel --lib`
  estiverem confirmadas.
