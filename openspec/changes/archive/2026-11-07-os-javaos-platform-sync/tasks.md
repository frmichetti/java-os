# Tasks

## 1. Nova syscall `SYSCALL_MUTEX_TRYLOCK`

- [x] 1.1 Tornar `kernel::pthread::Mutex::try_acquire` (`kernel/src/pthread.rs`)
      `pub(crate)` em vez de privado — verificar que `cargo kernel-build` continua
      compilando sem warning de método não usado.
- [x] 1.2 Adicionar `pub const SYSCALL_MUTEX_TRYLOCK: u64 = 22;` a
      `kernel/src/syscall.rs` (próximo número livre, após `SYSCALL_EXIT = 21`) e uma
      função `sys_mutex_trylock(handle: u64) -> u64` que chama
      `crate::pthread_handles::mutex(handle)` e, se encontrado,
      `mutex.try_acquire()` — devolve `0` em sucesso, `SYSCALL_ERROR` se o handle for
      desconhecido ou o mutex já estiver tomado — registrar no `dispatch` — verificar
      com um teste em `kernel/src/syscall.rs` (mesmo padrão dos testes existentes de
      `SYSCALL_VM_*`): tentativa sobre mutex livre tem sucesso, segunda tentativa
      sobre o mesmo mutex (já tomado) falha sem bloquear, handle desconhecido falha.
- [x] 1.3 Executar `cargo test -p kernel --lib` e confirmar que o novo teste passa e
      nenhum teste existente quebra.

## 2. `pthread_mutex_*`/`pthread_cond_*` reais em `userland_c/glue.c`

- [x] 2.1 Adicionar `SYS_MUTEX_CREATE=7`/`LOCK=8`/`UNLOCK=9`/`TRYLOCK=22`,
      `SYS_COND_CREATE=10`/`WAIT=11`/`SIGNAL=12`/`BROADCAST=13` a `glue.c` — mesma
      convenção dos números já existentes, batendo com `kernel/src/syscall.rs`.
- [x] 2.2 Incluir `<pthread.h>`/`<sys/_pthreadtypes.h>` em `glue.c` e implementar
      `pthread_mutex_init`/`pthread_mutex_destroy`/`pthread_mutex_lock`/
      `pthread_mutex_unlock`/`pthread_mutex_trylock`, com uma função auxiliar privada
      (`static`) que resolve um `pthread_mutex_t*` para um handle real, criando um
      mutex novo via `SYS_MUTEX_CREATE` se o valor atual for
      `PTHREAD_MUTEX_INITIALIZER` (`0xFFFFFFFF`) — `destroy` devolve `0` sem emitir
      nenhuma syscall (ver design.md) — verificar com
      `scripts/cross/x86_64-elf-gcc -c glue.c` (mesma invocação de
      `os-javaos-memory-pd`) sem erro. **Achado real durante a implementação**: o
      `pthread.h` do newlib vendorizado envolve todo o seu conteúdo (protótipos e
      macros, incluindo `PTHREAD_MUTEX_INITIALIZER`) em `#if defined(_POSIX_THREADS)`,
      que não é definido automaticamente para o alvo genérico `x86_64-elf` deste
      projeto — corrigido definindo `_POSIX_THREADS 1` no próprio `glue.c` antes do
      `#include <pthread.h>` (documentado no comentário do código; seguro porque
      `libc.a` não foi buildado com `--enable-newlib-multithread`, sem símbolo
      `pthread_*` próprio para colidir).
- [x] 2.3 Implementar `pthread_cond_init`/`pthread_cond_destroy`/
      `pthread_cond_signal`/`pthread_cond_broadcast`/`pthread_cond_wait`, com a mesma
      função auxiliar de inicialização preguiçosa adaptada para `pthread_cond_t`.
      `pthread_cond_wait` chama `SYS_COND_WAIT` com os dois handles (cond e mutex,
      resolvendo a inicialização preguiçosa de ambos antes) — verificar compilação
      como acima.

## 3. `PlatformMutex`/`PlatformMonitor` reais no porte `os_javaos`

- [x] 3.1 Criar `vendor/jdk-27-ga/src/hotspot/os/javaos/mutex_javaos.cpp` com
      `PlatformMutex::PlatformMutex()` (`pthread_mutex_init`), `~PlatformMutex()`
      (`pthread_mutex_destroy`), `lock()` (`pthread_mutex_lock`), `unlock()`
      (`pthread_mutex_unlock`), `try_lock()` (`pthread_mutex_trylock`) — verificar
      com `g++ -fsyntax-only -std=gnu++17 ...` (mesma invocação de
      `os-javaos-memory-pd`). **Achado**: diferente de `os_javaos.cpp`/`javaos_vm.h`,
      `mutex_javaos.hpp` inclui `<pthread.h>` direto do glibc do *host* (não do
      newlib vendorizado) — nenhum `-I`/`-D_POSIX_THREADS` extra é necessário aqui,
      o glibc do host já expõe tudo sem a lacuna de feature-test do newlib (ver
      design.md). **Achado 2**: o mesmo bug de comentário C (`*/` dentro de
      `pthread_mutex_*/pthread_cond_*`) que já tinha aparecido em `glue.c` se
      repetiu no comentário de topo deste arquivo, causando uma cascata de dezenas
      de erros não relacionados (o resto do arquivo virou código "fora" do
      comentário) — corrigido evitando a sequência `_*/` em qualquer comentário C
      novo.
- [x] 3.2 No mesmo arquivo, `PlatformMonitor::PlatformMonitor()`
      (`pthread_cond_init` além do `pthread_mutex_init` herdado),
      `~PlatformMonitor()` (`pthread_cond_destroy`), `notify()`
      (`pthread_cond_signal`), `notify_all()` (`pthread_cond_broadcast`) — verificar
      compilação como acima.
- [x] 3.3 `PlatformMonitor::wait(uint64_t millis)`: se `millis == 0`, chamar
      `pthread_cond_wait(cond(), mutex())` de verdade e devolver o código de sucesso
      esperado pela interface (`OS_OK`/equivalente, conferir
      `share/runtime/os.hpp`); se `millis != 0`, `fatal("PlatformMonitor::wait: espera com prazo nao implementada")`
      — verificar compilação como acima.

## 4. Verificação em boot real

- [x] 4.1 Criar `userland_c/javaos_pthread_test_program.c` (mesmo padrão de
      `javaos_vm_test_program.c`): inclui `<pthread.h>`, exercita em sequência
      `pthread_mutex_init`, `pthread_mutex_lock`, `pthread_mutex_trylock` (deve
      falhar — já tomado pela própria thread), `pthread_mutex_unlock`,
      `pthread_mutex_trylock` (deve ter sucesso — livre de novo),
      `pthread_mutex_unlock`, `pthread_mutex_destroy`, `pthread_cond_init`,
      `pthread_cond_signal`, `pthread_cond_broadcast`, `pthread_cond_destroy` — cada
      cenário reporta `P`/`F` numa única mensagem via `write()`, mesmo formato de
      `javaos_vm_test_program.c`. **Não chama `pthread_cond_wait`** (ver design.md,
      "Verificação em boot real" — bloquearia o processo para sempre, sem nenhuma
      outra execução para notificá-lo). Verificado com compilação + link completo
      (`crt0.o`+`glue.o`+`libc.a`) via o toolchain cruzado, sem erro. **Mesmo bug de
      comentário C (`*/` dentro de `pthread_mutex_*/pthread_cond_*`) apareceu pela
      terceira vez nesta change** (já visto em `glue.c` e `mutex_javaos.cpp`) — na
      primeira linha do comentário de topo deste arquivo; corrigido.
- [x] 4.2 Adicionar `build_javaos_pthread_test_program` a `xtask/src/main.rs` (mesmo
      corpo de `build_javaos_vm_test_program`), chamar a partir de `build_image`, e
      copiar o ELF resultante para `::/bin/jpthread.elf` — verificar que
      `cargo xtask build-image` produz o arquivo sem erro. **Achado real**: o nome
      originalmente planejado (`javaospthread.elf`, base de 13 caracteres) excede o
      limite de nomes curtos 8.3 que o driver FAT32 do kernel suporta (ver
      `kernel-filesystem`) — `mcopy` aceita o nome (gera um alias curto VFAT
      diferente), mas o carregador ELF do kernel, que só entende 8.3 literal,
      falharia com `NotFound` ao procurar o nome original. Corrigido usando
      `jpthread.elf` (8+3) desde o início, mesmo padrão de `javaosvm.elf`.
- [x] 4.3 Adicionar `JAVAOS_PTHREAD_TEST_PROGRAM_STACK_VIRT`/`_STACK_SIZE` a
      `kernel/src/scheduler.rs` (endereço distinto dos já existentes) e carregar
      `/bin/jpthread.elf` via `kernel::scheduler::spawn_from_elf` em
      `kernel/src/main.rs`, mesma disciplina de erro explícito (nunca pânico) dos
      carregamentos ELF já existentes ali — verificar via `cargo xtask run` em QEMU
      que a mensagem de resultado aparece no console serial com todos os cenários
      `P`, e que nenhum `EXCECAO`/pânico ocorre, nem antes nem depois dela no log de
      boot.

## 5. Documentação

- [x] 5.1 Adicionar a este change (`design.md`, "Resultado real da implementação",
      mesmo padrão de `os-javaos-memory-pd`) a confirmação de quais cenários foram
      verificados em boot real e o relato de qualquer achado real durante a
      implementação.
- [x] 5.2 Atualizar `ROADMAP.md` — mover o item concluído ("`PlatformMutex`/
      `PlatformMonitor`/`PlatformEvent`/`PlatformParker` de verdade") para a seção de
      implementado, deixando claro que só `PlatformMutex`/`PlatformMonitor` foram
      cobertos (`PlatformEvent`/`PlatformParker` continuam pendentes, já que dependem
      de espera com prazo), e apontar `libm`/conversão float (item 3 do roadmap
      médio prazo) como o próximo passo real — verificar que o documento permanece
      consistente (sem referência pendente a um "próximo passo" já concluído), e
      atualizar a contagem de syscalls (22 → 23) onde mencionada.

## Workflow follow-up

- Arquivar esta change (`openspec archive os-javaos-platform-sync`) depois que a
  verificação em boot real (tarefa 4.3) e a compilação `-fsyntax-only` dos corpos de
  `PlatformMutex`/`PlatformMonitor` (tarefas 3.1–3.3) estiverem confirmadas.
