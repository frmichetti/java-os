# Tasks

## 1. `ProcessId` disponível a partir de qualquer *syscall*

- [x] 1.1 Adicionar `process: ProcessId` a `kernel::process::Context`, com
      `Default`-like placeholder (`ProcessId(0)`, nunca lido antes de
      `register_thread` popular o valor real) nos quatro pontos de construção já
      existentes — mesmo padrão de `fpu_state`/`syscall_rsp_slot`/`timed_out`.
      Popular o valor real em `kernel::scheduler::register_thread` a partir de
      `thread.process` (hoje descartado) — verificar que `cargo kernel-build`
      continua compilando. **Achado real, simplificação em relação ao design.md**:
      não precisou de nenhum código novo em `register_thread` — os dois pontos de
      construção de `process.rs` (`create_kernel_thread`/`create_user_thread`) já
      recebem `process: ProcessId` como parâmetro (usado para o próprio campo
      `Thread.process`), então o `Context` literal ali já pode usar o mesmo valor
      diretamente (`Context { ..., process }`); `register_boot_thread` idem com
      `boot_process`. `register_thread`'s `contexts[index] = thread.context;`
      (já existente) carrega o campo correto automaticamente, sem precisar ler
      `thread.process` separadamente. Só o placeholder do `resize_with` precisou
      de um valor explícito (`ProcessId(0)`, nunca lido antes de ser sobrescrito).
- [x] 1.2 Adicionar `pub(crate) fn current_process_id() -> ProcessId` a
      `kernel::scheduler` — verificar com `cargo test -p kernel --lib` que os 271
      testes existentes continuam passando. **Achado real grave, corrigido
      durante a implementação — ver design.md, "`current_process_id()` lê uma
      global leve..."**: a primeira versão (`current_thread_id()` +
      `CONTEXTS.lock()[id].process`, como esta tarefa originalmente descrevia)
      usa `with_interrupts_disabled` (`cli`) — ao rodar `cargo test -p kernel
      --lib`, **derrubou a suíte inteira com `SIGSEGV`**, já que toda `sys_*`
      que cria/consulta um handle passaria a executar uma instrução privilegiada
      mesmo quando chamada via `dispatch` em teste de host (anel 3). Corrigido
      usando uma nova global leve `CURRENT_PROCESS_ID` (mesmo padrão de
      `CURRENT_FPU_AREA`/`CURRENT_SYSCALL_RSP_SLOT`, atualizada por
      `schedule_next`/`register_boot_thread`), lida sem nenhuma instrução
      privilegiada. **Confirmado depois da correção**: 273 testes passando
      (271 de antes + os 2 novos de posse de `pthread_handles`, tarefa 2.1),
      sem nenhum `SIGSEGV`.

## 2. `kernel::pthread_handles` com posse

- [x] 2.1 Mudar `MUTEXES`/`CONDVARS` para `Spinlock<Vec<(ProcessId, &'static
      Mutex/CondVar)>>`; `create_mutex`/`create_cond` passam a receber `owner:
      ProcessId`; `mutex`/`cond` passam a receber `caller: ProcessId` e só
      devolvem `Some` se `owner == caller` — verificar com `cargo test -p kernel
      --lib`, adicionando testes cobrindo: handle consultado pelo próprio dono
      (sucesso, mesmo resultado de antes), handle consultado por outro processo
      (rejeitado, mesmo resultado de handle desconhecido).

## 3. `kernel::file_handles` com posse e fechamento ao terminar

- [x] 3.1 `OPEN_FILES` passa a guardar `(ProcessId, Option<OpenFile>)`;
      `register` recebe `owner: ProcessId`; `read`/`close` recebem `caller:
      ProcessId` e só operam se `owner == caller` — verificar com `cargo test -p
      kernel --lib`, adicionando testes cobrindo: leitura/fechamento pelo próprio
      dono (sucesso, mesmo resultado de antes), por outro processo (rejeitado).
      **Achado real**: `owner` virou campo de `OpenFile` (dentro do `Option`), não
      uma tupla `(ProcessId, Option<OpenFile>)` — mais simples, sem precisar
      desembrulhar duas camadas em cada acesso.
- [x] 3.2 Adicionar `pub(crate) fn close_all_owned_by(owner: ProcessId)` a
      `kernel::file_handles`: percorre a tabela e chama `close` em cada entrada
      ainda aberta pertencente a `owner` — verificar com `cargo test -p kernel
      --lib`, teste cobrindo múltiplos arquivos de donos distintos (só os do
      dono pedido são fechados). **Confirmado**: 276 testes passando (273 de
      antes + 3 novos de `file_handles`).

## 4. `kernel::vmm` com posse e liberação ao terminar

- [x] 4.1 `ReservationState` ganha `owner: ProcessId`; `Vmm::reserve` recebe
      `owner: ProcessId`; `commit`/`decommit`/`protect`/`release`/
      `region_containing` recebem `caller: ProcessId` e só operam se
      `owner == caller` da reserva envolvida — verificar com `cargo test -p
      kernel --lib`, adicionando testes cobrindo: operação pelo próprio dono
      (sucesso, mesmo resultado de antes), por outro processo (rejeitada com o
      mesmo erro de reserva desconhecida). **Achado real**: `kernel::safepoint`/
      `kernel::safepoint_demo`/`kernel::vmm_demo` também chamam `Vmm` diretamente
      (fora de `kernel::syscall`) — também precisaram de `current_process_id()`.
- [x] 4.2 Adicionar `pub(crate) fn release_all_owned_by(owner: ProcessId)` a
      `kernel::vmm`: percorre as reservas vivas e libera cada uma pertencente a
      `owner` — verificar com `cargo test -p kernel --lib`, teste cobrindo
      múltiplas reservas de donos distintos (só as do dono pedido são
      liberadas). **Confirmado**: 279 testes passando (276 de antes + 3 novos de
      `vmm`). Testável em host só com reservas nunca comprometidas (`release`
      só toca `Mapper`/HHDM reais para páginas comprometidas).

## 5. Fiação em `kernel::syscall` e no ponto de limpeza do escalonador

- [x] 5.1 Toda `sys_*` que cria ou consulta um `handle`/reserva (`sys_mutex_create`/
      `sys_mutex_lock`/`sys_mutex_trylock`/`sys_mutex_unlock`/`sys_cond_create`/
      `sys_cond_wait`/`sys_cond_notify_one`/`sys_cond_notify_all`/`sys_open`/
      `sys_read`/`sys_close`/`sys_vm_reserve`/`sys_vm_commit`/`sys_vm_decommit`/
      `sys_vm_protect`/`sys_vm_release`) passa a repassar
      `kernel::scheduler::current_process_id()` para a função de tabela
      correspondente — nenhuma mudança de assinatura de `dispatch` nem de
      sentinela de erro. Verificar com `cargo test -p kernel --lib` que os
      testes já existentes de `syscall.rs` continuam passando sem alteração
      (cada um já chama a função de teste correspondente com um único
      `ProcessId` implícito — ver achado real se algum teste precisar de ajuste).
      **Confirmado**: nenhum teste existente de `syscall.rs` precisou de
      ajuste — todos continuam usando implicitamente `ProcessId(0)`
      (`CURRENT_PROCESS_ID` nunca escrita em teste de host), consistente em
      cada teste individual.
- [x] 5.2 Em `kernel::scheduler::schedule_next`, no mesmo ramo que já chama
      `AddressSpace::free_user_mappings` (`kernel-process-cleanup`), chamar
      também `kernel::file_handles::close_all_owned_by` e
      `kernel::vmm::release_all_owned_by` com o `ProcessId` de
      `contexts[was_running.0 as usize].process` — verificar com `cargo test -p
      kernel --lib` que compila e os testes existentes passam. **Confirmado**:
      279 testes passando.

## 6. Verificação em boot real

- [x] 6.1 Rodar `cargo xtask test` em QEMU e confirmar que todo programa de teste
      já existente (que só usa seus próprios handles) continua reportando
      exatamente os mesmos resultados de antes desta change — prova de que a
      checagem de posse não introduz nenhuma regressão para uso legítimo.
      **Confirmado**: `cargo xtask test` (harness padrão de 15s) passou
      (`smoke test de boot: PASSOU`), e todo programa de teste reportou
      exatamente o mesmo padrão de resultado de antes (`file_syscalls_demo:
      resultado PPPPPPPP`, `javaos_vm_test_program: resultado PPPPPPPP`,
      `user_posix_demo: resultado PPPPPPPPPPPP`, `timedwait_test_program:
      resultado PP`, `safepoint: 51607 leituras... PASSOU`, etc.) — nenhuma
      regressão para uso legítimo (cada processo só acessando os próprios
      handles). **Achado real, fora do escopo desta change**: ao estender
      manualmente a janela de boot para 30s (investigando um log mais longo),
      apareceu um panic reproduzível em `kernel::clock::cycles_to_nanos`
      (`attempt to multiply with overflow`) causado por uma calibração de TSC
      pré-existente (`calibrate_tsc()`, da change `kernel-wall-clock`) que
      mediu ~23,4 GHz sob contenção do host — nada relacionado ao código desta
      change. Registrado em `KNOW_ISSUES.md` com causa raiz completa e
      correção pendente; não bloqueia esta change, já que o harness padrão de
      15s (o critério real desta tarefa) nunca expõe o problema.
- [x] 6.2 Confirmar, no mesmo log de boot, a liberação de arquivos/reservas de
      memória virtual ao terminar: instrumentar temporariamente (ou de forma
      permanente, a critério da implementação) o ponto da tarefa 5.2 com uma
      mensagem reportando quantos arquivos/reservas de cada thread terminada
      foram fechados/liberados — mesmo padrão de diagnóstico já usado por
      `kernel-process-cleanup` para frames físicos. **Confirmado**: o log de
      boot mostra `kernel_handle_ownership: thread 9 fechou 0 arquivo(s) e
      liberou 1 reserva(s) de memoria virtual` (thread do safepoint demo,
      liberando sua página de poll reservada) e `kernel_handle_ownership:
      thread 8 fechou 0 arquivo(s) e liberou 1 reserva(s) de memoria virtual`
      (thread do `javaos_vm_test_program`, liberando sua última reserva viva
      ao terminar) — ambos na posição esperada, logo após o diagnóstico de
      `kernel_process_cleanup` da mesma thread.

## 7. Documentação

- [x] 7.1 Adicionar a este change (`design.md`, "Resultado real da implementação")
      a confirmação do que foi verificado (testes de host + boot real) e o
      relato de qualquer achado real durante a implementação.
- [x] 7.2 Atualizar `ROADMAP.md` — mover "Isolar as tabelas de handle..." para a
      seção de implementado (deixando claro que `pthread_handles` ganhou posse
      e rejeição entre processos, mas continua vazando os próprios objetos —
      Non-Goal documentado), e apontar o próximo item real de médio prazo como o
      próximo passo — verificar que o documento permanece consistente.
      **Confirmado**: movido para "Liberação real de recursos do processo
      encerrado" (com a nota de `pthread_handles` ainda vazando os próprios
      objetos); "Próximo passo imediato" agora aponta o esquema de erro
      granular (`errno`-like) + validação de ponteiro de usuário, promovido da
      antiga seção "Médio prazo".

## Workflow follow-up

- Arquivar esta change (`openspec archive kernel-handle-ownership`) depois que a
  verificação em boot real (tarefas 6.1-6.2) e `cargo test -p kernel --lib`
  estiverem confirmadas.
