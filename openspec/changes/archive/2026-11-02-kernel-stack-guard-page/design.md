# Design

## Context

Hoje `kernel_main` roda do início ao fim sobre a stack que o Limine monta antes de entregar o controle ao kernel — não existe nenhuma stack própria. `gdt::init()` já monta uma stack estática separada só para o handler de double fault (IST1, `DOUBLE_FAULT_STACK`, 16 KiB), mas isso não protege a stack principal: um estouro nela corrompe memória adjacente silenciosamente antes de qualquer falha ser sinalizada (ver `KNOW_ISSUES.md` — "Ausência de guard page na stack principal do kernel" — e `proposal.md` — Why).

Com `kernel-memory-management` já implementada, o kernel tem: um alocador de frames físicos (`kernel::phys::FRAME_ALLOCATOR`) utilizável assim que `FRAME_ALLOCATOR.lock().init(...)` retorna `Ok`, e um `Mapper` (`kernel::paging::Mapper`) capaz de mapear páginas virtuais de 4 KiB sob a paginação de 4 níveis já ativa (montada pelo Limine), criando tabelas intermediárias sob demanda via esse mesmo alocador. Essas duas peças são exatamente o que falta combinar para mapear uma stack própria com uma guard page.

## Goals / Non-Goals

**Goals:**
- Mapear uma stack dedicada do kernel (64 KiB, tamanho fixo) numa faixa virtual reservada e isolada, com a página imediatamente abaixo do seu limite inferior deliberadamente não mapeada.
- Trocar `RSP` para essa nova stack de forma segura, assim que o alocador de frames e a paginação estiverem prontos, sem corromper o estado de execução de `kernel_main`.
- Fechar o know issue registrado, para toda a execução a partir do ponto da troca.

**Non-Goals (nesta change):**
- Crescimento dinâmico da stack (sob demanda, ao estilo do heap) — tamanho fixo de 64 KiB, decidido já no `proposal.md`.
- Múltiplas stacks (por thread/processo) ou qualquer suporte a SMP — o kernel continua single-core, com uma única stack de execução principal.
- Qualquer mudança no mecanismo de double fault/IST já existente (`gdt::DOUBLE_FAULT_STACK`) — ele continua separado, estático, e fora do escopo desta guard page.
- Detecção de overflow por outros meios (stack canaries, `-Z stack-protector`, etc.) — a guard page via paginação é o único mecanismo desta change.
- Proteção contra um único frame de função maior que uma página (4 KiB) que "pule" por cima da guard page inteira sem nunca escrever nela — ver Riscos.

## Decisions

### Faixa virtual reservada para a stack
A stack do kernel vive numa faixa própria, isolada de qualquer outro uso: base em `0xffff_a000_0000_0000` (índice 320 do PML4), distante da HHDM (índice 256), do heap (`heap::HEAP_START`, índice 288) e do código do kernel (índice 511). Mapeia-se `KERNEL_STACK_SIZE = 64 KiB` (16 páginas de 4 KiB) a partir dessa base; `RSP` inicial aponta para o topo (base + tamanho). A página em `base - PAGE_SIZE` nunca é mapeada — essa é a guard page.

**Por que um PML4 inteiro vago ao redor, e não só uma página de guarda isolada:** como essa faixa (320) não é usada por mais nada, na prática existe um intervalo de 512 GiB inteiramente não mapeado antes da base da stack, não apenas uma página de 4 KiB. Isso dá margem extra contra o caso de uma única função consumir mais de uma página de stack de uma vez (ver Riscos) sem exigir nenhum mecanismo adicional — é uma consequência gratuita de reservar a faixa num PML4 isolado, não um requisito novo.

### Mecanismo de troca de stack: handoff via `asm!` com `jmp`, sem retorno
`kernel_main` é dividido em duas partes: a primeira (GDT/IDT, serial, HHDM, banner, memmap, alocador de frames) continua rodando na stack do Limine, pois depende de nada mais existir ainda. Assim que `FRAME_ALLOCATOR.init()` retorna `Ok`, antes de `heap::init()`, o kernel mapeia a nova stack e salta para `continue_boot: extern "C" fn() -> !`, uma função que engloba o restante de `kernel_main` (heap, x2APIC, timer, `idle_loop`).

O mecanismo é exposto em `kernel::stack` como duas funções separadas, não uma só:
1. `map(frame_allocator) -> Result<u64, StackInitError>`: mapeia as 16 páginas da nova stack via `Mapper` (usa o `frame_allocator` recebido) e retorna o endereço do topo.
2. `switch_to(stack_top, continuation) -> !`: calcula `new_rsp = stack_top - 8` (ver alinhamento abaixo) e executa `asm!("mov rsp, {0}", "jmp {1}", in(reg) new_rsp, in(reg) continuation, options(noreturn))`.

Como `continuation` nunca retorna (`-> !`, termina em `idle_loop()`), um `jmp` direto é suficiente e mais simples que simular um `call` — não há necessidade de endereço de retorno, e nenhuma rotina volta a usar a stack antiga depois do `jmp`. Isso elimina a classe de bug "retornar para uma função cujo frame estava na stack antiga" por construção, em vez de depender de disciplina do chamador.

**Por que duas funções, e não uma só `init_and_switch(frame_allocator, continuation)`:** essa foi a primeira versão implementada, e falhou em boot real (confirmado via GDB: o kernel trava em deadlock, girando para sempre em `Spinlock::lock` dentro de `continue_boot`). Causa: em `kernel_main`, a chamada era `stack::init_and_switch(&mut FRAME_ALLOCATOR.lock(), continue_boot)` — o `MutexGuard` temporário criado por `.lock()` vive, pelas regras de extensão de tempo de vida de temporários em Rust, até o fim da instrução que contém a chamada. Como a função chamada nunca retorna (`options(noreturn)`, `jmp` em vez de `call`/`ret`), esse "fim da instrução" nunca é alcançado, e o destrutor do guard (que liberaria o spinlock) nunca roda — o lock fica preso para sempre, e a primeira tentativa de `continue_boot` de travar o mesmo `FRAME_ALLOCATOR` (para logar a guard page) gira indefinidamente. Qualquer guard/RAII cujo `Drop` dependa de um retorno normal da função chamadora sofre do mesmo problema ao atravessar um `jmp` divergente — não é específico deste spinlock.

Separar em `map`/`switch_to` resolve isso ao mover a responsabilidade de liberar o lock para `kernel_main`, via escopo normal de bloco (`{ let mut g = FRAME_ALLOCATOR.lock(); ...; }`, que libera `g` ao sair do bloco) executado inteiramente *antes* da chamada a `switch_to` — nenhum guard fica vivo no momento do `jmp`. `switch_to` em si não toca o alocador de frames, então não há tentação de repetir o erro.

**Alinhamento:** a ABI x86-64 (System V) exige `RSP % 16 == 0` imediatamente antes de qualquer `call`, logo `RSP % 16 == 8` na entrada de uma função chamada normalmente (depois do `call` empilhar o endereço de retorno de 8 bytes). Como `init_and_switch` faz um `jmp` (não um `call`) para dentro de `continue_boot`, ele precisa produzir manualmente o mesmo estado que um `call` teria deixado, para que o prólogo de `continue_boot` e qualquer `call` subsequente all feitos a partir dele permaneçam corretamente alinhados. Por isso `new_rsp = base + KERNEL_STACK_SIZE - 8`, e não `base + KERNEL_STACK_SIZE`.

**Alternativa descartada:** trocar a stack dentro da própria `kernel_main` sem dividi-la em duas funções (ex.: só mudar `RSP` no meio do corpo e continuar executando o mesmo `fn`). Rejeitada porque o compilador pode manter em registradores ou na stack antiga qualquer estado calculado antes da troca (variáveis locais, otimizações do otimizador), e não há garantia de que o restante do corpo da função não referencie endereços relativos à stack antiga. Separar em duas `fn` distintas, conectadas por um `jmp` explícito sem estado implícito compartilhado (`continue_boot` não recebe nenhum argumento que dependa da stack antiga — tudo que precisa, como os estados de `phys`/`heap`, já vive em `static`s globais), é a forma de evitar essa classe de bug.

### Verificação de "página não mapeada": função de consulta somente-leitura em `paging`
A spec exige confirmar que o endereço logo abaixo da stack não está mapeado. Hoje `paging::Mapper` só expõe `map`/`map_new`/`unmap` (este último muta o estado se encontrar um mapeamento). Esta change adiciona `Mapper::translate(&self, virt: u64) -> Option<u64>` — percorre PML4→PDPT→PD→PT sem criar nem remover nenhuma tabela, retornando `None` assim que encontrar uma entrada ausente em qualquer nível, ou `Some(phys)` se a página final estiver presente. Usada tanto para logar a verificação da guard page no banner de boot (mesmo padrão de `memmap_report`) quanto nos testes unitários de `kernel::paging`.

**Alternativa descartada:** reaproveitar `unmap` para "verificar" (chamando e ignorando o resultado). Rejeitada por mutar o estado real das tabelas de página como efeito colateral de uma simples consulta — inaceitável mesmo em um log de boot.

### Ponto de troca: imediatamente após o alocador de frames, antes do heap
A proposta já fixa "logo após o alocador de frames físicos e a paginação estarem prontos". Nenhuma etapa entre a inicialização do alocador de frames e a chamada a `heap::init()` depende de `heap`, então a troca acontece nesse ponto exato — maximiza a porção do boot protegida pela guard page (o próprio `heap::init()`, que já mapeia páginas e pode falhar, passa a rodar sobre a stack protegida).

### Page fault também precisa de stack dedicada (IST)
Descoberto durante a implementação, não antecipado no `proposal.md` original: provocar um estouro real de stack (tarefa 4.2) resultava em `#DF` direto, sem nenhum `#PF` jamais ser reportado — violando o requisito desta própria change ("Estouro de Stack Falha Imediatamente", `specs/kernel-stack-protection/spec.md`) de que o resultado seja um `#PF` referenciando a guard page, não um `#DF`.

**Causa:** o handler de `#PF` (`idt.rs`) estava registrado com `ist_index = 0` (usa a stack corrente). Quando a recursão já moveu `RSP` para dentro/além da guard page no instante da falta, a própria CPU não consegue empilhar o frame de exceção do `#PF` nessa stack indisponível — por definição do x86_64, uma falha durante a entrega de outra exceção escala para `#DF`. O `#DF` só é entregável porque já usa uma IST dedicada (`DOUBLE_FAULT_IST_INDEX`); o `#PF`, sem o mesmo tratamento, nunca chega a rodar seu handler quando a stack já estourou — exatamente o cenário que esta change existe para diagnosticar.

**Decisão:** dar ao handler de `#PF` sua própria stack via IST, no mesmo padrão já estabelecido para `#DF` (`gdt::DOUBLE_FAULT_STACK`/`DOUBLE_FAULT_IST_INDEX`): uma nova stack estática (`PAGE_FAULT_STACK`, 16 KiB, mesmo tamanho da de double fault) referenciada por uma nova entrada de IST (`PAGE_FAULT_IST_INDEX = 2`) no TSS, e o handler de `#PF` em `idt.rs` passa a ser registrado com esse índice em vez de `0`. O comportamento observável do handler não muda (mesmo formato de log, mesma decisão de haltar) — só o mecanismo de entrega da exceção passa a funcionar de forma confiável quando a stack corrente já estourou.

Isso é uma modificação em `kernel-interrupts` (capability já implementada e arquivada), registrada como tal em `proposal.md` — Modified Capabilities e em `specs/kernel-interrupts/spec.md` (delta MODIFIED nesta change), já que sem ela o requisito central desta change (`kernel-stack-protection`) não é satisfazível em hardware real.

**Alternativa descartada:** relaxar o cenário da spec `kernel-stack-protection` para aceitar `#DF` como desfecho também válido (já que, nos testes, o `#DF` reportou de forma limpa e sem travar). Descartada porque o mecanismo de IST para o `#PF` é uma correção padrão e barata (mesmo padrão já existente para `#DF`, reaproveitando a mesma infraestrutura de TSS/IST), e resolve o problema na raiz em vez de aceitar um diagnóstico menos preciso (um `#DF` não traz o endereço que causou a falta original, apenas confirma que *alguma* exceção aninhada ocorreu).

## Riscos / Trade-offs

- **[Risco] Erro no cálculo de `new_rsp` ou no `asm!` do handoff corrompe a execução imediatamente após o `jmp`, sem nenhuma rede de segurança do compilador** → Mitigação: validar o boot normal end-to-end (banner, relatório de heap, inicialização do APIC/timer, todos depois da troca) e inspecionar `RSP` via GDB logo após o `jmp` para confirmar que cai dentro do intervalo esperado da nova stack, antes de considerar a tarefa concluída — mesmo padrão de verificação manual já usado para o double fault em `kernel-interrupts-timer`.
- **[Risco] Uma função que consuma mais de uma página (4 KiB) de stack em uma única vez pode, em tese, escrever além da guard page sem nunca tocar nela** → Mitigação: aceito como trade-off residual (nenhum código atual do kernel aloca tanto de uma vez); a faixa virtual isolada escolhida (ver Decisões) garante que, mesmo nesse caso extremo, o próximo acesso ainda cai em memória não mapeada (não há nada mapeado logo depois da guard page), então o pior caso ainda resulta em falha de página, só que potencialmente mais de uma página abaixo do limite.
- **[Trade-off] Stack de tamanho fixo (64 KiB) em vez de crescimento sob demanda** → Aceitável porque é o que a proposta já pede explicitamente; se o kernel precisar de mais stack no futuro (ex.: recursão mais profunda em código novo), o valor de `KERNEL_STACK_SIZE` pode ser simplesmente aumentado sem mudar o mecanismo.
- **[Trade-off] `Mapper::translate` adiciona uma nova forma pública de inspecionar o estado de paginação** → Superfície pequena e somente leitura; mantém a mesma convenção de walk manual já usada internamente por `map`/`unmap`.

## Migration Plan

Não há estado em produção. "Deploy" continua significando: o smoke test de boot (`cargo xtask test`) passa com o kernel rodando sobre a nova stack a partir do ponto de troca (confirmável pelo log de boot reportando o endereço da guard page e o resultado de `translate` sobre ela), e os testes unitários novos/existentes de `kernel::paging` (incluindo `translate`) passam. Como verificação adicional pontual (não automatizada, mesmo padrão já usado para validar double fault em `kernel-interrupts-timer`), provocar um estouro real de stack (recursão profunda) e confirmar via log/GDB que o `#PF` resultante referencia um endereço dentro da guard page, antes de qualquer corrupção de memória fora dela. Rollback é reverter os commits desta change; nenhum dado persistente é afetado, e fecha-se (ou reabre-se, em caso de reverter) a entrada correspondente em `KNOW_ISSUES.md`.
