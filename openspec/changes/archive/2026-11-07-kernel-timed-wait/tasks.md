# Tasks

## 1. Bloqueio com prazo em `kernel::wait_queue`

- [x] 1.1 Adicionar `timed_out: bool` a `kernel::process::Context` (default `false`),
      atualizando os três pontos de construção (`register_boot_thread`,
      `create_kernel_thread`, `create_user_thread`) — verificar que `cargo
      kernel-build` continua compilando. **Achado**: havia um quarto ponto de
      construção (`contexts.resize_with` em `register_thread`), não previsto no
      plano original — também atualizado.
- [x] 1.2 Adicionar `static TIMED_WAITERS: Spinlock<Vec<(ThreadId, u64, &'static
      WaitQueue)>>` a `kernel/src/wait_queue.rs`, e modificar `set_block_intent`
      (chamada por `block_current`/`block_current_if`/`register_intent_for`, os três
      pontos de registro de intenção de bloqueio) para remover primeiro qualquer
      entrada pendente de `TIMED_WAITERS` para a thread — verificar com um teste
      puro (sem hardware) que registrar uma nova intenção de bloqueio limpa um
      prazo pendente anterior da mesma thread. Verificado com 2 testes (limpa a
      própria thread, preserva a de outra thread).
- [x] 1.3 Adicionar `WaitQueue::register_timed_intent_for(&'static self, current:
      ThreadId, deadline_nanos: u64)` (mesmo papel de `register_intent_for` já
      existente, mas também insere em `TIMED_WAITERS`) — a thread que bloqueia lê e
      limpa `CONTEXTS.lock()[current.0 as usize].timed_out` (via novos
      `scheduler::mark_timed_out`/`take_timed_out`) logo após `commit_block()`
      retornar — verificar compilação. **Correção de escopo em relação ao plano
      original**: não foi adicionado um `WaitQueue::block_current_with_deadline`
      separado (sem mutex associado) — ficaria sem nenhum consumidor real nesta
      change (`cargo build` reportaria "never used") e a capacidade de bloqueio com
      prazo já é especificada em termos de comportamento observável, satisfeita de
      ponta a ponta por `CondVar::wait_timeout` (tarefa 2.1), que já precisa da sua
      própria dança de liberar o mutex entre o registro e o `commit_block()` — mesmo
      motivo pelo qual `CondVar::wait` (já existente) também nunca chamou
      `block_current()` diretamente.
- [x] 1.4 Adicionar `WaitQueue::cancel_if_blocked(&self, id: ThreadId) -> bool`
      (remove `id` de `self.blocked` se presente, devolve se removeu) e uma função
      `pub(crate) fn wake_expired_deadlines(now_nanos: u64)` em
      `kernel::wait_queue` que varre `TIMED_WAITERS`, remove toda entrada com prazo
      `<= now_nanos`, e para cada uma tenta `cancel_if_blocked` na fila indicada —
      se removeu, marca `CONTEXTS.lock()[id.0 as usize].timed_out = true` e chama
      `scheduler::make_ready(id)`; se não removeu (já despertada normalmente antes),
      não faz nada. **Achado real sobre testabilidade**: `scheduler::with_interrupts_disabled`
      (`cli`/`sti`) nunca é de fato exercitado por nenhum teste de host já existente
      neste kernel (executaria uma instrução privilegiada num processo de teste
      comum, travando) — todo teste existente sempre contorna isso testando só a
      lógica pura por trás (mesmo padrão de `try_register_block_intent`). Seguido o
      mesmo padrão: extraída `take_expired_deadlines(now_nanos)` (pura, só mexe em
      `TIMED_WAITERS`) de `wake_expired_deadlines` (que chama
      `scheduler::mark_timed_out`/`make_ready`, não testada diretamente). Verificado
      com 4 testes: `cancel_if_blocked` remove/não-remove corretamente;
      `take_expired_deadlines` seleciona só os prazos vencidos, preservando os
      demais.
- [x] 1.5 Chamar `kernel::wait_queue::wake_expired_deadlines(kernel::clock::monotonic_nanos())`
      em `kernel::scheduler::on_timer_tick`, logo após `crate::timer::on_tick()` e
      antes de `schedule_next` — verificar que `cargo kernel-build` continua
      compilando.
- [x] 1.6 Executar `cargo test -p kernel --lib` e confirmar que todos os testes
      novos (tarefas 1.2/1.4) e todos os já existentes (261 antes desta change)
      passam.

## 2. `CondVar::wait_timeout` e `SYSCALL_COND_WAIT` com prazo

- [x] 2.1 Adicionar `kernel::pthread::CondVar::wait_timeout(&'static self, mutex:
      &'static Mutex, deadline_nanos: u64) -> bool` (devolve `true` se notificada,
      `false` se expirou), mesma estrutura atômica de liberar o mutex + registrar
      intenção de `CondVar::wait`, mas chamando `block_current_with_deadline` em vez
      de `commit_block` direto, e reaquirindo o mutex em ambos os casos antes de
      retornar — verificar compilação.
- [x] 2.2 Adicionar `pub const SYSCALL_TIMEOUT: u64 = u64::MAX - 1;` a
      `kernel/src/syscall.rs`, e estender `sys_cond_wait` para aceitar um terceiro
      parâmetro (`timeout_nanos`): se `0`, comportamento idêntico ao de hoje
      (`cond.wait(mutex)`, devolve `0`); se maior que `0`, chama
      `cond.wait_timeout(mutex, kernel::clock::monotonic_nanos() + timeout_nanos)` e
      devolve `0` (notificada) ou `SYSCALL_TIMEOUT` (expirou) — atualizar o
      `dispatch` para passar `arg2` adiante — verificar com testes em
      `kernel/src/syscall.rs` (mesmo padrão dos já existentes): prazo zero preserva
      o comportamento atual; handle desconhecido continua `SYSCALL_ERROR`
      independente do prazo.
- [x] 2.3 Executar `cargo test -p kernel --lib` de novo e confirmar que nenhum teste
      existente de `SYSCALL_COND_WAIT` quebrou (todo chamador de teste já existente
      passa `0` no terceiro argumento, preservando o comportamento testado). 269
      testes passando (267 após o grupo 1 + 2 novos: rejeição de handle
      independente do prazo pedido).

## 3. `clock_gettime` e `pthread_cond_timedwait` reais na libc

- [x] 3.1 Adicionar `SYS_CLOCK_WALL=14`/`SYS_CLOCK_MONOTONIC=15` a `glue.c` — mesma
      convenção dos números já existentes, batendo com `kernel/src/syscall.rs`.
- [x] 3.2 Definir `_POSIX_TIMERS 1` em `glue.c` antes de `#include <time.h>` (mesmo
      achado/correção de `_POSIX_THREADS` para `<pthread.h>` — ver design.md) e
      implementar `clock_gettime(clockid_t clk_id, struct timespec *tp)` para
      `CLOCK_REALTIME` (via `SYS_CLOCK_WALL`) e `CLOCK_MONOTONIC` (via
      `SYS_CLOCK_MONOTONIC`), convertendo nanossegundos para `tv_sec`/`tv_nsec`;
      qualquer outro `clk_id` devolve `EINVAL` — verificar com
      `scripts/cross/x86_64-elf-gcc -c glue.c` sem erro. **Achado real adicional**:
      `CLOCK_MONOTONIC` exige também `_POSIX_MONOTONIC_CLOCK` definido — guarda
      separada de `_POSIX_TIMERS` no newlib vendorizado, só descoberta ao compilar
      (erro `'CLOCK_MONOTONIC' undeclared`); `CLOCK_REALTIME` sozinho não precisava
      dela.
- [x] 3.3 Implementar `pthread_cond_timedwait(pthread_cond_t *cond, pthread_mutex_t
      *mutex, const struct timespec *abstime)`: resolve a inicialização preguiçosa
      de ambos (mesmo padrão de `pthread_cond_wait`), calcula
      `delta = abstime_nanos - wall_clock_nanos()` via `SYS_CLOCK_WALL` direto (sem
      depender de `clock_gettime`); se `delta <= 0`, devolve `ETIMEDOUT`
      imediatamente sem emitir syscall; caso contrário, chama `SYS_COND_WAIT` com
      `delta` como terceiro argumento, devolvendo `0` (notificado) ou `ETIMEDOUT`
      (`r == SYSCALL_TIMEOUT`) — verificar compilação como acima.

## 4. `PlatformMonitor::wait(millis != 0)` real

- [x] 4.1 Em `vendor/jdk-27-ga/src/hotspot/os/javaos/mutex_javaos.cpp`, substituir o
      `fatal()` do caso `millis != 0` por: `clock_gettime(CLOCK_REALTIME, &deadline)`,
      somar `millis` (convertido para nanossegundos, tratando *overflow* de
      `tv_nsec`) a `deadline`, chamar `pthread_cond_timedwait(&_cond, mutex(),
      &deadline)`, e devolver `OS_OK` ou `OS_TIMEOUT` conforme o resultado —
      verificar com `g++ -fsyntax-only -std=gnu++17 ...` (mesma invocação de
      `os-javaos-platform-sync`) sem erro.

## 5. Verificação em boot real

- [x] 5.1 Criar `userland_c/timedwait_test_program.c` (mesmo padrão dos programas de
      teste anteriores): cria um mutex e uma variável de condição, mede
      `clock_gettime(CLOCK_MONOTONIC, ...)` antes, chama `pthread_cond_timedwait`
      com um prazo curto (ex.: 50 ms) sem que ninguém notifique, mede
      `clock_gettime(CLOCK_MONOTONIC, ...)` depois — confirma (a) que a chamada
      devolveu `ETIMEDOUT`, e (b) que o tempo decorrido é maior ou igual ao prazo
      pedido (dentro de uma margem generosa, considerando a granularidade de 100 Hz
      do timer) — reporta `P`/`F` por `write()`, mesmo formato dos programas
      anteriores. **Não testa** o caminho "notificado antes de expirar" — mesma
      limitação de sempre (sem syscall de criação de thread).
- [x] 5.2 Adicionar `build_timedwait_test_program` a `xtask/src/main.rs` (mesmo
      corpo dos `build_*_test_program` anteriores), chamar a partir de `build_image`,
      e copiar o ELF resultante para um nome curto 8.3 válido (ex.: `::/bin/timedw.elf`)
      — verificar que `cargo xtask build-image` produz o arquivo sem erro.
- [x] 5.3 Adicionar `TIMEDWAIT_TEST_PROGRAM_STACK_VIRT`/`_STACK_SIZE` a
      `kernel/src/scheduler.rs` (endereço distinto dos já existentes) e carregar
      `/bin/timedw.elf` via `kernel::scheduler::spawn_from_elf` em
      `kernel/src/main.rs`, mesma disciplina de erro explícito (nunca pânico) dos
      carregamentos ELF já existentes ali — verificar via `cargo xtask run` em QEMU
      que a mensagem de resultado aparece no console serial com `P`, e que nenhum
      `EXCECAO`/pânico ocorre. **Achado real grave, fora do escopo original desta
      change:** a primeira tentativa travava o kernel inteiro com `EXCECAO #PF` logo
      após o prazo de `timedw.elf` expirar — diagnosticado como um defeito
      pré-existente em `kernel::syscall` (`SYSCALL_USER_RSP_SCRATCH`, uma única
      `static` global usada para guardar/restaurar o `RSP` de usuário ao redor de
      `syscall`/`sysret`, nunca pensada para sobreviver a uma troca de contexto real
      para outra thread no meio do caminho — exatamente o que uma syscall bloqueante
      como `SYSCALL_COND_WAIT` faz). `timedw.elf` foi o primeiro cenário com uma
      janela de bloqueio garantidamente longa (50 ms, sem notificador) para expor
      isso. Registrado e corrigido em `KNOW_ISSUES.md` (ver lá o detalhe completo),
      a pedido explícito do usuário, já que sem a correção esta tarefa não podia ser
      verificada de verdade — `SYSCALL_USER_RSP_SCRATCH` foi substituída por um slot
      por-thread em `kernel::process::Context`. Verificado depois da correção:
      `timedwait_test_program: resultado PP (ordem: etimedout,elapsed_ge_requested)`,
      sem nenhum `EXCECAO`.
- [x] 5.4 Confirmar, no mesmo log de boot, que todos os programas de teste já
      existentes continuam reportando exatamente os mesmos resultados de antes
      (nenhuma regressão da extensão de `SYSCALL_COND_WAIT`/`Context`/`WaitQueue`,
      nem da correção de `SYSCALL_USER_RSP_SCRATCH` registrada na tarefa 5.3).
      Confirmado: `javaos_pthread_test_program` (10 `P`s), `libm_test_program` (5
      `P`s), `javaos_vm_test_program` (8 `P`s), `pthread`/`wait_queue`/
      `file_syscalls_demo`/`user_posix_demo`/DLOPEN-DLSYM/`libc_test_program` — todos
      com exatamente os mesmos resultados de antes desta change.

## 6. Documentação

- [x] 6.1 Adicionar a este change (`design.md`, "Resultado real da implementação")
      a confirmação do que foi verificado em boot real e o relato de qualquer achado
      real durante a implementação.
- [x] 6.2 Atualizar `ROADMAP.md` — mover o item concluído ("Primitiva de espera com
      prazo em `kernel::wait_queue`") para a seção de implementado, atualizar a nota
      de `PlatformMonitor::wait`/`PlatformEvent`/`PlatformParker` no destaque "o que
      falta para a JVM rodar" (item 2) para refletir que só `PlatformEvent`/
      `PlatformParker` continuam pendentes, e apontar o próximo item real de médio
      prazo como o próximo passo — verificar que o documento permanece consistente.

## Workflow follow-up

- Arquivar esta change (`openspec archive kernel-timed-wait`) depois que a
  verificação em boot real (tarefas 5.3/5.4) e `cargo test -p kernel --lib` (tarefa
  1.6/2.3) estiverem confirmadas.
