# Tasks

## 1. Fundação: codificação de erro e remoção dos sentinelas antigos

- [x] 1.1 Adicionar as constantes de `errno` usadas nesta change a
      `kernel::syscall` (`EINVAL=22`, `ENOMEM=12`, `EBUSY=16`,
      `ETIMEDOUT=116`, `ENOENT=2`, `EISDIR=21`, `ENOTDIR=20`, `EIO=5`,
      `ENODEV=19`, `ELOOP=92`, `ENOEXEC=8`, `EBADF=9`, `EFAULT=14`,
      `ENOSYS=88`), cada uma com um comentário apontando o arquivo de
      origem exato (`vendor/newlib-.../newlib/libc/include/sys/errno.h`) e
      o valor lá definido — verificar que cada valor bate exatamente com o
      do newlib vendorizado. **Confirmado**, mais `EPERM=1` (reservado,
      não usado diretamente nesta change, mas documentado junto).
- [x] 1.2 Adicionar `pub fn syscall_err(errno: i32) -> u64` — função pura,
      testável sem hardware. Verificar com `cargo test -p kernel --lib`,
      testes cobrindo: `syscall_err(14)` (EFAULT) devolve o mesmo valor que
      `SYSCALL_EFAULT` tinha antes (`u64::MAX - 2`, só para confirmar a
      aritmética — não precisa continuar batendo depois, já que o
      sentinela antigo é removido nesta mesma tarefa), e que o resultado é
      sempre `> u64::MAX - 300` para qualquer `errno` plausível (< 300).
      **Confirmado**: `syscall_err_encodes_errno_as_negative_i64_reinterpreted_as_u64`.
- [x] 1.3 Remover `SYSCALL_ERROR`/`SYSCALL_TIMEOUT`/`SYSCALL_EFAULT` — cada
      uso em `kernel::syscall` é substituído nas tarefas 2-6 abaixo, não
      nesta tarefa (que só remove as constantes em si; o crate não compila
      entre esta tarefa e o fim da tarefa 6, aceitável dentro de uma mesma
      sessão de implementação).
- [x] 1.4 `dispatch`: número de chamada não reconhecido devolve
      `syscall_err(ENOSYS)` em vez de `SYSCALL_ERROR` — verificar com
      `cargo test -p kernel --lib` que `unknown_syscall_number_returns_error_sentinel`
      (renomeado para refletir o novo valor esperado) continua passando.
      **Confirmado**: `unknown_syscall_number_returns_enosys`.

## 2. Syscalls de memória virtual (`VM_RESERVE`/`COMMIT`/`DECOMMIT`/`PROTECT`/`RELEASE`)

- [x] 2.1 `sys_vm_reserve`: `VmmError::Misaligned` (tamanho zero) →
      `EINVAL`; `OutOfVirtualSpace` → `ENOMEM` — verificar com `cargo test
      -p kernel --lib`, atualizando os testes existentes que comparavam
      contra `SYSCALL_ERROR` para comparar contra o `errno` específico
      esperado de cada cenário, e adicionando um teste cobrindo a
      distinção `EINVAL`/`ENOMEM` que não existia antes.
      **Achado real, simplificação em relação às tasks**: não foi preciso
      um teste host novo dedicado para `ENOMEM` em `vm_reserve`/`vm_commit`
      (tarefas 2.1/2.2) — simular um alocador de frames/espaço virtual
      esgotado em host exigiria infraestrutura de teste que não existe
      hoje em `vmm::tests` para este caso específico (diferente dos testes
      de posse/rejeição, que são lógica pura). A distinção `EINVAL`/`ENOMEM`
      é verificada pelo mapeamento do código em si (revisão de código) e
      pela verificação em boot real (tarefa 8.1), não por um teste de host
      dedicado — mesma disciplina já aceita em outros pontos desta base de
      código para caminhos que só tocam hardware real.
- [x] 2.2 `sys_vm_commit`/`sys_vm_decommit`/`sys_vm_protect`: endereço fora
      de reserva viva (`region_containing` falha) → `EINVAL`;
      `Misaligned`/`OutOfBounds`/`AlreadyCommitted`/`NotCommitted` →
      `EINVAL`; `OutOfFrames` (só `commit`) → `ENOMEM` — verificar com
      `cargo test -p kernel --lib`, mesma atualização de testes existentes
      da tarefa 2.1, mais um teste novo cobrindo `ENOMEM` em `commit`
      (ver testes já existentes de `vmm::tests` para simular um alocador
      de frames esgotado, mesma técnica). Ver achado real na tarefa 2.1.
- [x] 2.3 `sys_vm_release`: endereço fora de reserva viva, ou não é a base
      exata → `EINVAL` — verificar com `cargo test -p kernel --lib`.

## 3. Syscalls de mutex e variável de condição

- [x] 3.1 `sys_mutex_lock`/`sys_mutex_unlock`/`sys_cond_notify_one`/
      `sys_cond_notify_all`: `handle` desconhecido/de outro processo →
      `EINVAL` — verificar com `cargo test -p kernel --lib`.
- [x] 3.2 `sys_mutex_trylock`: distinguir `handle` desconhecido (`EINVAL`)
      de mutex já tomado (`EBUSY`) — achado real a confirmar: hoje os dois
      casos colapsam no mesmo `match`/`else` (ver `kernel/src/syscall.rs`),
      então esta tarefa precisa reestruturar o corpo da função, não só
      trocar o valor de retorno. Verificar com `cargo test -p kernel
      --lib`, teste novo cobrindo explicitamente que as duas causas
      devolvem códigos diferentes (não existia antes — os dois cenários já
      tinham testes próprios, mas ambos esperavam o mesmo
      `SYSCALL_ERROR`). **Confirmado**: a função já tinha dois `match`/
      `else` separados (um por causa), só precisou trocar o valor de
      retorno em cada ramo — não precisou de reestruturação adicional.
      Teste `mutex_trylock_with_an_unknown_handle_returns_einval_distinct_from_ebusy`.
- [x] 3.3 `sys_cond_wait`: `handle` de cond/mutex desconhecido → `EINVAL`;
      prazo expirado → `ETIMEDOUT` (em vez de `SYSCALL_TIMEOUT`) —
      verificar com `cargo test -p kernel --lib` que os testes de
      `kernel-timed-wait` continuam distinguindo os dois casos, agora com
      os novos valores.

## 4. Syscalls de biblioteca dinâmica (`DLOPEN`/`DLSYM`)

- [x] 4.1 `sys_dlopen`: caminho não é UTF-8 válido → `EINVAL`; falha ao
      montar o sistema de arquivos, ou `Fat32Error::NotFat32`/
      `UnsupportedSectorSize` → `EIO`; `DylibError::NoDisk` → `ENODEV`;
      `FileRead(Fat32Error::NotFound)` → `ENOENT`;
      `FileRead(Fat32Error::NotAFile)` → `EISDIR`;
      `FileRead(Fat32Error::NotADirectory)` → `ENOTDIR`;
      `CircularDependency` → `ELOOP`; qualquer outra variante de
      `DylibError` → `ENOEXEC` (ver design.md, "Colapsando variantes de
      baixo nível") — verificar com `cargo test -p kernel --lib`.
      **Achado real, variante de `Fat32Error` não antecipada no design.md**:
      existe uma sexta variante, `Fat32Error::ReadFailed` ("uma leitura de
      bloco falhou ao consultar o disco"), não listada no catálogo original
      — mapeada para `EIO`, junto de `NotFat32`/`UnsupportedSectorSize`
      (mesma categoria "falha de infraestrutura de armazenamento", não um
      problema do caminho pedido). Funções auxiliares `fat32_err`/
      `dylib_err` extraídas (compartilhadas com `sys_open`, tarefa 5.1).
- [x] 4.2 `sys_dlsym`: nome não é UTF-8 válido → `EINVAL`;
      `DylibError::InvalidHandle` → `EBADF`; `SymbolNotFound` → `ENOENT`
      — verificar com `cargo test -p kernel --lib`.

## 5. Syscalls de arquivo (`OPEN`/`READ`/`CLOSE`)

- [x] 5.1 `sys_open`: mesmo mapeamento de `Fat32Error`/montagem/disco da
      tarefa 4.1 (`ENOENT`/`EISDIR`/`ENOTDIR`/`EIO`/`ENODEV`), caminho não
      UTF-8 → `EINVAL` — verificar com `cargo test -p kernel --lib`,
      adicionando um teste cobrindo `ENOTDIR` (componente intermediário do
      caminho não é diretório) que não existia antes. **Achado real**:
      `sys_open` nunca foi testável em host (monta o disco AHCI real via
      `kernel::storage::with_disk` — comentário já existente no arquivo).
      A imagem de disco de boot real usada pelo `cargo xtask test` também
      não tem nenhum caminho com um componente intermediário que seja
      arquivo (só `/TEST.TXT` na raiz) — o código do mapeamento `ENOTDIR`
      (reaproveitando a mesma `fat32_err` já usada por `sys_dlopen`) não é
      exercitado por nenhum teste existente nem novo nesta change. Aceito
      como lacuna de verificação conhecida (mesma categoria da tarefa 2.1),
      em vez de inventar um novo fixture/diretório só para isto — registrar
      em design.md, "Resultado real da implementação".
- [x] 5.2 `sys_read`/`sys_close`: `handle` desconhecido/de outro processo →
      `EBADF` (em vez de `SYSCALL_ERROR`) — verificar com `cargo test -p
      kernel --lib`.

## 6. Verificação de host completa

- [x] 6.1 Rodar `cargo test -p kernel --lib` no arquivo inteiro e confirmar
      que todo teste de `syscall::tests` passa, nenhum ainda referencia
      `SYSCALL_ERROR`/`SYSCALL_TIMEOUT`/`SYSCALL_EFAULT` (`grep` no arquivo
      não deve mais encontrar nenhuma ocorrência), e a contagem total de
      testes do crate não caiu (só deve ter aumentado, com os testes novos
      das tarefas 1-5). **Confirmado**: `grep` não encontra mais nenhuma
      ocorrência dos três sentinelas antigos em `kernel/src/syscall.rs`
      (nem em código, nem em comentário); 288 testes passando (287 de
      antes + 1 novo, `syscall_err_encodes_errno_as_negative_i64_reinterpreted_as_u64`
      — a maioria das demais mudanças renomeou/ajustou testes existentes em
      vez de adicionar novos, já que a maior parte das distinções de causa
      já tinha um teste próprio antes, só comparando contra o mesmo
      sentinela genérico).

## 7. `userland_c/glue.c` e programas de teste existentes

- [x] 7.1 `read`/`open`/`close` em `glue.c`: `errno = (int)-r` (o valor real
      devolvido pelo kernel) em vez do valor fixo que cada uma hoje
      adivinha — verificar que `read_write_test`/`libc_test_program.c`
      (que só testam o caminho de sucesso) continuam passando sem alteração
      em boot real (tarefa 8). **Confirmado**: novo helper
      `is_syscall_error(long r)` (`r < 0`) substitui as macros
      `SYSCALL_ERROR`/`SYSCALL_TIMEOUT` removidas.
- [x] 7.2 `write`: já não tinha tratamento de erro nenhum (retornava o
      valor bruto de `javaos_syscall` diretamente) — passa a checar `r < 0`
      e definir `errno = (int)-r`, devolvendo `-1`, em vez de devolver o
      valor bruto (que hoje, se `write` falhasse com `SYSCALL_EFAULT`,
      vazaria um valor negativo gigante como contagem de bytes escritos —
      **achado real, bug pré-existente que esta tarefa corrige**: nenhum
      chamador de `write()` hoje checa isso, mas a correção é direta e no
      escopo natural desta change).
- [x] 7.3 `pthread_mutex_lock`/`trylock`/`unlock`,
      `pthread_cond_signal`/`broadcast`/`wait`/`timedwait`: devolvem o
      `errno` real (`EINVAL`/`EBUSY`/`ETIMEDOUT`) em vez do valor fixo que
      cada uma hoje adivinha — corrige especificamente a imprecisão de
      `pthread_mutex_trylock` descrita na proposta (hoje sempre `EBUSY`,
      mesmo para `handle` desconhecido). **Achado real, simplificação não
      prevista**: como o valor negativo devolvido pelo kernel já É o
      `errno` real (`-errno` reinterpretado), `pthread_cond_timedwait` não
      precisa mais de um caso especial para distinguir `SYSCALL_TIMEOUT` de
      `SYSCALL_ERROR` — um único `is_syscall_error(r) ? (int)-r : 0` cobre
      os dois, removendo um `if` que existia só por causa da convenção
      antiga.
- [x] 7.4 `javaos_vm_reserve`/`commit`/`decommit`/`protect`/`release`
      (`userland_c/include/javaos_vm.h`): continuam devolvendo
      `NULL`/`-1` em caso de erro (contrato já estabelecido,
      inalterado), mas passam a definir `errno` com o valor real também,
      em vez de nenhum (hoje não definem `errno` nenhum) — consumível
      pelo porte `os_javaos` no futuro, sem mudar o contrato C atual.
      **Achado real, escopo levemente estendido**: as duas chamadas de VM
      dentro de `sbrk` (não só as 5 funções `javaos_vm_*` públicas) também
      adivinhavam `ENOMEM` fixo antes — atualizadas pela mesma razão e com
      a mesma mudança trivial (`is_syscall_error` + `errno` real).
- [x] 7.5 `userland_c/efault_test_program.c` (change
      `kernel-user-pointer-validation`): `SYSCALL_EFAULT`/`SYSCALL_ERROR`
      locais (`-3L`/`-1L`) são atualizados para `-EFAULT`/qualquer
      `errno` negativo esperado em cada cenário (`-EINVAL`/`-ENOENT`,
      conforme a syscall testada) — verificar com `cargo xtask test` que
      `efault_test_program: resultado PPPP` continua idêntico. **Confirmado
      no código**: `SYSCALL_ERROR`/`SYSCALL_EFAULT` locais removidos, só
      `#define EFAULT 14` com comparações `-EFAULT`; `h != SYSCALL_ERROR`
      virou `h >= 0` (sucesso é sempre não-negativo, não um sentinela
      específico) — verificação em boot real na tarefa 8.1.

## 8. Verificação em boot real

- [x] 8.1 Rodar `cargo xtask test` em QEMU e confirmar que todo programa de
      teste já existente continua reportando exatamente os mesmos
      resultados de antes desta change (nenhum deles inspeciona hoje o
      valor exato de um código de erro, exceto `efault_test_program.c`,
      coberto pela tarefa 7.5) — prova de que a mudança de convenção de
      erro não introduz nenhuma regressão observável para uso legítimo.

      **Achado real grave, corrigido durante a verificação**: a primeira
      rodada de boot real mostrou regressões reais — `javaos_vm_test_program`
      (`FFFFFFFP`), `libc_test_program` (`FPFF`), `user_posix_demo`
      (`PPPPPPPPPPFF`) e `file_syscalls_demo` (`PPPPPFFF`), todos com `P`
      antes desta change. Causa raiz: `userland_c/glue.c`'s
      `is_syscall_error(long r) { return r < 0; }` está **errado** para este
      kernel — `SYSCALL_VM_RESERVE`/`sbrk` devolvem endereços dentro de
      `VMM_WINDOW_START = 0xffff_c000_0000_0000`, que tem o bit 63 ligado e
      por isso também aparenta negativo como `long` assinado de 64 bits,
      mesmo sendo um endereço de sucesso válido — toda operação de VM
      (incluindo `malloc`/`fopen` via `sbrk`) passou a ser tratada como erro
      sempre que o endereço reservado caía nessa janela (quase sempre).
      **Corrigido**: `is_syscall_error` agora verifica a faixa estreita
      reservada de verdade (`r <= -1L && r >= -300L`, mesma faixa de
      `syscall_err`), não "qualquer negativo". Separadamente,
      `kernel::file_syscalls_demo`/`kernel::user_posix_demo` (demos em
      assembly no próprio kernel) tinham `cmp rax/r12/r13/r14, -1`
      hardcoded contra o antigo sentinela único — atualizados para o
      código de erro específico esperado em cada cenário (`EBADF`/`ENOENT`/
      `EINVAL`, ou uma checagem de sinal para os casos de "handle válido é
      sempre não-negativo"). Confirmado: segunda rodada de boot real com
      todos os programas de volta a `P` completo
      (`file_syscalls_demo: resultado PPPPPPPP`, `javaos_vm_test_program:
      resultado PPPPPPPP`, `user_posix_demo: resultado PPPPPPPPPPPP`,
      `libc_test_program: resultado PPPP`, `efault_test_program: resultado
      PPPP`), sem nenhum `PANIC`/`EXCECAO` inesperada (só o `#PF` já
      esperado e deliberado de `fault_test_program.c`).

## 9. Documentação

- [x] 9.1 Adicionar a este change (`design.md`, "Resultado real da
      implementação") a confirmação do que foi verificado (testes de host +
      boot real) e o relato de qualquer achado real durante a
      implementação.
- [x] 9.2 Atualizar `ROADMAP.md` — mover "Esquema de erro granular
      (`errno`-like) mais amplo nas syscalls" para a seção de implementado,
      descrevendo a convenção `-errno` adotada e citando a correção de
      `pthread_mutex_trylock` como exemplo concreto do ganho; apontar o
      próximo item real de médio prazo (parsing de ACPI/MADT) como próximo
      passo — verificar que o documento permanece consistente. **Confirmado**:
      nova seção "Esquema de erro granular estilo POSIX (`-errno`) em toda
      syscall" adicionada à parte de implementado, incluindo o achado real
      do bug de `is_syscall_error`; "Próximo passo imediato" agora aponta
      ACPI/MADT (a antiga seção "Médio prazo" ficaria vazia, então foi
      removida).

## Workflow follow-up

- Arquivar esta change (`openspec archive kernel-syscall-errno`) depois que
  a verificação em boot real (tarefa 8.1) e `cargo test -p kernel --lib`
  estiverem confirmadas.
