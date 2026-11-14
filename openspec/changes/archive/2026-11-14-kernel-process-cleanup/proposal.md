# Proposal

## Why

Hoje, quando uma thread de modo usuário termina — por `SYSCALL_EXIT` (voluntário) ou
por uma exceção de CPU em modo usuário (involuntário, `kernel-fault-isolation`) —
nenhum recurso dela é liberado de volta ao sistema: o espaço de endereçamento
inteiro (tabelas de página + todos os frames físicos mapeados por ele) continua
"vivo" na memória para sempre. Com o isolamento de falha pronto, um processo pode
falhar repetidamente sem derrubar o kernel — mas cada falha consome memória física
que nunca mais volta, até o kernel ficar sem frames (`FrameError::OutOfFrames`) e
qualquer coisa que precise alocar memória passar a falhar. Este é o "Próximo passo
imediato" do `ROADMAP.md` depois de `kernel-fault-isolation`.

**Achado de pesquisa que define o escopo**: `kernel::phys::FrameAllocator` já tem um
`free()` real (lista encadeada, não um bump allocator) — a peça que falta não é a
primitiva de liberação em si, é **chamá-la** para cada frame do espaço de
endereçamento de uma thread terminada. Já `kernel::stack` (pool de stacks de
*kernel*) e as tabelas globais `kernel::pthread_handles`/`kernel::file_handles` não
rastreiam qual processo é dono de qual entrada — liberá-las exigiria primeiro
adicionar esse rastreamento, um trabalho estrutural à parte (já registrado no
`ROADMAP.md`, "Médio prazo": isolar essas tabelas por processo). Esta change se
limita ao recurso mais caro e mais imediatamente viável de recuperar: a memória
física do espaço de endereçamento.

## What Changes

- `kernel::paging::AddressSpace` ganha um método que percorre recursivamente suas
  próprias tabelas de página (só a metade inferior, índices de PML4 abaixo de
  `KERNEL_PML4_START` — a metade superior é compartilhada com todo o resto do
  kernel e nunca deve ser tocada) e devolve ao `kernel::phys::FRAME_ALLOCATOR`
  **todo frame físico presente**: cada página de usuário mapeada (código, stack,
  heap) e cada tabela intermediária (PDPT/PD/PT) que esse espaço de endereçamento
  possui — sem nunca tocar a própria PML4 nem qualquer entrada da metade superior.
- `kernel::scheduler::schedule_next` passa a chamar esse método, exatamente uma vez,
  no ponto em que uma thread marcada como terminada é definitivamente excluída da
  rotação (o mesmo ponto que já existe hoje, só sem nenhuma ação associada) — depois
  de `switch_address_space` já ter trocado para a próxima thread, nunca antes.
- Extensão do programa de teste já existente de `kernel-fault-isolation`
  (`userland_c/fault_test_program.c`/`/bin/fault.elf`) para a verificação: o kernel
  registra `kernel::phys::FRAME_ALLOCATOR.lock().free_frames()` antes de carregar
  esse processo e de novo logo depois do tick que o encerra, confirmando que a
  contagem de frames livres volta ao nível anterior (prova de que os frames
  voltaram de verdade, não só um "parece ter funcionado").

## Capabilities

### New Capabilities
(nenhuma)

### Modified Capabilities
- `kernel-processes`: novo requisito sobre liberação da memória física do espaço de
  endereçamento de uma thread/processo terminado — extensão direta do requisito de
  encerramento já adicionado por `kernel-fault-isolation` ("nunca mais executa"
  passa a vir acompanhado de "e sua memória volta a ficar disponível").

## Impact

- **Código afetado**: `kernel/src/paging.rs` (novo método em `AddressSpace`),
  `kernel/src/scheduler.rs` (chamada no ponto de evicção definitiva já existente),
  `userland_c/fault_test_program.c`/`kernel/src/main.rs` (instrumentação de
  verificação, sem mudar o comportamento do programa em si).
- **Não afetado**: `kernel::stack` (pool de stacks de kernel, continua sem
  reciclagem — Non-Goal já documentado), `kernel::pthread_handles`/
  `kernel::file_handles` (continuam vazando — precisam de rastreamento de dono
  primeiro, fora de escopo), `kernel::vmm` (idem), a própria `Process`/`Thread` nas
  tabelas de bookkeeping do escalonador (continuam existindo para sempre, mesma
  disciplina de "leak aceitável" já aceita — só a memória física cara é recuperada,
  não a contabilidade leve).
- **Verificação**: totalmente provável em boot real (mesmo programa de teste já
  existente de `kernel-fault-isolation`, só com uma medição a mais).
