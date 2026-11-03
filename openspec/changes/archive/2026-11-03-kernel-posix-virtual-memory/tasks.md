# Tasks

## 1. `kernel::paging`: `Mapper` ganha a primitiva de `protect`

- [x] 1.1 Implementar `Mapper::protect(virt, flags) -> Result<(), MapError>` (`kernel/src/paging.rs`): caminha as tabelas como `translate`/`unmap`, reescreve a `PageTableEntry` presente com o mesmo endereço físico e as novas flags, invalida a TLB da página (`flush_tlb`), e retorna `MapError::NotMapped` se a página não estiver presente — ver design.md, "`protect` usa a nova primitiva `Mapper::protect`". Verificar com `cargo kernel-build`.
- [x] 1.2 Adicionar teste unitário em host cobrindo a guarda de desalinhamento de `protect` (mesmo padrão de `translate_rejects_misaligned_address_without_touching_cr3`, já existente). Verificar com `cargo test -p kernel --lib`.

## 2. `kernel::vmm`: contabilidade pura (faixas livres e estado de reservas)

- [x] 2.1 Implementar o alocador de faixas livres (`Vec<FreeRange>` ordenado, *first-fit* com *split*, `release` com coalescência de vizinhos adjacentes) como lógica pura, sem nenhum acesso a `Mapper`/`FrameAllocator` — ver design.md, "Alocador de faixas livres". Verificar com testes unitários em host: reserva exata, reserva com sobra gera *split*, reserva maior que o espaço livre total é rejeitada, `release` funde com faixas livres adjacentes, e uma reserva após `release` reaproveita o espaço liberado mesmo quando o espaço nunca-usado restante sozinho não seria suficiente.
- [x] 2.2 Implementar o rastreamento de reservas vivas: `Region { base, size, generation }`, `ReservationState` (`generation` + `Vec<bool>` de páginas comprometidas) e um `BTreeMap<u64, ReservationState>` — ver design.md, "`Region` como handle opaco com geração". Verificar com testes unitários em host (sem tocar `Mapper`/`FrameAllocator`): comprometer dentro dos limites marca exatamente as páginas certas; comprometer fora dos limites, ou uma sub-região já total/parcialmente comprometida, é rejeitado sem alterar o estado; descomprometer desmarca as páginas; `protect` exige que toda a sub-região já esteja comprometida; um handle com `generation` antiga, apresentado após o `release` daquela reserva, é rejeitado (`StaleHandle`) mesmo que o endereço base tenha sido reaproveitado por uma reserva nova.

## 3. `kernel::vmm`: integração real com `Mapper`/`FrameAllocator`

- [x] 3.1 Definir a faixa virtual fixa (`VMM_WINDOW_START` no índice 352 da PML4, `VMM_WINDOW_SIZE` de 64 GiB — ver design.md, "Faixa virtual fixa para reservas") e implementar `Vmm::reserve`/`Vmm::release` ligando o alocador da tarefa 2.1 a essa faixa. Verificar que `cargo kernel-build` compila.
- [x] 3.2 Implementar `Vmm::commit`/`Vmm::decommit`: por página da sub-região, aloca um frame (`FrameAllocator::alloc`), zera seu conteúdo via HHDM antes de mapear (`Mapper::map`) para `commit`; desmapeia (`Mapper::unmap`) e libera o frame (`FrameAllocator::free`) para `decommit` — sempre validando limites e estado via a tabela da tarefa 2.2 antes de tocar `Mapper`/`FrameAllocator`. Verificar que `cargo kernel-build` compila e que `cargo test -p kernel --lib` não tem nenhuma regressão nos testes puros das tarefas 2.1/2.2.
- [x] 3.3 Implementar `Vmm::protect`, aplicando `Mapper::protect` (tarefa 1.1) página a página na sub-região, após confirmar via a tabela da tarefa 2.2 que toda ela já está comprometida. Verificar que `cargo kernel-build` compila.
- [x] 3.4 Declarar `pub static VMM: Spinlock<Vmm>` (mesmo padrão de `FRAME_ALLOCATOR`) e inicializá-la em `kernel_main` logo após o heap estar pronto. Verificar em boot real, via `cargo xtask test`, que a inicialização não falha e não introduz nenhuma regressão no relato de boot existente.

## 4. Demonstração e verificação ponta a ponta em boot real

- [x] 4.1 Implementar `kernel::vmm_demo`: reserva uma faixa, compromete uma sub-região, escreve um padrão de bytes conhecido nela, lê de volta e confirma a igualdade pelo console serial. Verificar que `cargo kernel-build` compila.
- [x] 4.2 Confirmar explicitamente, em boot real (QEMU/GDB), o requisito "Acesso a Sub-região Não Comprometida Falha": provocar uma leitura numa sub-região reservada e ainda não comprometida (cenário temporário só para esta verificação) e confirmar que `#PF` é levantada com `CR2` dentro da faixa esperada — mesmo método de verificação manual já usado por `kernel-stack-guard-page`. Documentar o resultado em design.md.
- [x] 4.3 Confirmar explicitamente, em boot real, os três cenários restantes que dependem de `#PF` observável: (a) escrita após `protect` remover a permissão de escrita gera `#PF` com `CR2` dentro da sub-região protegida; (b) após `decommit`, um novo acesso à mesma sub-região volta a gerar `#PF`; (c) após `release`, uma nova `reserve` de tamanho igual ou menor reaproveita o espaço virtual liberado (confirmado comparando a base de endereço retornada pelo console serial). Documentar os três resultados em design.md.
- [x] 4.4 Em `continue_boot` (`kernel/src/main.rs`), registrar a thread de `kernel::vmm_demo` no mesmo ponto onde as demais threads de demonstração já são iniciadas. Verificar com `cargo xtask test` que o boot continua passando sem regressão no console serial.
- [x] 4.5 Confirmar `cargo test -p kernel --lib` (suíte completa de host) sem regressão, incluindo todos os novos testes das tarefas 1.2, 2.1 e 2.2.
- [x] 4.6 Atualizar `docs/architecture/overview.md`: seção 2 (Memória), registrando a nova primitiva de reserva/comprometimento/descomprometimento/proteção/liberação, e o item 8 do roadmap (seção 15), registrando esta segunda sub-change como concluída (sem marcar o item 8 inteiro como concluído — ainda faltam `pthread_mutex`/`pthread_cond`, sinal de safepoint, relógio, sistema de arquivos, linking dinâmico, e o próprio porte do HotSpot). Verificar lendo o arquivo atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para as modificações propostas.
