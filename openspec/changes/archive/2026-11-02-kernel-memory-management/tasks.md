# Tasks

## 1. Alocador de Frames Físicos (`kernel-physical-memory`)

- [x] 1.1 Declarar a boot request `HhdmRequest` no kernel e implementar os helpers `phys_to_virt`/`virt_to_phys` (aplicando o offset HHDM), com um teste unitário puro (host) verificando a conversão de endereço em ambas as direções.
- [x] 1.2 Implementar a lista livre intrusiva de frames, inicializada a partir das regiões `usable` do memory map do Limine (reaproveitando o parsing já usado por `kernel::memmap_report`), e verificar com um teste unitário que, para um memory map sintético conhecido, a contagem total de frames inicializados é a esperada.
- [x] 1.3 Implementar `alloc_frame`/`free_frame` (O(1) sobre a lista livre), com testes unitários cobrindo: alocação decrementa a contagem de livres, liberar um frame permite realocá-lo, alocar sem frames livres retorna um erro explícito (não um endereço inválido), e liberar um endereço fora do intervalo gerenciado é rejeitado — cobre os cenários "Alocação bem-sucedida...", "Falha explícita quando não há memória livre", "Frame liberado pode ser realocado" e "Liberação de frame inválido é rejeitada" da spec.
- [x] 1.4 Tratar explicitamente a ausência de memory map na inicialização do alocador físico (bootloader não respondeu), retornando um erro de inicialização em vez de assumir um layout arbitrário, com teste unitário cobrindo esse caso — cobre "Memory map indisponível impede a inicialização".
- [x] 1.5 Integrar a inicialização do alocador físico em `kernel_main` (logo após o relato do mapa de memória já existente) e reportar no banner de boot serial o total de frames e quantos estão livres, verificando com `cargo xtask run -- -serial stdio -display none` que a linha aparece com valores plausíveis — cobre "Relato de Estatísticas de Memória Física".

## 2. Gerenciamento de Paginação (`kernel-paging`)

- [x] 2.1 Definir as estruturas `repr(C)` de entrada de tabela de página (PTE) e os helpers de flags (presente, leitura/escrita, usuário/supervisor, no-execute), com teste unitário cobrindo a codificação e decodificação correta dos bits de cada flag.
- [x] 2.2 Implementar a leitura do `CR3` atual e a caminhada (walk) das tabelas de 4 níveis via HHDM, incluindo a criação de tabelas intermediárias ausentes sob demanda (solicitando frames ao alocador físico da seção 1), e verificar manualmente no QEMU (via `cargo xtask run -- -s -S` + GDB, inspecionando a tabela antes/depois) que criar uma entrada nova não corrompe entradas vizinhas já existentes.
- [x] 2.3 Implementar `map_page`/`unmap_page`, rejeitando o mapeamento de um endereço virtual já mapeado quando a substituição não é solicitada explicitamente — cobre "Mapeamento de Página Virtual" (ambos os cenários) e "Desmapeamento de Página Virtual". Verificar com um teste de integração manual: mapear uma página de teste fora do range do kernel, escrever e ler de volta pelo endereço virtual mapeado, desmapeá-la, e confirmar (via `cargo xtask run -- -s -S` + GDB observando o fault, ou via handler de page fault temporário) que o acesso subsequente falha em vez de ler dado obsoleto. Reverter qualquer código de teste temporário após verificar.
- [x] 2.4 Confirmar que as operações de mapeamento/desmapeamento de páginas de teste não afetam o mapeamento higher-half do próprio kernel, rodando `cargo xtask test` normalmente após exercer map/unmap no caminho de boot — cobre "Preservação do Mapeamento do Kernel".

## 3. Heap do Kernel (`kernel-heap`)

- [x] 3.1 Adicionar a dependência `linked_list_allocator` ao `kernel/Cargo.toml` e definir a constante de faixa virtual reservada para o heap (endereço base e tamanho máximo, documentados no código com a justificativa de não colidir com a HHDM nem com o código do kernel), verificando que o crate ainda compila para o target bare-metal.
- [x] 3.2 Implementar a inicialização do heap: mapear o primeiro bloco de frames (via `kernel-paging`) na faixa reservada e inicializar o `linked_list_allocator` sobre essa região, registrando-o como `#[global_allocator]`; verificar com um teste manual temporário (criar um `Vec`/`String` logo após a inicialização e confirmar sucesso via serial) — cobre "Alocação Dinâmica Geral". Remover o código de teste temporário após verificar.
- [x] 3.3 Implementar o crescimento sob demanda do heap (mapear mais frames imediatamente adjacentes na faixa reservada e chamar `extend()` do allocator) quando uma alocação não cabe no espaço já mapeado mas há memória física livre; verificar rodando `cargo xtask run -- -serial stdio -display none` com uma alocação temporária maior que o tamanho inicial do heap, confirmando sucesso via serial — cobre "Crescimento sob Demanda". Reverter a alocação de teste após verificar.
- [x] 3.4 Implementar o `#[alloc_error_handler]` (ajustando ao mecanismo atual do toolchain nightly pinado, caso a API tenha mudado — ver nota em `design.md`), reportando a falha via `serial_println!` e entrando em `hlt_loop`; verificar forçando temporariamente uma alocação maior que o limite máximo da faixa reservada e confirmando que o kernel reporta a falha e para limpo, sem corrupção nem reinício — cobre "Falha de Alocação Tratável". Reverter a alocação de teste após verificar.
- [x] 3.5 Reportar no banner de boot o tamanho inicial do heap mapeado, e confirmar com `cargo xtask test` que o smoke test de boot continua passando com o novo marcador de saída serial.

## 4. Testes e Integração Final

- [x] 4.1 Revisar as três specs (`kernel-physical-memory`, `kernel-paging`, `kernel-heap`) e garantir que todo cenário ainda não coberto por um teste unitário de host tenha um, rodando `cargo test -p kernel --lib` e confirmando que todos passam.
- [x] 4.2 Atualizar `docs/architecture/overview.md` (seção "2. Arquitetura de Memória") para refletir o que foi efetivamente implementado nesta change (alocador físico, paginação, heap), mantendo claro o que ainda é futuro (isolamento de endereço por processo, huge pages, NUMA), e verificar que o documento corresponde ao estado real do código.
- [x] 4.3 Rodar a regressão completa do zero (`rm -rf target boot/dist`, `cargo kernel-build`, `cargo test -p kernel --lib`, `cargo xtask test`) e confirmar que tudo passa antes de considerar a change pronta para archive.

## Workflow follow-up

- Arquivar a change (`/opsx:archive`) depois que a revisão do usuário estiver satisfeita.
- Verificar que a spec principal `openspec/specs/{kernel-physical-memory,kernel-paging,kernel-heap}/spec.md` foi criada corretamente após o archive.
