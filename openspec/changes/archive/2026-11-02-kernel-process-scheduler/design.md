# Design

## Context

Hoje o kernel é um único fluxo sequencial: `kernel_main` → `continue_boot` → `idle_loop()` para sempre, sobre uma única stack (a stack dedicada com guard page de `kernel-stack-guard-page`, em `0xffff_a000_0000_0000`). As peças que este design combina já existem e funcionam:
- `kernel::phys`/`kernel::paging`: alocador de frames e `Mapper` que caminha/edita a paginação de 4 níveis atualmente ativa (sempre via `read_cr3()`).
- `kernel::idt`/`kernel::gdt`: IDT própria com handlers `extern "x86-interrupt"`, e duas stacks dedicadas via IST (double fault, page fault) para entrega confiável de exceções mesmo com a stack corrente comprometida.
- `kernel::timer`: timer do Local APIC calibrado, disparando a 100 Hz, com um handler `extern "x86-interrupt"` que só incrementa um contador de ticks e manda EOI.
- `kernel::stack`: mapeia UMA stack de 64 KiB com guard page, numa faixa virtual fixa isolada.
- `kernel::heap`: heap global (`alloc::*` disponível), crescendo sob demanda.

Ver `proposal.md` — Why/What Changes para a motivação e o escopo; `specs/kernel-processes`, `specs/kernel-scheduler`, e os deltas de `specs/kernel-paging`/`specs/kernel-stack-protection` para os requisitos.

## Goals / Non-Goals

**Goals:**
- Processos com espaço de endereçamento próprio (nova PML4, mapeamento higher-half do kernel preservado).
- Threads como unidade de escalonamento, cada uma com sua própria stack de kernel protegida por guard page e seu próprio contexto de registradores.
- Escalonador preemptivo round-robin, disparado pelo tick do timer já existente, sem exigir cooperação da thread em execução.
- Reaproveitar o contexto de execução já em andamento (a stack dedicada de `kernel-stack-guard-page`, sobre a qual `continue_boot` já roda) como a primeira thread do sistema, em vez de criar uma thread "idle" separada do zero.

**Non-Goals (nesta change):**
- Modo usuário/anel 3, `syscall`/`sysret`, ou qualquer transição de privilégio — tudo continua em anel 0. Fica para a change futura de "System Calls" (roadmap item 5).
- Carregamento de executáveis (ELF) — threads/processos são sempre criados a partir de uma função Rust do próprio kernel.
- Encerramento/limpeza de threads ou processos (`exit`, reaproveitamento de stack/espaço de endereçamento) — threads criadas nesta change rodam indefinidamente; destruir uma thread fica para uma iteração futura.
- Múltiplos núcleos (SMP) — roadmap item 12, explicitamente fora de escopo; a implementação não precisa ser SMP-safe além do já estabelecido (`Spinlock` existente), mas também não precisa resolver sincronização multi-core real.
- Primitivas de sincronização entre threads expostas (mutex, semáforo, canal) além do lock interno do próprio escalonador — ficam para quando houver consumidores reais.
- Prioridades ou múltiplas filas de prontos — apenas round-robin de uma única fila.
- Crescimento dinâmico da stack de uma thread — tamanho fixo, mesmo valor já usado pela stack principal (64 KiB).

## Decisions

### A thread "boot" é o próprio `continue_boot`, não uma thread idle separada
Em vez de criar uma thread idle do zero, o contexto que já está executando `continue_boot` (sobre a stack dedicada de `kernel-stack-guard-page`) é registrado como a primeira thread do escalonador (`ThreadId(0)`) no momento em que o escalonador é iniciado — depois do timer já estar configurado, antes de qualquer `sti`. Quando essa thread for preemptada pela primeira vez, seu contexto é salvo exatamente como o de qualquer outra; quando não houver mais nenhuma outra thread pronta, ela volta a ser escalonada e, ao chegar de novo em `idle_loop()`, mantém exatamente o comportamento de halt ocioso já exigido por `kernel-boot`.

**Alternativa descartada**: criar uma thread idle nova (stack própria, função `fn idle() -> ! { idle_loop() }`), deixando `continue_boot` terminar e nunca mais existir como contexto de execução. Rejeitada por desperdiçar a stack que `continue_boot` já tem mapeada e por introduzir uma assimetria sem necessidade (duas formas de "thread que não faz nada", quando uma só já resolve).

### Troca de contexto: ponto de entrada do timer em assembly puro, não `extern "x86-interrupt"`
O handler atual do timer (`timer_interrupt_handler`, `extern "x86-interrupt"`) só incrementa um contador — a ABI `x86-interrupt` do Rust não expõe os registradores de uso geral (RAX-R15) para o código Rust, então não há como salvar/restaurar um contexto completo de thread a partir dela. O vetor do timer passa a apontar para uma rotina escrita em assembly (`core::arch::naked_asm!`/`global_asm!`, consistente com o padrão já usado em `kernel::stack::switch_to` e nos `asm!` de `gdt.rs`/`paging.rs`) que:
1. Empilha todos os registradores de uso geral na stack corrente (a da thread que estava rodando).
2. Chama uma função Rust (`scheduler::on_timer_tick(current_rsp: u64) -> u64`) passando o `RSP` atual — essa função faz o que o handler antigo já fazia (incrementa o contador de ticks, manda EOI) e decide a próxima thread a rodar (ver próxima decisão), salvando `current_rsp` no `Context` da thread interrompida e devolvendo o `rsp` salvo da próxima thread.
3. Troca `RSP` para o valor devolvido.
4. Desempilha os registradores de uso geral — agora da stack da nova thread, exatamente como foram salvos da última vez que ela foi interrompida (ou como foram sintetizados na criação da thread, ver próxima decisão).
5. `iretq` — o frame de interrupção empilhado pela CPU na entrada já está, para a thread retomada, com os valores corretos (os que a CPU empilhou da última vez que ela foi interrompida, ou os valores sintéticos da criação).

**Precisão sobre o tamanho do frame de hardware (achado durante a implementação, com uma correção importante feita só depois de testar em hardware real):** a primeira versão desta seção presumia, por analogia com o modo protegido de 32 bits, que sem IST e sem troca de privilégio a CPU empilharia apenas 3 campos (RIP, CS, RFLAGS). **Isso está errado para o modo 64 bits (long mode):** testado em boot real (QEMU + GDB), `iretq` em modo 64 bits sempre lê os 5 campos completos (RIP, CS, RFLAGS, RSP, SS), mesmo sem troca de privilégio e sem IST — ao contrário do modo protegido legado. Com um frame sintético de apenas 3 campos, `iretq` lia 8 bytes além do fim da stack mapeada da thread, causando um `#PF` na própria instrução `iretq` (confirmado via GDB: `RSP` correto, os 3 campos corretos, mas o quarto/quinto campo que `iretq` insiste em ler caía exatamente na guard page do próximo slot do pool de stacks). O trap frame sintético de uma thread nova (`kernel::process::init_stack`) usa os 5 campos; o campo `RSP` do frame é, inclusive, conveniente: ele é o valor que efetivamente vira o `RSP` de `entry`, então o ajuste de alinhamento de ABI (`-8`, ver abaixo) é aplicado diretamente nesse campo, em vez de depender de quantos bytes sobram depois do `iretq`.

**Alinhamento de `RSP` antes do `call` (achado durante a implementação):** uma interrupção pode chegar com `RSP` em qualquer alinhamento — o código interrompido pode estar em qualquer ponto de sua execução, não necessariamente com `RSP % 16 == 0`. Como a ABI System V exige exatamente esse alinhamento imediatamente antes de um `call` (para que o prólogo/chamadas internas de `on_timer_tick` funcionem corretamente), a rotina de entrada força `and rsp, -16` depois de capturar `current_rsp` (em `rdi`, o primeiro argumento) e antes do `call` — o valor pré-alinhamento é descartado sem problema, já que a stack é trocada para a da próxima thread logo em seguida (nunca precisamos "desfazer" o alinhamento).

Isso significa que a stack de kernel de cada thread guarda, entre trocas de contexto, exatamente o "trap frame" que a entrega de interrupção deixaria — o mesmo padrão usado por kernels de referência (xv6, Linux `switch_to`). `idt::set_handler` continua funcionando sem mudanças: recebe o endereço do símbolo assembly em vez do endereço de uma `fn` Rust.

**Alternativa descartada**: manter o handler `extern "x86-interrupt"` e tentar fazer a troca de contexto só com os registradores callee-saved (RBX, RBP, R12-R15) que uma função Rust comum preserva, deixando os caller-saved para a convenção da ABI. Rejeitada por depender de garantias sutis e não documentadas de como o LLVM gera código para `extern "x86-interrupt"` (ABI ainda instável no Rust) — salvar todos os registradores explicitamente em assembly à mão é mais verboso, mas auditável e sem dependência de comportamento não especificado do compilador, consistente com a filosofia já estabelecida do projeto (paginação, GDT/IDT escritos à mão).

### Criação de thread: sintetizar um "trap frame" inicial na stack nova
Para uma thread nunca executada, a stack recém-alocada é preenchida, no momento da criação, com um frame sintético equivalente ao que o passo 5 acima espera: RIP = endereço da função de entrada, CS = `KERNEL_CODE_SELECTOR`, RFLAGS = interrupções habilitadas (bit IF), RSP/SS apontando para o topo dessa mesma stack, e todos os registradores de uso geral zerados. Na primeira vez que o escalonador a escolhe, o caminho de restauração é idêntico ao de uma thread já existente — nenhum caso especial de "primeira execução" no código de troca de contexto.

### Espaço de endereçamento por processo: nova PML4 com higher-half copiado por referência
`kernel::paging` ganha `AddressSpace`: aloca um frame para uma nova PML4, zera-o, e copia as entradas de índice 256-511 (HHDM, heap, stack pool de threads — ver abaixo, código do kernel) da PML4 ativa no momento da criação. Como uma entrada de PML4 só guarda o endereço físico de uma tabela de nível inferior, essa cópia é por referência: todas as address spaces compartilham as mesmas tabelas (e portanto o mesmo mapeamento) para tudo que é "kernel", e cada uma tem sua própria metade inferior (índices 0-255), inicialmente vazia — é aí que mapeamentos específicos de um processo (hoje, nada; no futuro, memória de aplicação/usuário) entrariam, sem afetar nenhum outro processo. Trocar de address space é escrever `CR3` com o endereço físico dessa nova PML4.

`Mapper` ganha a capacidade de operar sobre uma PML4 explícita (um novo construtor, mantendo `Mapper::new` com o comportamento atual de operar sobre `read_cr3()`, para não exigir mudança nos chamadores já existentes em `kernel::heap`/`kernel::stack`).

### Stack de cada thread: pool de slots numa nova faixa virtual compartilhada, não por processo
Diferente da metade inferior (específica de cada processo), a stack de uma thread é código/dado de kernel — vive na região compartilhada (higher-half), copiada por referência entre todas as address spaces. Se cada processo tentasse colocar a stack de sua(s) thread(s) num endereço virtual fixo igual ao de outro processo, as duas colidiriam na mesma tabela física compartilhada. Por isso, as stacks de threads são alocadas de um pool global de slots, cada um com endereço virtual único: `THREAD_STACK_POOL_BASE = 0xffff_b000_0000_0000` (índice 352 da PML4 — distante da HHDM, do heap e da stack principal), com o slot N ocupando `[BASE + N*SLOT_STRIDE, BASE + (N+1)*SLOT_STRIDE)`, onde `SLOT_STRIDE = PAGE_SIZE + KERNEL_STACK_SIZE` (guard page de 4 KiB + 64 KiB de stack, mesmo tamanho da stack principal). Um alocador simples (bump, sem reciclagem — ver Non-Goals) entrega o próximo slot livre a cada thread criada.

### Fila de prontos do escalonador: lock exclusivo com interrupções desabilitadas
A fila de prontos (lista de threads escalonáveis) é protegida por um `Spinlock` (o mesmo tipo já usado por `FRAME_ALLOCATOR`), mas **toda** seção crítica sobre ela — tanto no código comum (criar uma thread, adicioná-la à fila) quanto dentro de `on_timer_tick` — SHALL rodar com interrupções desabilitadas (`cli` antes do lock, `sti`/restauração do estado anterior de `IF` depois). Motivo: o handler do timer precisa da mesma fila a cada tick; se uma thread comum for preemptada bem no meio de uma seção crítica sobre essa fila (lock já adquirido), o handler do timer travaria para sempre tentando adquirir o mesmo lock — um deadlock real de single-core, não apenas teórico (é a mesma classe de bug — lock mantido através de um ponto onde o controle não retorna normalmente — já encontrada e corrigida durante `kernel-stack-guard-page`, ali envolvendo um `jmp` divergente em vez de uma interrupção). Desabilitar interrupções durante a posse do lock elimina a janela de reentrância por completo, já que não há SMP para considerar.

### Processo e thread: `Process` possui `Vec<Thread>`, `kernel::process::spawn_thread` reusa o address space
`Process` guarda sua `AddressSpace` e uma lista de `ThreadId`s que pertencem a ele. Criar um processo (`Process::new`) cria a address space e uma primeira thread. Criar uma thread adicional no mesmo processo (`Process::spawn_thread`) reusa a `AddressSpace` já existente, apenas aloca um novo slot de stack e um novo `Context` — cobre a distinção processo/thread do roadmap ("threads são as unidades de escalonamento dentro de um processo") sem exigir uma segunda change.

## Riscos / Trade-offs

- **[Risco] A rotina de troca de contexto em assembly é, de longe, o código mais delicado já escrito neste kernel — um erro sutil (ordem de push/pop, registrador esquecido, offset errado no "trap frame" sintético) pode corromper silenciosamente o estado de uma thread ou travar de forma difícil de diagnosticar** → Mitigação: extrair toda lógica que não depende de hardware (decisão de qual a próxima thread, round-robin sobre a fila) para funções Rust puras e testáveis em host (mesmo padrão já usado em `paging::decompose`, `phys::usable_frame_addresses`); verificar a troca de contexto em si via boot real com GDB, inspecionando registradores antes/depois de cada troca entre pelo menos 3 threads de teste, antes de considerar a tarefa concluída (mesmo padrão de verificação manual já exigido em `kernel-stack-guard-page`).
- **[Risco] Deadlock por reentrância de lock através da interrupção do timer** → Mitigado pela decisão acima (lock da fila de prontos sempre com interrupções desabilitadas); documentado aqui para lembrar que qualquer lock global tocado futuramente a partir de `on_timer_tick` precisa da mesma proteção.
- **[Trade-off] Pool de stacks de thread sem reciclagem (bump allocator)** → Aceitável porque esta change não implementa encerramento de thread (Non-Goal); se o pool se esgotar, a criação de nova thread falha de forma tratável (erro explícito), não há exaustão silenciosa.
- **[Trade-off] Processos com address space própria mas, nesta change, sem nenhum consumidor real da metade inferior (nenhum código de usuário ainda existe)** → Aceitável: a spec `kernel-processes` testa o mecanismo de isolamento diretamente (mapear uma página de teste na metade inferior de um processo e confirmar que não aparece no outro), sem depender de modo usuário existir; a mecânica fica pronta para a change de System Calls reusar.
- **[Trade-off] Processo e thread salvam/restauram todos os registradores de uso geral a cada tick (não só os callee-saved)** → Mais simples e auditável do que otimizar para salvar só o necessário; a 100 Hz, o custo extra é desprezível frente ao objetivo desta fase (corretude, não desempenho).

## Migration Plan

Não há estado em produção. "Deploy" continua significando: o smoke test de boot (`cargo xtask test`) passa com o escalonador ativo (banner, memmap, heap, APIC e timer reportados como hoje, mais uma nova linha confirmando o escalonador iniciado), os testes unitários da lógica pura do escalonador (seleção round-robin, alocação de slots de stack) passam em host, e uma verificação manual via GDB confirma pelo menos duas threads de teste alternando execução (ex.: cada uma incrementando um contador próprio, ambos avançando ao longo do tempo) antes de qualquer tarefa de troca de contexto ser marcada concluída. Rollback é reverter os commits desta change; nenhum dado persistente é afetado.
