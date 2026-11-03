# Design

## Context

Ver proposal.md - Why/What Changes para a motivação e o escopo. Pontos do estado atual relevantes para o "como":

- `kernel::paging::Mapper::map`/`map_new` sempre mapeiam com um frame físico já resolvido na hora da chamada — não existe "reservar sem mapear".
- `kernel::phys::FrameAllocator::alloc` **não zera** o frame devolvido (a lista livre intrusiva reaproveita os próprios bytes do frame livre para o ponteiro "próximo"; um frame recém-alocado pode conter lixo de um uso físico anterior). Qualquer garantia de "memória comprometida vem zerada" precisa zerar explicitamente depois de mapear — mesmo padrão já usado em `next_table_or_create` (`kernel/src/paging.rs:209`) para tabelas intermediárias novas.
- Layout de endereço virtual já reservado (ver `docs/architecture/overview.md`, seção 2): HHDM no índice 256 da PML4, heap do kernel a partir de `0xffff_9000_0000_0000` (índice 288), stack dedicada a partir de `0xffff_a000_0000_0000` (índice 320), kernel propriamente dito no topo (índice 511). Cada índice de PML4 cobre 512 GiB de espaço de endereçamento.
- `kernel::sync::Spinlock` é o único mecanismo de exclusão mútua disponível; o kernel é single-core, então um spinlock simples é suficiente e já é o padrão usado por `FRAME_ALLOCATOR`.
- O kernel já expõe `alloc::{Vec, String, Box}` livremente via `kernel::heap` (`linked_list_allocator`), então a lógica de contabilidade (faixas livres, estado de comprometimento por reserva) pode usar coleções normais em vez de estruturas intrusivas escritas à mão, diferente do alocador físico de boot.
- O handler de `#PF` (page fault) trata hoje qualquer falta de página como evento fatal (halt) — não existe nenhum mecanismo de "esta falta é esperada, resolva e continue" (isso é exatamente o gap de "sinal de safepoint" registrado em `jvm-posix-compat-strategy`, fora do escopo desta change).

## Goals / Non-Goals

**Goals:**
- Fornecer, dentro do próprio kernel (sem syscall nova), as primitivas de reserva/comprometimento/descomprometimento/proteção/liberação descritas na spec `kernel-virtual-memory`.
- Detectar e rejeitar explicitamente todo uso indevido (fora dos limites, duplo comprometimento, handle obsoleto após liberação) em vez de corromper estado silenciosamente — mesmo padrão de robustez já usado em `kernel::phys`/`kernel::paging`.
- Manter a lógica de contabilidade (faixas livres, estado de comprometimento) pura e testável em host, separada do código que efetivamente toca `Mapper`/`FrameAllocator` (só verificável em boot real) — mesmo padrão de `kernel::heap`/`kernel::paging`.

**Non-Goals:**
- Expor `mmap`/`munmap`/`mprotect` como *syscalls* nomeadas para modo usuário (fica para uma sub-change futura, quando a ABI de syscalls for estendida para a VM hóspede).
- Paginação sob demanda disparada por *page fault* (`commit` automático a partir do handler de `#PF`): o `commit` desta change é sempre uma chamada explícita. Isso depende do mecanismo de "sinal de safepoint"/handler de falta configurável, que é um gap separado do inventário de `jvm-posix-compat-strategy`.
- Integração com `kernel::process::Process` (uma faixa de reserva por processo, liberação automática na destruição do processo): esta change opera sobre uma única faixa virtual fixa, análoga a `kernel::heap`, compartilhada pelo kernel inteiro (consumida apenas pelo novo demo nesta change).
- Suporte a huge pages (2 MiB/1 GiB) nas páginas comprometidas — permanece futuro, como já registrado para `kernel-paging` em geral.

## Decisions

### Faixa virtual fixa para reservas, em um novo índice de PML4
Reserva-se o índice 352 da PML4 (`0xffff_b000_0000_0000..0xffff_b7ff_ffff_ffff`, 512 GiB de espaço de endereçamento) para o `kernel::vmm` — o primeiro índice livre após o heap (288) e a stack (320), seguindo o mesmo padrão de espaçamento já estabelecido entre eles. Dentro desse índice, um subconjunto de 64 GiB (`VMM_WINDOW_SIZE`) é de fato gerenciado pelo alocador de faixas — generoso o suficiente para qualquer reserva de demonstração ou, futuramente, um heap Java inteiro reservado de uma vez, sem precisar de todo o índice de PML4. Alternativa descartada: reaproveitar a faixa do heap existente — rejeitada porque o heap já tem sua própria política de crescimento (`kernel::heap::grow`) e semântica diferente (sempre comprometido, nunca "reservado mas não committado"); misturar as duas responsabilidades no mesmo intervalo tornaria ambas mais difíceis de raciocinar.

### Contabilidade em `alloc::Vec`/`BTreeMap`, não em estrutura intrusiva
Ao contrário do `FrameAllocator` (que não pode depender do heap do kernel, já que inicializa antes dele), `kernel::vmm` roda depois do heap estar pronto, então sua lista de faixas livres e a tabela de reservas vivas usam coleções normais (`alloc::vec::Vec`, `alloc::collections::BTreeMap`) em vez de uma lista ligada intrusiva. Isso simplifica a implementação e mantém a lógica pura mais fácil de testar em host. Tudo protegido por um único `kernel::sync::Spinlock<VmmState>` global (mesmo padrão de `FRAME_ALLOCATOR`), suficiente enquanto o kernel for single-core.

### Alocador de faixas livres: lista ordenada com coalescência, *first-fit*
`reserve`/`release` operam sobre um `Vec<FreeRange>` ordenado por endereço base. `reserve(size)` busca a primeira faixa livre grande o suficiente (*first-fit*; simples e previsível, suficiente nesta escala — não há necessidade de *best-fit* com poucas reservas esperadas), fazendo *split* se sobrar espaço. `release` devolve a faixa à lista e funde (*coalesce*) com vizinhas adjacentes livres, para que o espaço reaproveitado por reservas futuras não fique permanentemente fragmentado em pedaços pequenos — cobre o requisito "Reaproveitamento de Faixas Virtuais Liberadas" da spec. Toda essa lógica (busca, split, coalescência) é pura (opera só sobre uma lista de `{base, size}` em memória), testável em host sem hardware.

### `Region` como handle opaco com geração, para detectar uso após liberação
Uma reserva é identificada por uma `struct Region { base: u64, size: u64, generation: u64 }`, devolvida por `reserve` e exigida por `commit`/`decommit`/`protect`/`release`. A tabela de reservas vivas é um `BTreeMap<u64 /* base */, ReservationState>`, onde `ReservationState` guarda `generation` e um `Vec<bool>` com o estado de comprometimento por página. Toda operação compara a `generation` do handle recebido com a armazenada; uma reserva liberada é removida do mapa, e seu espaço de endereço pode ser reaproveitado por uma reserva futura com uma `generation` mais alta — um handle antigo apresentado depois disso é rejeitado (`VmmError::StaleHandle`) em vez de operar silenciosamente sobre a reserva nova que por acaso ocupa o mesmo endereço (problema ABA). Mesmo espírito de `kernel::phys::FrameAllocator::free` detectando double-free explicitamente, aplicado aqui a reservas inteiras.

### `commit`/`decommit` mapeiam e desmapeiam página a página via `Mapper`/`FrameAllocator` existentes
`commit` aloca um frame por página via `FrameAllocator::alloc`, zera seu conteúdo (`core::ptr::write_bytes`, mesma técnica de `next_table_or_create`) **antes** de considerar a página comprometida, e então chama `Mapper::map_new`... na prática `Mapper::map` com o frame já zerado, não `map_new` (que zera depois) — a ordem é zerar primeiro, mapear depois, para nunca expor conteúdo não zerado mesmo que o acesso ocorra imediatamente após o mapeamento. `decommit` chama `Mapper::unmap` por página e devolve o frame via `FrameAllocator::free`. Comprometer uma sub-região já comprometida (mesmo parcialmente) é rejeitado a priori pela checagem de bounds + estado por página (`ReservationState`), sem tocar `Mapper`/`FrameAllocator` nesse caso de erro — cobre "nenhum frame físico SHALL ser alocado como efeito colateral" da spec.

### `protect` usa a nova primitiva `Mapper::protect` (extensão de `kernel-paging`)
`Mapper` ganha `protect(&mut self, virt: u64, flags: PageFlags) -> Result<(), MapError>`: resolve a caminhada de tabelas (mesma lógica de `translate`/`unmap`), e se a página estiver presente, reescreve a `PageTableEntry` com o mesmo endereço físico e as novas flags, seguido de `flush_tlb(virt)` — sem alocar nem liberar nenhum frame. Retorna `MapError::NotMapped` se a página não estiver presente, cobrindo "Alterar permissões de uma página não mapeada é rejeitada". `kernel::vmm::protect` usa esse método por página dentro da sub-região solicitada, após verificar (via `ReservationState`) que toda a sub-região está de fato comprometida.

### Verificação de "acesso falha" em boot real, sem handler recuperável
As specs exigem que acesso a memória não comprometida (ou já descomprometida/liberada) "falhe" e que uma página protegida contra escrita rejeite escrita. Como não existe handler de `#PF` recuperável (fora do escopo, ver Non-Goals), a verificação em boot real segue o mesmo método já usado por `kernel-stack-guard-page`: provocar deliberadamente o acesso num cenário de teste temporário e confirmar, via GDB/console serial, que `#PF` é levantado com o endereço (`CR2`) dentro da sub-região esperada (não comprometida, ou protegida contra escrita) — sem esperar que a execução continue depois. Isso é suficiente para provar a primitiva; tornar a falta *recuperável* é trabalho de uma change futura (safepoint).

## Achados durante a implementação

Os quatro cenários de `#PF` esperado pela spec `kernel-virtual-memory` (acesso a sub-região reservada e não comprometida; escrita após `protect` remover a permissão de escrita; acesso após `decommit`; reaproveitamento do espaço virtual após `release`) foram verificados em boot real (QEMU, `cargo xtask test`), com um gatilho temporário inserido e removido em `kernel::vmm_demo::run_fallible` um de cada vez (ver tarefas 4.2/4.3) — a mesma técnica já usada por `kernel-stack-guard-page`. Os três que dependem de `#PF` confirmaram exatamente o endereço esperado em `CR2` (sempre a base da reserva, `0xffffb00000000000` no teste, já que a leitura/escrita de verificação sempre mirava o primeiro byte da sub-região):

- **Acesso a sub-região reservada e não comprometida**: `EXCECAO #PF (page fault): endereco=0xffffb00000000000 codigo_erro=0x0 ...` — código de erro `0x0` (bit 0 zerado: página ausente), como esperado para uma página nunca mapeada.
- **Escrita após `protect` remover a permissão de escrita**: `EXCECAO #PF (page fault): endereco=0xffffb00000000000 codigo_erro=0x3 ...` — código de erro `0x3` (bit 0 e bit 1 setados: página presente, falha causada por uma escrita), confirmando que `Mapper::protect` de fato restringiu a permissão sem desmapear a página (bit "presente" continua ligado no código de erro).
- **Acesso após `decommit`**: `EXCECAO #PF (page fault): endereco=0xffffb00000000000 codigo_erro=0x0 ...` — mesmo padrão do primeiro cenário (página ausente), confirmando que `decommit` de fato desmapeou a página em vez de só marcar o estado interno.

O quarto cenário (reaproveitamento do espaço virtual após `release`) não depende de nenhum `#PF`: verificado diretamente pelo console serial no caminho feliz permanente de `kernel::vmm_demo` (sem gatilho temporário), comparando a base devolvida pela nova `reserve` com a base da reserva liberada — `vmm: nova reserva reaproveitou o espaco virtual liberado (base 0xffffb00000000000) - PASSOU`.

Como esperado (ver "Non-Goals"), cada um dos três cenários de `#PF` halta o kernel pelo resto do boot (sem handler recuperável) — por isso o gatilho de cada um foi inserido, testado e removido individualmente, nunca os três simultaneamente, e o build final não contém nenhum deles: o caminho feliz de `kernel::vmm_demo` (reserva, comprometimento zerado, escrita/leitura, proteção, descomprometimento, liberação, reserva de novo) nunca dispara `#PF`, preservando o comportamento observável de todo o restante do boot (teclado, armazenamento, rede, modo usuário) exatamente como antes desta change.

## Risks / Trade-offs

- [Reaproveitamento de faixas via *first-fit* pode fragmentar o espaço virtual ao longo de muitos ciclos reserva/libera de tamanhos variados] → Impacto limitado: o espaço de endereço virtual da faixa (64 GiB) é muito maior que o uso esperado (um demo, e futuramente um número pequeno de reservas por processo/JVM); fragmentação de endereço virtual não tem o mesmo custo de fragmentação de memória física. Revisitar o algoritmo apenas se um uso futuro real demonstrar necessidade.
- [`Vec<bool>` por página para rastrear comprometimento gasta 1 byte por página de 4 KiB reservada (256 KiB de overhead por GiB reservado)] → Aceitável na escala desta change (reservas de demonstração, não um heap Java de produção ainda); uma reserva muito grande e esparsamente comprometida pagaria esse overhead proporcionalmente ao tamanho total reservado, não ao comprometido. Se isso importar no futuro (ex.: JVM reservando dezenas de GiB), trocar por um bitset é uma otimização isolada que não muda a API pública.
- [Confiar em verificação manual via GDB/serial para "acesso falha", em vez de um teste automatizado de ponta a ponta] → Mesma limitação já aceita por `kernel-stack-guard-page`/`kernel-wait-queue` para comportamento que só existe em boot real; documentar o resultado em design.md (seção "Achados durante a implementação", a ser preenchida durante `tasks.md`) como já é costume no projeto.
- [Faixa fixa de 64 GiB escolhida sem um caso de uso concreto ainda (nenhum consumidor real além do demo)] → Se o porte da JVM revelar necessidade de uma faixa maior ou de múltiplas faixas por processo, isso é uma extensão aditiva (mudar uma constante, ou generalizar para múltiplas janelas) que não invalida a API de `reserve`/`commit`/`decommit`/`protect`/`release` já especificada.

## Migration Plan

Não aplicável a infraestrutura nova sem consumidores existentes: nenhum código atual (`kernel::heap`, `kernel::stack`, `kernel::process`) muda de comportamento ou passa a depender de `kernel::vmm` nesta change. A ativação é apenas o registro da thread de demo em `continue_boot`, reversível removendo esse spawn.
