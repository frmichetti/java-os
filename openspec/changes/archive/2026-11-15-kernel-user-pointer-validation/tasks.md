# Tasks

## 1. Validação de intervalo de memória de usuário em `kernel::paging`

- [x] 1.1 Adicionar `fn spanned_pages(ptr: u64, len: u64) -> Option<(u64, u64)>`
      (primeira página alinhada, quantidade de páginas tocadas pelo intervalo
      `[ptr, ptr+len)`; `None` se `ptr + len` estourar `u64`) — função livre,
      sem `self`. Verificar com `cargo test -p kernel --lib`, testes
      cobrindo: intervalo inteiro dentro de uma única página, intervalo
      atravessando duas páginas, intervalo começando no meio de uma página e
      terminando no início da seguinte, `len` que faz `ptr + len` estourar
      `u64` (devolve `None`). **Confirmado**: `cargo test -p kernel --lib`
      com 5 novos testes passando (`spanned_pages_within_a_single_page`,
      `_crossing_a_page_boundary`, `_ending_exactly_at_the_start_of_the_next_page`,
      `_rejects_an_overflowing_range`, `_rejects_zero_length`).
- [x] 1.2 Adicionar `pub fn validate_user_range(&self, ptr: u64, len: u64,
      require_writable: bool) -> bool` a `kernel::paging::Mapper`: `len == 0`
      sempre devolve `true` sem chamar `spanned_pages`; caso contrário, usa
      `spanned_pages` e, para cada página do intervalo, caminha as 4 tabelas
      (mesmo código de `translate`, adaptado para expor as *flags* da
      entrada folha, não só o endereço físico) checando `PTE_PRESENT` +
      `PTE_USER` (+ `PTE_WRITABLE` se `require_writable`) — primeira página
      que falhar já retorna `false`. Verificar com `cargo test -p kernel
      --lib` que compila; a caminhada real de tabelas só é verificável em
      boot real (tarefa 3), mesma limitação já documentada de
      `translate`/`map`/`unmap`. **Confirmado**: extraído `translate_pte`
      (caminhada compartilhada por `translate`/`validate_user_range`);
      `cargo test -p kernel --lib` com 287 testes passando (281 de antes +
      5 de `spanned_pages` + 1 de `validate_user_range` com `len == 0`, o
      único caso testável em host sem tocar `CR3`).

## 2. `SYSCALL_EFAULT` e validação em cada *syscall* que recebe ponteiro de usuário

- [x] 2.1 Adicionar `pub const SYSCALL_EFAULT: u64 = u64::MAX - 2;` a
      `kernel::syscall` (distinto de `SYSCALL_ERROR`/`SYSCALL_TIMEOUT`) —
      verificar que o valor não colide com nenhum sentinela existente.
      **Confirmado**. Junto, uma pequena função privada
      `user_range_is_valid(ptr, len, require_writable)` (lock do
      `FRAME_ALLOCATOR` + `Mapper::new` + `validate_user_range`) para
      evitar repetir esse bloco em cada uma das 5 chamadas abaixo —
      **achado real, simplificação não prevista no design.md original**.
- [x] 2.2 `sys_write` valida `ptr`/`len` (leitura) com `validate_user_range`
      antes de `core::slice::from_raw_parts`, devolvendo `SYSCALL_EFAULT` se
      inválido — verificar com `cargo test -p kernel --lib` que os testes
      já existentes de `sys_write`/`dispatch` (se houver) continuam
      passando.
- [x] 2.3 `sys_dlopen`/`sys_dlsym` validam seu(s) ponteiro(s) de
      nome/caminho (leitura) antes de `core::slice::from_raw_parts`,
      devolvendo `SYSCALL_EFAULT` se inválido — verificar com `cargo test -p
      kernel --lib`.
- [x] 2.4 `sys_open` valida `path_ptr`/`path_len` (leitura) antes de
      `core::slice::from_raw_parts`, devolvendo `SYSCALL_EFAULT` se inválido
      — verificar com `cargo test -p kernel --lib`.
- [x] 2.5 `sys_read` valida `buf_ptr`/o tamanho efetivamente lido (escrita,
      `require_writable: true`) antes de `core::slice::from_raw_parts_mut`,
      devolvendo `SYSCALL_EFAULT` se inválido, sem avançar o cursor do
      arquivo nem ler nada dele antes dessa checagem — verificar com `cargo
      test -p kernel --lib` que os testes já existentes de leitura
      continuam passando sem alteração. **Confirmado**: validação usa
      `buf_len` (o tamanho pedido), não a quantidade efetivamente lida —
      roda antes de `crate::file_handles::read`, então uma falha de
      validação nunca toca o arquivo. `cargo test -p kernel --lib` com 287
      testes passando (281 de antes + 6 de `kernel::paging`), nenhum teste
      de `syscall::tests` precisou de ajuste.

## 3. Verificação em boot real

- [x] 3.1 Novo programa de teste em `userland_c/` que invoca uma dessas
      cinco chamadas (ex.: `SYSCALL_WRITE`) com um ponteiro deliberadamente
      inválido (ex.: `0x0`, ou um endereço nunca mapeado pelo seu próprio
      espaço de endereçamento) e confirma que o retorno é `SYSCALL_EFAULT`
      — adicionar ao `xtask`/imagem de boot, seguindo o mesmo padrão dos
      demais programas de teste (`fault_test_program.c`, etc.). **Confirmado**:
      `userland_c/efault_test_program.c` (carregado como `/bin/efault.elf`,
      `kernel::scheduler::EFAULT_TEST_PROGRAM_STACK_VIRT`, spawn em
      `kernel/src/main.rs` logo após `fault.elf`) cobre 4 cenários via
      syscall bruta (sem passar pelos wrappers de `glue.c`, para distinguir
      `SYSCALL_EFAULT` de `SYSCALL_ERROR`): ponteiro válido continua
      funcionando (`SYS_WRITE`), ponteiro nulo rejeitado (`SYS_WRITE`),
      ponteiro de caminho inválido rejeitado (`SYS_OPEN`), e buffer de
      leitura inválido rejeitado sem consumir o cursor do arquivo
      (`SYS_READ` — confirmado lendo o primeiro byte real do arquivo fixture
      logo depois da tentativa inválida).
- [x] 3.2 Rodar `cargo xtask test` em QEMU e confirmar: (a) o novo programa
      reporta `SYSCALL_EFAULT` sem derrubar o kernel nem gerar nenhuma
      `EXCECAO #PF`/pânico; (b) todo programa de teste já existente continua
      reportando exatamente os mesmos resultados de antes desta change —
      prova de que a validação não introduz nenhuma regressão para ponteiros
      válidos. **Confirmado**: `smoke test de boot: PASSOU`;
      `efault_test_program: resultado PPPP (ordem: write_valido,write_nulo,
      open_invalido,read_invalido_sem_consumir_cursor)`; todo programa de
      teste já existente reportou exatamente o mesmo padrão de antes
      (`file_syscalls_demo: resultado PPPPPPPP`, `javaos_vm_test_program:
      resultado PPPPPPPP`, `user_posix_demo: resultado PPPPPPPPPPPP`,
      `libc_test_program: resultado PPPP`, etc.); o único `EXCECAO #PF` do
      log é o já esperado, deliberado, de `fault_test_program.c`
      (`kernel-fault-isolation`), sem nenhuma relação com esta change.

## 4. Documentação

- [x] 4.1 Adicionar a este change (`design.md`, "Resultado real da
      implementação") a confirmação do que foi verificado (testes de host +
      boot real) e o relato de qualquer achado real durante a
      implementação.
- [x] 4.2 Atualizar `ROADMAP.md` — mover a parte de validação de ponteiro de
      usuário do item "Esquema de erro granular (`errno`-like) nas
      syscalls, e validação de ponteiro de usuário" para a seção de
      implementado (deixando claro que o esquema de erro em si continua só
      com `SYSCALL_ERROR`/`SYSCALL_TIMEOUT`/`SYSCALL_EFAULT`, não um
      conjunto amplo ao estilo POSIX — ver design.md, "Alternativas
      consideradas"), e apontar o próximo item real de médio prazo como o
      próximo passo — verificar que o documento permanece consistente.
      **Confirmado**: nova seção "Validação de ponteiro de usuário em
      argumentos de syscall" adicionada à parte de implementado;
      "Próximo passo imediato" agora aponta o esquema de erro granular mais
      amplo (estilo POSIX) como item restante, deixando explícito que é um
      escopo maior, deliberadamente adiado.

## Workflow follow-up

- Arquivar esta change (`openspec archive kernel-user-pointer-validation`)
  depois que a verificação em boot real (tarefas 3.1-3.2) e `cargo test -p
  kernel --lib` estiverem confirmadas.
