# Tasks

## 1. Reconhecimento do segmento `PT_TLS` no carregador de ELF

- [x] 1.1 Adicionar `PT_TLS` (tipo 7) e uma função `load_tls_segment(buf,
  header) -> Result<Option<TlsSegment>, ElfError>` em `kernel::elf`,
  devolvendo `memsz`/`align`/a fatia de `filesz` bytes da imagem inicial
  (ou `None` se nenhum segmento `PT_TLS` existir); extrair `p_align`
  (bytes 48-56 do `Elf64_Phdr`, hoje nunca lido). Reaproveitar a mesma
  validação de limites já usada por `load_program_headers`
  (`p_memsz < p_filesz`, offsets dentro de `buf`); verificar com testes de
  host cobrindo executável sem `PT_TLS` (devolve `None`, comportamento
  idêntico a antes), com `PT_TLS` presente (dados extraídos corretamente),
  e com segmento malformado (rejeitado, mesmos códigos de erro já usados
  por `PT_LOAD`).

## 2. Janela de endereços e construção do bloco de TLS

- [x] 2.1 Adicionar `TLS_BLOCK_POOL_BASE`/`TLS_BLOCK_POOL_STRIDE`/
  `TLS_TCB_OFFSET` e um contador global monotônico de próximo slot em
  `kernel::stack` (mesmo padrão de `USER_THREAD_STACK_POOL_BASE`), mais
  uma função pura que calcula o endereço do TCB e o início da imagem
  dentro de um slot, dado `memsz`/`align`; verificar com teste de host que
  dois slots consecutivos nunca se sobrepõem e que o endereço do início da
  imagem sempre respeita o `align` pedido.
- [x] 2.2 Adicionar uma função que aloca o próximo slot, mapeia sua página
  (via `Mapper::for_root`, mesmo racional de `next_user_thread_stack_base`
  — a janela é privada por processo), copia a imagem inicial, zera o
  restante até `memsz`, escreve o auto-ponteiro do TCB, e devolve o
  endereço do TCB (futuro `Context::tls_base`); rejeitar `memsz >
  TLS_TCB_OFFSET` como segmento malformado (feito em `spawn_from_elf`, ao
  construir o `TlsTemplate`, não dentro desta função). Implementada como
  `kernel::process::init_tls_block` (não em `kernel::stack` — mesma
  correção de design já aplicada em `kernel-user-thread-create`: mapear
  exige a `root` do processo, que só quem já está montando o `Context` da
  thread tem). Verificado em boot real (tarefa 4.3) — **achado real**:
  `map_new`/a travessia de tabelas de página nunca foi testável em host
  neste projeto (nenhum teste existente jamais a chama com escrita real de
  memória — mesma limitação já documentada para `translate`/
  `validate_user_range`), então não há teste de host possível aqui, só em
  boot real.

## 3. Bloco de TLS por thread em `kernel::process`/`kernel::scheduler`

- [x] 3.1 Adicionar `tls_template: Option<TlsTemplate>` em `Process`
  (`memsz`/`align`/imagem inicial copiada, não um empréstimo do buffer do
  executável) e `tls_base: u64` em `Context` (`0` = nenhum bloco).
  `create_user_thread` passa a receber `Option<&TlsTemplate>` e, quando
  presente, aloca o bloco (tarefa 2.2) antes de montar o `Context`.
  `finish_new_user` constrói o `TlsTemplate` a partir do que a tarefa 1.1
  extraiu; `spawn_user_thread` lê `self.tls_template.as_ref()`. Verificado
  por compilação (`cargo test -p kernel --lib`, 337 testes, todos os sites
  de `Process::finish_new_user` atualizados) e em boot real (tarefa 4.3) —
  mesmo achado de não-testabilidade em host da tarefa 2.2 acima (a própria
  `create_user_thread` já não era testável em host antes desta change,
  pelo mesmo motivo).
- [x] 3.2 Em `schedule_next`, trocar `FS.base` via `crate::apic::wrmsr`
  (MSR `IA32_FS_BASE = 0xC000_0100`) para `contexts[next.0 as
  usize].tls_base`, no mesmo trecho que já troca `CR3`/`TSS.rsp0`.
  Verificado em boot real (tarefa 4.3) — não testável em host (`wrmsr` é
  instrução privilegiada, mesmo motivo já documentado para
  `with_interrupts_disabled`/`cli` em `CURRENT_PROCESS_ID`) que threads
  sem TLS continuam funcionando normalmente (`FS.base = 0` nunca
  desreferenciado por elas).

## 4. `userland_c/linker.ld` e programa de teste de TLS

- [x] 4.1 Adicionar seções `.tdata`/`.tbss` explícitas a
  `userland_c/linker.ld`, com `ALIGN` antes de cada uma (mesmo racional já
  documentado no arquivo para `.bss`); verificar com `readelf -l` (ou
  `kernel::elf::load_program_headers`/a nova `load_tls_segment` da tarefa
  1.1 rodada sobre o executável compilado) que um programa com
  `__thread int x;` agora produz um segmento `PT_TLS` com o `p_align`
  esperado.
- [x] 4.2 Criar `userland_c/tls_test_program.c`: declara `__thread int
  contador;`, a thread principal escreve um valor nele, cria uma
  thread-irmã via `SYSCALL_THREAD_CREATE` que escreve um valor diferente
  no seu próprio `contador` e reporta de volta (variável global comum,
  mesmo padrão de poll limitado de `sibling_thread_test_program.c`), e a
  thread principal confirma que seu próprio `contador` continua com o
  valor que ela escreveu, não o da thread-irmã.
- [x] 4.3 Adicionar `tls_test_program` ao processo de build (`xtask`) e à
  lista de programas verificados em boot real; verificar em QEMU que o
  programa reporta sucesso (cada thread via valor independente do
  `__thread int`), e que todos os programas de teste já existentes
  continuam reportando exatamente as mesmas strings P/F de antes.

## 5. Documentação

- [x] 5.1 Atualizar `ROADMAP.md`: registrar TLS por thread como
  concluída, e apontar `pthread_create`/`os::create_thread`/
  `os::pd_start_thread` real + lock de `malloc`/`_sbrk` como o novo
  próximo passo.
- [x] 5.2 Registrar em `design.md` desta change (seção "Resultado real da
  implementação") qualquer achado descoberto durante a implementação ou
  verificação em boot real.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive kernel-thread-local-storage`)
  depois que o usuário revisar a implementação.
