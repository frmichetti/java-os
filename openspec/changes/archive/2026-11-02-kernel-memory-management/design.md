# Design

## Context

O kernel hoje (pós `kernel-boot-foundation`) sobe via Limine, reporta boot/mapa de memória, e para em halt limpo — sem nenhuma forma de alocação dinâmica. `build-std` já inclui a crate `alloc`, mas nenhum `#[global_allocator]` foi registrado, então nada em `alloc::*` pode ser usado hoje. Não existe nenhum código de paginação própria do kernel: a paginação de 4 níveis ativa quando `kernel_main` começa a executar foi inteiramente montada pelo Limine antes de entregar o controle (CR3 já aponta para tabelas válidas, o kernel está mapeado higher-half). O Limine também mantém, para toda a memória usável/reclamável, um mapeamento direto de physical→virtual com offset fixo (HHDM — Higher-Half Direct Map), obtido via boot request própria (`HhdmRequest`, ainda não declarada pelo kernel). Ver `proposal.md` — Why para a motivação e `openspec/specs/{kernel-physical-memory,kernel-heap,kernel-paging}/spec.md` para o comportamento exigido de cada capability.

## Goals / Non-Goals

**Goals:**
- Alocador de frames físicos funcional sobre as regiões `usable` do memory map, sem depender de heap nem de paginação própria do kernel para inicializar (quebra a ordem circular via HHDM).
- Heap do kernel utilizável por qualquer módulo via `alloc::{Vec, String, Box}`.
- API de mapeamento/desmapeamento de páginas virtuais arbitrárias, usada pelo próprio heap para crescer sobre uma região virtual contígua reservada, e disponível para uso futuro (framebuffer, processos).
- Resolver de forma definitiva, nesta change, a ordem de bootstrap entre as três capabilities (ver Decisões), para não deixar uma dependência circular nas específicações.

**Non-Goals (nesta change):**
- Alocação de múltiplos frames físicos contíguos (a camada de frames só aloca um frame de cada vez; crescimento de heap resolve contiguidade virtual via paginação, não via frames fisicamente contíguos).
- Huge pages (2 MiB/1 GiB), demand paging, copy-on-write, swap, ou qualquer política de reclamação de memória.
- Espaços de endereçamento por processo (CR3 próprio por processo) — isso é parte da change futura de Processos e Threads (roadmap item 4), que vai reusar a API de paginação definida aqui.
- SMP/concorrência real (o kernel continua single-core); as estruturas de dados (listas livres, tabelas de página) devem evitar suposições que impeçam adicionar locks depois, mas nenhum lock de verdade é necessário ainda.

## Decisions

### Ordem de bootstrap (resolve a dependência circular física/heap/paginação)
1. **`kernel-physical-memory`** inicializa primeiro, usando **apenas** o memory map do Limine e o offset HHDM (`phys_to_virt(addr) = addr + hhdm_offset`) para acessar a memória física diretamente — sem precisar de heap nem de paginação própria.
2. **`kernel-paging`** inicializa em seguida. Ele lê a tabela de nível 4 atual a partir de `CR3` (convertendo o endereço físico para virtual via HHDM) e pode criar novas tabelas intermediárias quando necessário — para isso, pede frames físicos ao alocador do passo 1 (única dependência entre as duas: paging depende de physical-memory para alocar *novas* tabelas, nunca o contrário).
3. **`kernel-heap`** inicializa por último, usando physical-memory (passo 1) para obter frames e paging (passo 2) para mapeá-los dentro de uma faixa virtual fixa e contígua reservada para o heap.

Esta ordem elimina a circularidade "heap precisa de paginação, paginação precisa de frames, frames não precisam de nada" ao usar a HHDM do Limine como o único mecanismo de acesso físico-sem-mapeamento-próprio necessário no bootstrap.

**Alternativa descartada**: fazer o heap crescer diretamente sobre endereços HHDM (sem usar `kernel-paging`). Mais simples, mas frames alocados um a um pelo alocador físico não são fisicamente contíguos, e um allocator de heap por extensão de região (bump/linked-list) precisa de uma região *virtualmente* contígua para crescer — exatamente o que a paginação resolve ao mapear frames espalhados numa faixa virtual única. Sem paginação própria, o heap ficaria limitado ao tamanho de um único frame por "pedaço", inviabilizando alocações maiores que 4 KiB.

### Alocador físico: lista livre intrusiva (não bitmap)
Cada frame livre guarda, nos seus próprios primeiros 8 bytes (acessados via HHDM), o endereço físico do próximo frame livre — uma pilha de frames livres encadeada, com um único ponteiro de topo. Alocar e liberar são O(1) e não exigem nenhuma estrutura de bookkeeping externa (sem precisar "achar espaço" para um bitmap antes do alocador existir).

**Alternativa descartada**: bitmap (1 bit por frame). Mais fácil de auditar/depurar e permite buscas de frames contíguos no futuro, mas exige reservar antecipadamente um array proporcional ao total de RAM (ex.: ~128 KiB para 4 GiB de RAM) em algum lugar — um problema de inicialização (onde colocar o bitmap antes de o alocador existir) que a lista encadeada intrusiva evita completamente. Fica registrado como extensão natural caso aloção de frames contíguos vire necessária (roadmap futuro).

### Heap: crate `linked_list_allocator` sobre uma faixa virtual reservada
Usar a crate `linked_list_allocator` (`no_std`, amplamente usada em kernels Rust, implementa `GlobalAlloc` com suporte a `extend()`) como o allocator registrado via `#[global_allocator]`, inicializado sobre uma faixa virtual fixa reservada no espaço de endereçamento do kernel (ex.: `0xffff_9000_0000_0000`, bem distante da HHDM e do próprio código do kernel), crescida sob demanda mapeando novos frames físicos nela via `kernel-paging`.

**Alternativa descartada**: implementar um bump/linked-list allocator próprio, como foi feito para o driver serial. Descartado aqui porque um heap allocator correto (coalescing de blocos livres, alinhamento, fragmentação) é consideravelmente mais arriscado de acertar à mão do que um driver de UART, e `linked_list_allocator` é pequena, auditável e sem dependências transitivas — o ganho de evitar mais uma dependência externa não compensa o risco de um bug sutil de corrupção de heap.

### Paginação: tabelas de página escritas à mão (sem crate `x86_64`)
Implementar a caminhada e edição de tabelas de página de 4 níveis diretamente (estruturas `repr(C)` para as entradas, leitura de `CR3` via `asm!`), em vez de usar a crate `x86_64` (que fornece abstrações prontas de `PageTable`/`Mapper`).

**Alternativa descartada**: crate `x86_64`. Reduziria esforço de implementação, mas quebraria a consistência já estabelecida em `kernel-boot-foundation` de escrever o código que toca hardware diretamente à mão (ex.: o driver serial não usa a crate `uart_16550`); mantém o kernel com controle fino sobre o layout de paginação desde já, o que facilita decisões futuras (isolamento de processo, KASLR, etc.).

### Falha de alocação
`linked_list_allocator` retorna null em falha; o kernel registra um `#[alloc_error_handler]` (ou o mecanismo equivalente mais atual do toolchain nightly pinado, a confirmar na implementação — já houve divergência de API nesse tipo de ponto de extensão entre versões do nightly, ver `kernel-boot-foundation`/design.md) que reporta a falha via `serial_println!` e entra na mesma rotina de halt definida em `kernel-boot` (`hlt_loop`), em vez de panicar de forma genérica ou continuar em estado indefinido.

## Risks / Trade-offs

- **[Risco] Bug em paginação própria pode corromper o mapeamento do kernel** → Mitigação: testar mapear/desmapear páginas de teste fora do range do kernel antes de qualquer uso real pelo heap, e validar via o requisito "Preservação do Mapeamento do Kernel" (`kernel-paging` spec) rodando o smoke test de boot normalmente depois de cada operação de paginação exercida.
- **[Risco] Fragmentação de heap ao longo do tempo** → Fora de escopo mitigar agora (não há carga de trabalho real ainda); `linked_list_allocator` faz coalescing básico de blocos adjacentes, suficiente para esta fase.
- **[Trade-off] Lista livre intrusiva não suporta alocação de frames contíguos** → Aceitável porque nenhum consumidor desta change precisa disso (heap resolve contiguidade via paginação); registrado como Non-Goal.
- **[Trade-off] Faixa virtual do heap tem tamanho máximo fixo reservado (ex.: algumas centenas de MiB)** → Simples de implementar e suficiente para esta fase; se esgotada, a alocação falha de forma tratável (ver Decisões — Falha de alocação) em vez de travar.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: o smoke test de boot (`cargo xtask test`) passa com a nova linha de estatísticas do alocador físico no banner, e os testes unitários de `kernel-physical-memory`/`kernel-paging` (lógica pura, testável no host como já estabelecido para `memmap_report`) passam. Rollback é reverter os commits desta change; nenhum dado persistente é afetado.
