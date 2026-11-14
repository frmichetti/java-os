# Tasks

## 1. Liberação recursiva de `AddressSpace`

- [x] 1.1 Adicionar `AddressSpace::free_user_mappings<S: FrameStore>(&self,
      frame_allocator: &mut FrameAllocator<S>)` a `kernel/src/paging.rs`: percorre
      recursivamente as 512 entradas da PML4 nos índices `0..KERNEL_PML4_START`
      (nunca a metade superior); para cada entrada presente, desce recursivamente
      (PDPT → PD → PT → folha), chamando `frame_allocator.free(...)` em cada frame
      presente encontrado — folhas e tabelas intermediárias — depois de terminar de
      descer nelas (nunca libera uma tabela antes de ter liberado tudo que ela
      aponta). Nunca toca a própria PML4 (`self.root()`) — ver design.md, "Por que a
      PML4 em si não é liberada". Mesmo acesso a tabelas via HHDM já usado por
      `Mapper`/`AddressSpace::new` (`phys_to_table_ptr`) — verificar que `cargo
      kernel-build` continua compilando. **Mesma limitação de testabilidade já
      documentada para `Mapper::map`/`unmap`/`translate`** (comentário em
      `kernel/src/paging.rs`, `mod tests`): a caminhada real de tabelas só é
      verificável em boot real, não em teste de host (precisa de `CR3`/HHDM reais).
      **Achado real**: a assinatura final recebe `root: u64` em vez de `&self` —
      `pml4_phys` é um campo privado de `AddressSpace`, e o escalonador só guarda o
      `u64` de `Context::address_space_root` por thread, nunca uma `AddressSpace`
      inteira (cuja posse é do `Process`); vira uma função associada
      (`AddressSpace::free_user_mappings(root, &mut frame_allocator)`), não um
      método. `cargo test -p kernel --lib`: 271 testes passando.
- [x] 1.2 Chamar `AddressSpace::free_user_mappings` a partir de
      `kernel::scheduler::schedule_next`, no ramo que já existe hoje para uma thread
      terminada (o `else` implícito de `if !take_block_intent(...) &&
      !is_terminated(...) { push_back }`) — **depois** de `switch_address_space`
      já ter trocado para `next` (nunca antes, ver design.md). Usa
      `contexts[was_running.0 as usize].address_space_root` diretamente (nunca
      precisa consultar `Process`/`PROCESSES` — ver design.md, "Achado de
      segurança que define o ponto de chamada") e
      `kernel::phys::FRAME_ALLOCATOR.lock()` — verificar com `cargo test -p kernel
      --lib` que os 271 testes existentes continuam passando (nenhum teste de host
      exercita este caminho, já que depende de hardware real — mesma limitação da
      tarefa 1.1) e com `cargo kernel-build` que compila.

## 2. Verificação em boot real

- [x] 2.1 Em `kernel/src/main.rs`, imediatamente antes de carregar `/bin/fault.elf`
      (já existente, `kernel-fault-isolation`), reportar
      `kernel::phys::FRAME_ALLOCATOR.lock().free_frames()` pelo canal de saída de
      boot (ex.: `"frames livres antes de /bin/fault.elf: {}"`). **Achado real,
      corrigido durante a implementação**: a primeira tentativa travou o boot
      (*deadlock*) — este ponto ainda está dentro do escopo que já detém
      `frame_allocator` travado (`FRAME_ALLOCATOR.lock()`, não reentrante);
      corrigido usando a variável `frame_allocator` já travada em vez de travar de
      novo.
- [x] 2.2 ~~Imediatamente antes de `idle_loop()`~~ — **redesenhado durante a
      implementação** (ver design.md, "Resultado real da implementação"): nada é
      de fato escalonado antes de `idle_loop()` chamar `sti` pela primeira vez
      (interrupções ficam desabilitadas durante toda a sequência de `continue_boot`
      — confirmado: nenhum `sti` existe em `kernel/src/main.rs` antes dali), então
      medir "antes"/"depois" nesses dois pontos nunca captura a falta/limpeza real
      (que só acontece bem depois, já em plena rotação). Em vez disso, a medição
      "depois" foi movida para dentro do próprio ponto de limpeza em
      `kernel::scheduler::schedule_next` (tarefa 1.2): `free_frames()` imediatamente
      antes e depois de `AddressSpace::free_user_mappings`, reportado por
      `serial_println!` — mede exatamente a liberação desta thread, sem nenhuma
      outra atividade de alocação possível entre as duas leituras (interrupções já
      desabilitadas no escopo de `with_interrupts_disabled`).
- [x] 2.3 Verificar via `cargo xtask test` em QEMU que a liberação de cada thread
      terminada (tarefa 2.2) reporta um ganho positivo de frames livres, e que
      nenhum programa de teste já existente perdeu o resultado de antes.
      **Confirmado**: o log mostra a liberação de toda thread que termina durante o
      boot (voluntária por `SYSCALL_EXIT`, não só o encerramento involuntário de
      `/bin/fault.elf`), cada uma com um ganho positivo e plausível (8 a 20 frames,
      compatível com o tamanho real de cada ELF/stack/página de usuário):
      ```
      fault_isolation: frames livres antes de /bin/fault.elf: 50884
      kernel_process_cleanup: thread 4 liberou 8 frames (livres: 50675 -> 50683)
      kernel_process_cleanup: thread 9 liberou 11 frames (livres: 50672 -> 50683)
      kernel_process_cleanup: thread 12 liberou 20 frames (livres: 50683 -> 50703)
      kernel_process_cleanup: thread 8 liberou 13 frames (livres: 50703 -> 50716)
      EXCECAO #PF (page fault): ... modo_usuario=true - thread encerrada
      kernel_process_cleanup: thread 14 liberou 11 frames (livres: 50716 -> 50727)
      kernel_process_cleanup: thread 13 liberou 11 frames (livres: 50727 -> 50738)
      kernel_process_cleanup: thread 11 liberou 11 frames (livres: 50739 -> 50750)
      kernel_process_cleanup: thread 10 liberou 11 frames (livres: 50750 -> 50761)
      ```
      Nenhuma regressão: `smoke test de boot: PASSOU`, e `cargo test -p kernel
      --lib`: 271 testes passando.

## 3. Documentação

- [x] 3.1 Adicionar a este change (`design.md`, "Resultado real da implementação")
      a confirmação do que foi verificado em boot real (as duas contagens de
      frames livres) e o relato de qualquer achado real durante a implementação.
- [x] 3.2 Atualizar `ROADMAP.md` — mover "Liberação real de recursos do processo
      encerrado" para a seção de implementado (deixando claro que só a memória
      física do espaço de endereçamento foi coberta — `pthread_handles`/
      `file_handles`/`kernel::vmm`/o pool de stacks de kernel continuam vazando,
      por falta de rastreamento de dono), e apontar o próximo item real de médio
      prazo como o próximo passo — verificar que o documento permanece consistente.
      **Decisão**: "Próximo passo imediato" passa a apontar para "Isolar as
      tabelas de handle hoje globais... por processo" (promovido de "Médio prazo",
      removido de lá para não duplicar) — pré-requisito direto para liberar essas
      entradas, agora que a memória física já é recuperada.

## Workflow follow-up

- Arquivar esta change (`openspec archive kernel-process-cleanup`) depois que a
  verificação em boot real (tarefas 2.1-2.3) e `cargo test -p kernel --lib`
  estiverem confirmadas.
