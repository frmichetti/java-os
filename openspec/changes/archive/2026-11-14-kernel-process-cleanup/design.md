# Design

## Context

`kernel::phys::FrameAllocator` (`kernel/src/phys.rs`) já tem `alloc()`/`free()` reais
sobre uma lista encadeada (não um bump allocator) — `free_frames()`/`total_frames()`
já expõem contagem exata. `kernel::paging::Mapper::unmap(virt)` (`kernel/src/
paging.rs`) hoje só limpa a entrada folha (PTE) e invalida o TLB — nunca chama
`frame_allocator.free()` no frame que estava mapeado, e nunca libera uma tabela
intermediária (PDPT/PD/PT) que fique vazia. `AddressSpace` (`{ pml4_phys: u64 }`) não
tem `Drop` nem nenhum método de "liberar tudo". `kernel::scheduler::schedule_next`
já tem o ponto exato onde uma thread terminada é excluída definitivamente da
`ReadyQueue` (o `else` implícito do `if !take_block_intent(...) &&
!is_terminated(...) { push_back }`) — hoje sem nenhuma ação associada. Ver
proposal.md para a motivação completa e o porquê de `kernel::stack`/
`kernel::pthread_handles`/`kernel::file_handles`/`kernel::vmm` ficarem fora de
escopo.

**Achado de segurança que define o ponto de chamada**: `mark_terminated` (logo,
esta liberação) só é alcançável por duas vias — `sys_exit` (via `syscall`, que só
existe a partir de modo usuário) e `terminate_current_thread` chamada pelos
handlers de `#PF`/`#GP` **só quando `is_user_mode(frame.code_segment)`** (ver
`kernel-fault-isolation`). Uma thread de *kernel* (`spawn_sharing_current_
address_space` — `keyboard_demo`, `wait_demo`, etc.) nunca passa por nenhuma das
duas: ela nunca executa a instrução `syscall`, e um `#PF`/`#GP` causado por ela
nunca entra no ramo de modo usuário. **Logo, toda thread que chega a ser marcada
terminada tem, garantidamente, seu próprio `AddressSpace` privado** (nunca o
compartilhado da thread de boot) — liberar `contexts[was_running].address_space_root`
é sempre seguro, sem precisar consultar `Process`/`PROCESSES` para descobrir dono
nenhum.

## Goals / Non-Goals

**Goals:**
- Um novo método em `kernel::paging::AddressSpace` que percorre recursivamente as
  próprias tabelas de página (só índices de PML4 `0..KERNEL_PML4_START`) e devolve
  ao `FrameAllocator` todo frame presente — folhas e tabelas intermediárias.
- Chamar esse método a partir de `schedule_next`, exatamente uma vez por thread
  terminada, depois de `switch_address_space` já ter trocado para a próxima thread.
- Verificação real em boot: `free_frames()` antes/depois provando que os frames
  realmente voltam.

**Non-Goals:**
- Liberar a própria PML4 (o frame raiz do espaço de endereçamento) — fica retida de
  propósito (ver Decisions, "Por que a PML4 em si não é liberada").
- Reciclar o slot do pool de stacks de *kernel* (`kernel::stack`) — continua
  monotônico, Non-Goal já documentado desde que esse pool existe.
- Liberar entradas de `kernel::pthread_handles`/`kernel::file_handles`, ou janelas
  de `kernel::vmm` — nenhuma das três rastreia qual processo é dona de qual
  entrada/reserva hoje; adicionar esse rastreamento é trabalho estrutural à parte
  (`ROADMAP.md`, "Médio prazo": isolar essas tabelas por processo).
- Remover o `Process`/`Thread` das tabelas de bookkeeping do escalonador
  (`PROCESSES`/`CONTEXTS`/`TERMINATED`) — continuam existindo para sempre, mesma
  disciplina de "leak aceitável" já aceita para a contabilidade leve (bytes, não
  páginas de 4 KiB).
- Qualquer coordenação entre múltiplas threads do mesmo processo — nenhum processo
  de modo usuário tem mais de uma thread hoje (sem syscall de criação de thread em
  modo usuário), mesma limitação já documentada em changes anteriores.

## Decisions

### Liberação recursiva percorre só a metade inferior (`0..KERNEL_PML4_START`)

A metade superior da PML4 (índices `KERNEL_PML4_START..512`) é compartilhada por
cópia de valor entre toda `AddressSpace` (`AddressSpace::new`, já existente) — as
mesmas tabelas PDPT/PD/PT de lá são referenciadas por **todo processo**, inclusive
o próprio kernel. Percorrer e liberar qualquer coisa ali derrubaria memória do
kernel inteiro, não só do processo terminado. A liberação desta change nunca olha
para esses índices — exatamente a mesma fronteira que `Process::finish_new_user`/
`AddressSpace::new` já respeitam ao copiar essas entradas.

### Chamada depois de `switch_address_space(next.root)`, nunca antes

No instante em que `schedule_next` descobre que `was_running` terminou, `CR3`
**ainda** aponta para o próprio espaço de endereçamento de `was_running` (só muda
quando `switch_address_space` é chamado, mais abaixo na mesma função, para `next`).
Liberar frames de um espaço de endereçamento **tecnicamente ainda ativo em `CR3`**
seria, na melhor das hipóteses, uma fonte de confusão para qualquer leitor futuro —
mesmo sendo inofensivo em termos de execução real (liberar só marca o frame como
reaproveitável na lista encadeada do alocador; não apaga nem desmapeia nada que o
hardware ainda possa tentar acessar antes da próxima troca de `CR3`, e nenhuma
instrução roda entre a liberação e essa troca). **Decisão**: a chamada acontece
**depois** de `switch_address_space(next.root)`, eliminando qualquer ambiguidade —
quando a liberação roda, `CR3` já aponta para `next`, nunca para o espaço sendo
liberado. Mesmo padrão já usado por `Mapper::for_root` em `spawn_from_elf`/
`spawn_user_demo` (manipular as tabelas de um `AddressSpace` que não está
atualmente em `CR3`, via acesso físico-direto pela HHDM).

### Por que a PML4 em si não é liberada

O `Process` que possui este `AddressSpace` continua existindo para sempre em
`PROCESSES` (Non-Goal acima) — seu campo `address_space: AddressSpace { pml4_phys
}` continuaria **vivo**, apontando para um frame que, se fosse liberado aqui,
poderia ser realocado e sobrescrito por qualquer coisa completamente alheia a
partir da próxima alocação. Mesmo que nada hoje volte a ler esse campo depois do
encerramento, deixar um `Process` com um ponteiro fisicamente dangling é um convite
a um bug futuro sutil (ex.: uma função de diagnóstico futura que itere
`PROCESSES` e tente inspecionar `address_space_root`). **Decisão**: libera-se tudo
que a PML4 aponta (entradas `0..KERNEL_PML4_START`, recursivamente — PDPT/PD/PT e
folhas), mas a própria página da PML4 (4 KiB) fica retida — resíduo fixo e
pequeno, documentado, por processo terminado. **Alternativa rejeitada**: também
liberar a PML4 e aceitar o campo dangling — rejeitada por trocar um vazamento
pequeno e previsível por uma classe de bug (uso de memória já realocada) que só
se manifestaria se/quando alguém adicionar código que leia `PROCESSES` depois do
encerramento — exatamente o tipo de risco que vale a pena evitar de propósito
em vez de descobrir depois.

### Reaproveita `FrameAllocator::free` já existente, nenhuma primitiva nova no alocador

`kernel::phys::FrameAllocator::free(phys)` já existe, testado, e já rejeita
double-free (`FrameError::DoubleFree`) e endereço não gerenciado
(`FrameError::NotManaged`) — a liberação recursiva só precisa chamá-lo para cada
frame presente encontrado na caminhada, nenhuma mudança no alocador em si.

## Risks / Trade-offs

- **[Risco] Um bug na caminhada recursiva (ex.: um índice de nível errado) poderia
  liberar o frame errado, incluindo possivelmente algo da metade superior** →
  Mitigação: a caminhada nunca itera índices de PML4 fora de `0..KERNEL_PML4_START`
  — a mesma fronteira, testável em host sem hardware, que decide quais entradas
  copiar em `AddressSpace::new` (reaproveitando o mesmo raciocínio/constante já
  comprovado, `KERNEL_PML4_START`).
- **[Risco] Resíduo fixo de 4 KiB (a própria PML4) por processo terminado continua
  acumulando ao longo do tempo** → Aceito, documentado como Non-Goal — ordens de
  grandeza menor que o vazamento de hoje (todo o espaço de endereçamento completo);
  revisitar só se/quando `PROCESSES` ganhar algum mecanismo de remoção real.
- **[Risco] Programas que usaram `SYSCALL_VM_RESERVE`/`COMMIT` antes de terminar
  deixam suas páginas sem liberação explícita por `SYSCALL_VM_RELEASE`** → Não é um
  risco novo desta change: essas páginas, uma vez mapeadas no espaço de
  endereçamento do processo, SÃO cobertas pela caminhada recursiva (qualquer
  entrada presente sob `0..KERNEL_PML4_START` é liberada, independente de ter
  vindo de `SYSCALL_VM_COMMIT` ou de qualquer outro mapeamento) — a janela de
  `kernel::vmm` em si (a contabilidade de reservas/liberações, não as páginas
  físicas) é que continua sem relação com o processo, mas isso não impede a
  liberação física de acontecer.

## Migration Plan

Puramente aditivo — nenhum comportamento observável existente muda (uma thread
terminada continua nunca mais executando, exatamente como antes); o único efeito
novo é que a memória física dela volta a ficar disponível, em vez de permanecer
reservada para sempre. Nenhum passo de migração necessário.

## Resultado real da implementação

`AddressSpace::free_user_mappings` foi implementada como função associada (recebe
`root: u64`, não `&self`/`AddressSpace` — `pml4_phys` é campo privado, e o
escalonador só guarda o `u64` de `Context::address_space_root` por thread, nunca
uma `AddressSpace` inteira) e chamada de `schedule_next` exatamente como planejado,
depois de `switch_address_space`.

**Achado real durante a verificação em boot real**, que exigiu redesenhar a tarefa
2.2 (estratégia de medição, não o mecanismo de liberação em si): a primeira
tentativa de verificação (medir `free_frames()` antes de carregar `/bin/fault.elf`
e de novo imediatamente antes de `idle_loop()`) travou o boot inteiro — *deadlock*,
já que esse segundo ponto ainda estava dentro do escopo que já detém
`frame_allocator` travado (`kernel::phys::FRAME_ALLOCATOR.lock()`, um `Spinlock`
não reentrante) desde bem antes em `continue_boot`. Corrigido usando a variável já
travada em vez de travar de novo. Mas, mesmo corrigido, essa medição revelou um
problema mais profundo: a contagem **caiu** entre os dois pontos (50884 → 50680),
o oposto do esperado — porque **nada é de fato escalonado antes de `idle_loop()`
chamar `sti` pela primeira vez** (confirmado: nenhum `asm!("sti")` existe em
`kernel/src/main.rs` antes desse ponto) — toda a sequência de `continue_boot`
roda com interrupções desabilitadas, então nenhuma troca de contexto real
acontece, nenhuma thread chega a executar uma única instrução, e a falta/limpeza
de `/bin/fault.elf` só acontece bem depois, já em plena rotação pós-`sti`. A queda
observada era só o custo fixo de registrar os demais processos/threads de teste
(stacks de kernel, áreas de FPU, slots de `syscall`), não evidência de limpeza
alguma ainda não ter ocorrido.

**Correção**: a medição "depois" foi movida para dentro do próprio ponto de
limpeza em `schedule_next` — `free_frames()` imediatamente antes e depois de
`AddressSpace::free_user_mappings`, dentro da mesma seção crítica
(`with_interrupts_disabled`), eliminando qualquer possibilidade de outra alocação
acontecer entre as duas leituras. Essa medição, muito mais direta, acabou provando
bem mais do que o planejado: todo processo que termina durante o boot (não só o
encerramento involuntário de `/bin/fault.elf` — também o encerramento voluntário
via `SYSCALL_EXIT` de `test.elf`/`libctest.elf`/`cxxtest.elf`/`javaosvm.elf`/
`jpthread.elf`/`libm.elf`/`timedw.elf`) libera corretamente um número positivo e
plausível de frames (8 a 20, compatível com o tamanho real de cada ELF/stack):
```
fault_isolation: frames livres antes de /bin/fault.elf: 50884
kernel_process_cleanup: thread 4 liberou 8 frames (livres: 50675 -> 50683)
kernel_process_cleanup: thread 9 liberou 11 frames (livres: 50672 -> 50683)
kernel_process_cleanup: thread 12 liberou 20 frames (livres: 50683 -> 50703)
kernel_process_cleanup: thread 8 liberou 13 frames (livres: 50703 -> 50716)
EXCECAO #PF (page fault): endereco=0x0 ... modo_usuario=true - thread encerrada
kernel_process_cleanup: thread 14 liberou 11 frames (livres: 50716 -> 50727)
kernel_process_cleanup: thread 13 liberou 11 frames (livres: 50727 -> 50738)
kernel_process_cleanup: thread 11 liberou 11 frames (livres: 50739 -> 50750)
kernel_process_cleanup: thread 10 liberou 11 frames (livres: 50750 -> 50761)
```
Sem nenhuma regressão: `smoke test de boot: PASSOU`, `cargo test -p kernel --lib`:
271 testes passando.
