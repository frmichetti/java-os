# Design

## Context

Ver proposal.md - Why/What Changes para a motivação. Pontos do estado atual relevantes para o "como" (código lido em `kernel/src/`):

- `kernel::idt::page_fault_handler` é `extern "x86-interrupt" fn(frame: InterruptStackFrame, error_code: u64)`, instalado como *interrupt gate* (não *trap gate* — `GATE_TYPE_INTERRUPT = 0xE` em `idt.rs`), rodando na stack dedicada `PAGE_FAULT_STACK` (IST2, 8 KiB, um único buffer fixo reutilizado por **toda** entrega de `#PF`, nunca por thread). Hoje sempre reporta e chama `crate::hlt_loop()` — nunca retorna.
- `error_code` do `#PF`: bit 0 (`P`) indica a causa — `0` = página ausente (não mapeada), `1` = violação de permissão numa página presente. Já observado em boot real (`kernel-posix-virtual-memory`, "Achados"): `codigo_erro=0x0` para ausência, `codigo_erro=0x3` para escrita numa página somente-leitura.
- `kernel::vmm::Vmm::{reserve, commit, decommit, region_containing}` (`kernel-virtual-memory`/`kernel-jvm-posix-syscalls`) já fornecem exatamente "mapear sob demanda" e "desmapear sob demanda" de uma sub-região; `decommit` desmapeia de verdade (via `Mapper::unmap`), tornando qualquer acesso subsequente uma falta por ausência (`P=0`) — não existe, nesta arquitetura, uma forma de tornar uma página "totalmente inacessível mas ainda presente" (`PageFlags` só tem `writable`/`user_accessible`/`executable`; presença implica legibilidade).
- Nenhum gate da IDT deste kernel é *trap gate*: toda entrada por `#PF` já roda com `IF=0` automaticamente (entrada de interrupt gate limpa `IF`; só a restauração de `RFLAGS` salva no `iretq` final o religa) — confirmado lendo `idt.rs`/`gdt.rs`, nenhum código adicional faz isso manualmente.
- `kernel::wait_queue`/`kernel::scheduler::with_interrupts_disabled` existem para bloqueio voluntário de thread, mas sempre chamados em contexto de thread normal (nunca de dentro de um handler de exceção rodando sobre uma IST compartilhada).

## Goals / Non-Goals

**Goals:**
- Permitir registrar uma faixa de endereço virtual, dentro de uma reserva viva de `kernel::vmm`, como "ponto de poll de safepoint".
- Uma falta de página por ausência (`P=0`) dentro de uma faixa registrada retoma a execução da thread automaticamente (reexecuta a instrução que faltou), sem reportar exceção nem haltar.
- Qualquer outra falta (violação de permissão, ausência fora de uma faixa registrada) continua exatamente como hoje (fatal).
- Provar o mecanismo ponta a ponta com um demo em modo kernel (mesmo padrão de `vmm_demo`/`pthread_demo`): uma thread arma (descompromete) e desarma (compromete de novo) a página; outra a sonda continuamente, sem o kernel haltar em nenhum momento.

**Non-Goals (nesta change):**
- Desvio de execução para um handler escolhido pelo processo (`sigaction(SIGSEGV, ...)` completo, com *trampoline* e salvamento de contexto para um endereço arbitrário de modo usuário) — decisão explícita do usuário ao propor esta change: implementar só o "retry automático" que o safepoint da JVM precisa, evitando o risco de reentrância na IST compartilhada que um desvio genérico (potencialmente bloqueando a thread) exigiria (ver "Decisions" abaixo, "Por que não bloquear a thread").
- Expor isto como *syscall* a modo usuário — mesma disciplina incremental já seguida por `kernel-wait-queue`, `kernel-posix-virtual-memory`, `kernel-pthread-sync`, `kernel-wall-clock` (primitiva em modo kernel primeiro; exposição via *syscall* fica para uma sub-change futura, como `kernel-jvm-posix-syscalls` fez de uma vez para as quatro anteriores).
- Cancelar um registro (`unregister`) — mesmo Non-Goal já aceito para `kernel::pthread_handles` (sem destruição): a tabela só cresce, aceitável na escala de uso atual (poucas faixas, cada uma viva para sempre).
- Safepoint via violação de permissão (página presente mas protegida contra leitura) — inalcançável nesta arquitetura (`PageFlags` não expressa "presente mas ilegível"); o mecanismo é inteiramente baseado em ausência (`decommit`/`commit`).
- Isolamento por processo do registro de faixas — global ao kernel inteiro, mesma simplificação já aceita por `kernel::vmm`/`kernel::pthread_handles`.
- Qualquer validação de que o chamador tem permissão para registrar uma faixa específica — moot nesta change (só código de modo kernel chama o registro; validação de confiança fica para quando a *syscall* for exposta).

## Decisions

### Mecanismo: "retry automático" via retorno normal do handler, não bloqueio
A decisão central desta change: quando `page_fault_handler` reconhece uma falta por ausência dentro de uma faixa registrada, ele **não faz nada além de retornar normalmente** — nem bloqueia a thread, nem reescreve o frame de interrupção. Como o handler nunca escreve em `frame` (`instruction_pointer`/`code_segment`/`cpu_flags`/`stack_pointer`/`stack_segment` permanecem exatamente como a CPU empilhou), o `iretq` gerado pelo compilador para a ABI `extern "x86-interrupt"` restaura exatamente o estado anterior à falta — a CPU tenta de novo a mesma instrução. Se a página continuar ausente, a mesma falta é entregue de novo (outro `#PF`, retorno, retry) — efetivamente um *busy-wait* implementado via hardware em vez de um laço de software; se a página já tiver sido comprometida de novo, a instrução simplesmente terá sucesso. Mesmo padrão já usado por `default_interrupt_handler` (que também nunca toca `frame` e retorna normalmente, confiando no `iretq` da ABI).

**Por que não bloquear a thread** (alternativa descartada: `WaitQueue::block_current()` a partir de dentro do handler): `PAGE_FAULT_STACK` é um **único buffer fixo de 8 KiB, reutilizado por toda entrega de `#PF`** (não uma stack por thread) — a CPU sempre reinicia `RSP` para o mesmo topo ao entregar o vetor. Se o handler bloqueasse a thread corrente (removendo-a da `ReadyQueue`, salvando seu "RSP corrente" como contexto retomável), esse RSP apontaria para dentro da `PAGE_FAULT_STACK` — memória que a *próxima* entrega de `#PF` (de qualquer outra thread, nesta mesma faixa ou em qualquer falta fatal não relacionada) sobrescreveria incondicionalmente, corrompendo o contexto salvo da primeira thread. Isso é inevitável em cenários de safepoint reais: múltiplas threads tipicamente atingem a página de poll quase simultaneamente. Evitar completamente esse bloqueio — nunca deixando nada "pendurado" na IST entre uma entrega e a próxima — elimina o risco por construção, sem precisar de uma stack de IST por thread (que exigiria uma mudança bem mais profunda em `kernel::stack`/`gdt`).

**Por que isso não precisa desabilitar interrupções manualmente**: todo gate desta IDT já é *interrupt gate* (`GATE_TYPE_INTERRUPT`), que limpa `IF` automaticamente na entrada — o corpo inteiro do handler (a consulta ao registro, intencionalmente curtíssima) já roda atomicamente em relação a outras interrupções, restaurado só pelo `iretq`. Isso elimina qualquer risco de uma interrupção de timer aninhar durante a janela (ainda que breve) em que o handler ocupa a `PAGE_FAULT_STACK`.

### `kernel::safepoint`: registro puro + validação contra `kernel::vmm`
Mesmo padrão de separação já usado em `kernel::vmm` (camada pura vs. camada real) e em `kernel::syscall`/`kernel::vmm` (tradução endereço→`Region`):
- `SafepointRegistry` (privada): `Vec<(base: u64, size: u64)>` e `contains(&self, addr: u64) -> bool` — lógica pura, sem nenhum acesso a `kernel::vmm`, testável em host.
- `pub fn register(base: u64, size: u64) -> Result<(), SafepointError>`: valida, via `kernel::vmm::VMM.lock().region_containing(base)`, que `[base, base+size)` cai inteiramente dentro de uma reserva viva (cobre "Faixa fora de qualquer reserva viva é rejeitada" da spec); se válido, insere em `SafepointRegistry` (instância global, `Spinlock`).
- `pub fn is_registered(addr: u64) -> bool`: consultado por `page_fault_handler` — delega a `SafepointRegistry::contains`.

### `page_fault_handler` ganha um desvio no início, sem mudar assinatura
```
extern "x86-interrupt" fn page_fault_handler(frame: InterruptStackFrame, error_code: u64) {
    const PAGE_FAULT_ERROR_PRESENT: u64 = 1 << 0;
    if error_code & PAGE_FAULT_ERROR_PRESENT == 0 && crate::safepoint::is_registered(read_cr2()) {
        return; // retoma a mesma instrucao (iretq gerado pela ABI, frame nunca alterado)
    }
    // comportamento existente, inalterado
    crate::serial_println_emergency!(...);
    crate::hlt_loop();
}
```
Nenhuma mudança de assinatura, de stack dedicada, ou do caminho fatal existente — a mudança é estritamente aditiva (um `if` antes do comportamento já existente).

### Demo em modo kernel: duas threads kernel compartilhando o espaço de endereçamento corrente
Mesmo padrão de `kernel::wait_demo`/`kernel::pthread_demo` (`scheduler::spawn_sharing_current_address_space`, sem isolamento de processo — threads kernel comuns). Uma thread "coordenadora" reserva+compromete uma página via `kernel::vmm`, registra-a via `kernel::safepoint::register`, descompromete-a (arma o safepoint), aguarda um número fixo de ticks, e compromete-a de novo (libera); uma thread "sondadora" lê repetidamente (via `core::ptr::read_volatile`) um byte da página registrada, por um número fixo de iterações que atravessa a janela de descomprometimento/recomprometimento (garantido pela preempção round-robin normal do escalonador, mesma técnica de concorrência real já usada por `wait_demo`/`pthread_demo`), contando quantas leituras completam. O simples fato de o log de boot chegar até a mensagem final (reportando a contagem completa) já prova que nenhuma falta dentro da janela armada derrubou o kernel — qualquer falha real produziria a mensagem de exceção fatal existente e um halt observável, mesmo método de verificação já usado por `kernel-posix-virtual-memory`/`kernel-stack-guard-page` para comportamento só observável em boot real.

## Achados durante a implementação

- **Bug pré-existente descoberto: `kernel::vmm::VMM_WINDOW_START` colidia com `kernel::stack::THREAD_STACK_POOL_BASE`.** Ambos usavam literalmente o mesmo endereço (`0xffff_b000_0000_0000`, índice 352 da PML4) — `kernel-process-scheduler` reservou esse índice para o pool de stacks de kernel de cada thread nova; `kernel-posix-virtual-memory` (change posterior) reservou o **mesmo** índice para a janela de `kernel::vmm`, por uma suposição errada no próprio comentário do código ("o primeiro índice livre após a stack dedicada do kernel, índice 320" — sem considerar que o índice 352 já estava ocupado pelo pool de stacks, documentado só em `kernel::stack`, não em `docs/architecture/overview.md`). A colisão nunca foi detectada porque nenhuma verificação anterior exercitava `kernel::vmm` e mais de uma thread ao mesmo tempo além das primeiras 1-2 páginas da janela — o pool de stacks aloca slots de 68 KiB (guard page + 64 KiB) sequencialmente a partir do mesmo endereço, então a stack de kernel da primeira thread criada (`kernel::user_demo`) ocupa exatamente a segunda página da "janela do VMM".
  - **Sintoma**: ao implementar o demo desta change (a primeira verificação a reservar/comprometer uma página do `kernel::vmm` *depois* de outras threads já criadas), `Vmm::commit` encontrava `Mapper::map` retornando `AlreadyMapped` para a página reservada — na verdade a stack de kernel viva de outra thread. Depurado com `cargo xtask run`/GDB e prints temporários (removidos depois), bissectando até confirmar, via `Mapper::translate`, exatamente qual chamada (`finish_new_user`/`create_user_thread`/`stack::alloc_thread_stack`) mapeava a página em questão.
  - **Risco real**: qualquer uso de `kernel::vmm` além de poucas páginas corromperia silenciosamente a stack de kernel de alguma thread viva (ou vice-versa) — um risco sério para o porte da JVM (heap Java via `mmap`), não apenas para esta change.
  - **Correção**: `VMM_WINDOW_START` movido para `0xffff_c000_0000_0000` (índice 384 da PML4, verificado livre — nenhuma outra faixa reservada usa esse índice). Atualizado também: a constante espelhada no assembly de `kernel::user_posix_demo` (`kernel-jvm-posix-syscalls`, que verifica o endereço reservado contra os limites da janela) e `docs/architecture/overview.md` (seções 1 e a descrição de `kernel-posix-virtual-memory`), que agora documenta explicitamente o índice de PML4 de cada faixa reservada (HHDM 256, heap 288, stack de boot 320, pool de stacks de thread 352, janela do VMM 384) para evitar uma colisão futura do mesmo tipo.
- **Risco de deadlock encontrado e corrigido na própria implementação desta change**: as seções críticas de `kernel::safepoint_demo::coordinator` que tocam `kernel::vmm::VMM`/`kernel::phys::FRAME_ALLOCATOR` precisaram ser envolvidas em `scheduler::with_interrupts_disabled` (mesmo padrão já usado para `READY_QUEUE`/`CONTEXTS`/`SERIAL1`/`CONSOLE`). Sem isso: uma *syscall* de outro processo (que roda inteiramente com `IF=0` via `SFMASK`) tentando `VMM.lock()`/`FRAME_ALLOCATOR.lock()` enquanto a coordenadora detém esses locks e é preemptada no meio da operação (código de thread normal, sem `with_interrupts_disabled`, livre para ser interrompido pelo timer) ficaria presa em `Spinlock::lock()` **com interrupções desabilitadas** — a mesma categoria de impasse já documentada em `KNOW_ISSUES.md` para os locks de impressão, agora também relevante para `kernel::vmm`. `kernel::syscall::sys_vm_*` já roda inteiramente com `IF=0` (dentro da *syscall*), então não precisou de mudança — só o lado do demo, que é código de thread comum.
- **Verificação em boot real**: com as duas correções acima, `cargo xtask run` (3 execuções consecutivas) e `cargo xtask test` completam sem nenhum `EXCECAO #PF`/halt, com `kernel::safepoint_demo` reportando `"safepoint: N leituras completadas sem halt (incluindo durante a janela armada) - PASSOU"` em todas as execuções (N variando por execução, de ~38 mil a ~56 mil leituras, conforme a velocidade de escalonamento sob QEMU/TCG — nunca zero, confirmando que a sondadora de fato atravessou a janela armada via `#PF`/retry repetidos). `cargo test -p kernel --lib`: 167 testes passando (165 + os 2 novos de `is_page_absent`), sem regressão.

## Risks / Trade-offs

- [Uma thread sondando uma página armada consome CPU inteira em *retries* de hardware (`#PF`/`iret` repetidos) em vez de ceder a vez] → Aceito como a própria essência do mecanismo "retry automático" escolhido (ver Decisions) — mesmo espírito de tolerância a *busy-wait* já aceito em `kernel::sync::Spinlock` nesta fase do projeto. Um mecanismo que efetivamente bloqueia sem busy-wait exigiria resolver o problema de stack de IST por thread, fora do escopo desta change.
- [`SafepointRegistry` só cresce, nunca encolhe] → Mesmo Non-Goal já aceito para `kernel::pthread_handles`; desprezível na escala de uso atual.
- [Registro global, sem isolamento entre processos — qualquer registro afeta o kernel inteiro] → Herdado de `kernel::vmm`/`kernel::pthread_handles`; não piora nem resolve a limitação já existente.
- [`is_registered` é consultado a cada `#PF`, incluindo os já existentes de erro genuíno (guard page, `vmm_demo`, etc.) — adiciona uma busca linear curta ao caminho fatal existente] → Desprezível (`Vec` pequeno, poucas faixas esperadas) e não muda nenhum comportamento observável desses casos (todos têm `P=1` ou não estão registrados, caindo no mesmo `hlt_loop()` de sempre).

## Migration Plan

Mudança puramente aditiva no binário do kernel — nenhuma migração de dado, nenhum consumidor existente afetado (o caminho fatal de `page_fault_handler` para qualquer falta não registrada continua idêntico). Nenhum rollback especial.
