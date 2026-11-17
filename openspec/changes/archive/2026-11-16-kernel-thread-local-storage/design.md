# Design

## Context

Ver `proposal.md` — "Why" para a motivação (bloqueio direto para
`os::create_thread`/`pthread_create` real, já que `Thread::current()` do
próprio HotSpot usa `__thread`).

Estado atual confirmado em código:

- `kernel::elf::load_program_headers` (`elf.rs:157`) já ignora
  silenciosamente qualquer `p_type != PT_LOAD` (`if p_type != PT_LOAD {
  continue; }`) — adicionar extração de `PT_TLS` é puramente aditivo, sem
  risco de mudar o comportamento de nenhum executável já existente.
  `PHDR_SIZE = 56` (`elf.rs:57`) já cobre o layout completo de um
  `Elf64_Phdr`, incluindo `p_align` (bytes 48-56), hoje nunca lido.
- `kernel::process::Context` (`process.rs:67-116`) guarda todo estado
  por-thread que `schedule_next` precisa trocar a cada troca de contexto
  (`rsp`, `kernel_stack_top`, `address_space_root`, `fpu_state`,
  `syscall_rsp_slot`) — mesmo padrão a seguir para o novo `FS.base` por
  thread.
- `kernel::process::create_user_thread` (`process.rs:257-282`, função
  privada) é o **único** ponto que cria uma `Thread` de modo usuário — tanto
  `Process::finish_new_user` (primeira thread de um processo recém-criado
  por `spawn_from_elf`) quanto `Process::spawn_user_thread` (thread
  adicional, `kernel-user-thread-create`) passam por ele. É o ponto de
  inserção natural para a criação do bloco de TLS: uma única mudança cobre
  os dois casos.
- `kernel::scheduler::schedule_next` já troca `CR3`
  (`crate::paging::switch_address_space`) e `TSS.rsp0`
  (`set_current_kernel_stack`) no mesmo trecho, a cada troca de contexto —
  ver `scheduler.rs` em torno de `let contexts = CONTEXTS.lock();`. Mesmo
  ponto para a troca de `FS.base`.
- `crate::apic::{rdmsr, wrmsr}` (`apic.rs:40,49`) já são `pub(crate)` e
  usados por `kernel::syscall` para `IA32_STAR`/`IA32_LSTAR`/`IA32_FMASK` —
  reaproveitados aqui para `IA32_FS_BASE` (MSR `0xC000_0100`).
- `userland_c/linker.ld` não declara nenhuma seção `.tdata`/`.tbss` — hoje
  irrelevante (nenhum programa de teste usa `__thread`), mas sem essas
  seções explícitas o comportamento de orphan-section do `ld` para colocar
  `.tdata`/`.tbss` (e, portanto, emitir um `PT_TLS`) não é controlado por
  este projeto, ao contrário de todo o resto do layout (ver comentário já
  existente no arquivo sobre alinhamento de página).
- `Process` (`process.rs:226-230`) tem só `id`/`address_space`/`threads` —
  nenhum estado que sobreviva entre a criação do processo e a criação de
  uma thread adicional nele. Precisa de um novo campo para o "molde" de TLS
  (tamanho, alinhamento, imagem inicial), já que `spawn_user_thread` (uma
  thread adicional) não tem acesso ao buffer do executável original — só
  `finish_new_user` (chamada de dentro de `spawn_from_elf`) tem.

## Goals / Non-Goals

**Goals:**
- Cada thread de modo usuário — a primeira de um processo e qualquer
  adicional criada nele — ganha seu próprio bloco de TLS, inicializado a
  partir da imagem de TLS do executável quando ele declarar uma.
- `kernel::elf` reconhece o segmento `PT_TLS`, sem mudar nenhum
  comportamento de executáveis que não o declaram.
- Verificação com um programa de teste real que declara `__thread int` e
  prova independência entre duas threads.

**Non-Goals:**
- `pthread_create`/`os::create_thread`/`os::pd_start_thread` reais, ou
  qualquer arquivo vendorizado do HotSpot (`os_javaos.cpp`) — change
  seguinte, que reaproveita esta TLS real.
- `errno`/reentrância por-thread do newlib — fica possível depois desta
  change (via `__getreent()`/`_impure_ptr` reais), mas não é implementado
  aqui.
- Modelo dinâmico de TLS (`__tls_get_addr`, módulos carregados
  dinamicamente via `dlopen` com sua própria TLS) — só o modelo "exec
  local" (`-static -no-pie`, já as flags de link de todo executável deste
  projeto), que é o único que `-static -no-pie` pode produzir.
- Qualquer limite de tamanho de TLS além do suficiente para o uso real
  conhecido hoje (um único ponteiro do HotSpot, um `int` do programa de
  teste) — ver "Decisões" sobre o tamanho máximo de bloco escolhido.

## Decisions

### Ponto único de criação: dentro de `create_user_thread`
Em vez de duplicar a lógica de alocação de bloco de TLS em
`finish_new_user` e em `spawn_user_thread`, `create_user_thread` passa a
receber um `Option<&TlsTemplate>` (novo tipo, só `memsz`/`align`/`image:
&[u8]`) e, quando presente, aloca e inicializa o bloco antes de montar o
`Context` da nova thread. `finish_new_user` constrói o `TlsTemplate` a
partir do que `kernel::elf` acabou de extrair do executável;
`spawn_user_thread` lê o `TlsTemplate` já salvo no `Process` (novo campo
`tls_template: Option<TlsTemplate>`, preenchido uma única vez por
`finish_new_user`, nunca mutado depois).

### Modelo ELF TLS "Variant II" com TCB em offset fixo dentro do slot
Cada slot da nova janela mapeia uma única página (4 KiB). O bloco de
controle de thread (TCB — uma palavra de 8 bytes contendo um ponteiro para
si mesma, convenção do modelo Variant II) fica num offset fixo dentro do
slot, `TLS_TCB_OFFSET = 2048` (meio da página) — múltiplo de qualquer
alinhamento prático (`8`/`16`/`32`/`64`), então `tcb_addr - round_up(memsz,
align)` (o início da imagem de `.tdata`/`.tbss`) sempre cai alinhado
corretamente, sem lógica de arredondamento de endereço de página adicional.
`FS.base` dessa thread = `tcb_addr`. A imagem (`image` bytes do segmento
`PT_TLS`) é copiada terminando em `tcb_addr - round_up(memsz, align) +
filesz`, e o restante até `tcb_addr` é zerado — mesma convenção de BSS já
usada para segmentos `PT_LOAD`.

**Limite de tamanho**: `memsz` maior que `TLS_TCB_OFFSET` (2048 bytes) é
rejeitado (erro de carregamento, mesma categoria de "segmento malformado"
já coberta pela spec `kernel-elf-loader`) — folga generosa acima de
qualquer uso real conhecido hoje (um ponteiro do HotSpot, um `int` do
programa de teste), e evita qualquer lógica de slot de tamanho variável.

**Alternativa considerada e rejeitada**: calcular o offset do TCB
dinamicamente a partir de `round_up(memsz, align)` a cada thread criada.
Rejeitada por complexidade desnecessária — um offset fixo, generoso o
bastante, é mais simples e totalmente equivalente do ponto de vista do
código gerado (que só enxerga offsets negativos a partir de `FS.base`,
nunca o layout absoluto do slot).

### Nova janela de endereços dedicada a blocos de TLS
`TLS_BLOCK_POOL_BASE = 0x0000_1200_0000_0000` — primeiro incremento livre
da mesma faixa de `0x0000_0100_0000_0000` já usada por toda constante de
endereço de programa de teste/pool deste projeto (depois de
`SIBLING_THREAD_TEST_PROGRAM_STACK_VIRT = 0x0000_1100_...`, a mais recente
antes desta change). Um único slot por página (`TLS_BLOCK_POOL_STRIDE =
PAGE_SIZE`), contador global monotônico (mesmo padrão de
`USER_THREAD_STACK_POOL_BASE`/`NEXT_USER_THREAD_STACK_SLOT`) — seguro pelo
mesmo motivo: endereços virtuais de processos diferentes nunca colidem
(PML4s diferentes), e esta janela, como a de stacks de usuário adicionais,
vive na metade inferior do endereçamento (privada por processo), mapeada
via `Mapper::for_root`, não `Mapper::new`.

### Troca de `FS.base` em `schedule_next`, mesmo ponto que `CR3`/`TSS.rsp0`
Novo campo `Context::tls_base: u64` (`0` = nenhum bloco de TLS — sentinela
seguro, já que nenhuma thread sem TLS declarada pelo seu executável nunca
desreferencia `%fs:`). `schedule_next` chama
`crate::apic::wrmsr(IA32_FS_BASE, contexts[next.0 as usize].tls_base)` no
mesmo trecho onde já troca `CR3`/`TSS.rsp0`/`CURRENT_FPU_AREA`/
`CURRENT_SYSCALL_RSP_SLOT`. Threads de modo kernel (`spawn`/
`spawn_sharing_current_address_space`) sempre têm `tls_base = 0` — nenhuma
delas executa código compilado com `__thread` do `os_javaos`/HotSpot.

### `userland_c/linker.ld` ganha seções `.tdata`/`.tbss` explícitas
Adicionadas logo após `.data`/`.bss`, com `ALIGN` explícito antes de cada
uma — mesmo racional já documentado no arquivo para `.bss` (evitar que o
`ld` empacote seções com propriedades diferentes na mesma página,
garantindo que o segmento `PT_TLS` resultante tenha exatamente o `p_align`
que o código gerado pelo `gcc` espera, em vez de depender do tratamento
padrão de "orphan section" do linker).

## Risks / Trade-offs

- **[Risco]** Erro de offset na construção do TCB resultaria em código do
  HotSpot/programa de teste lendo/escrevendo endereço errado — silencioso,
  sem page fault necessariamente (a página inteira do slot está mapeada).
  → **Mitigação**: verificado com um programa de teste dedicado que
  escreve e lê de volta um valor conhecido em `__thread int` a partir de
  duas threads distintas, antes de qualquer uso pelo HotSpot.
- **[Risco]** `TLS_TCB_OFFSET = 2048` fixo rejeita qualquer TLS futura
  maior que isso. → **Mitigação**: aceitável — bem acima do uso real
  conhecido hoje; revisitar só se/quando um caso real precisar de mais
  (documentar como Non-Goal explícito, não "vai dar errado
  silenciosamente").
- **[Trade-off]** Um slot inteiro (4 KiB) por thread para um bloco de TLS
  tipicamente de poucos bytes — desperdício de espaço de endereço virtual,
  nunca de memória física (só a página efetivamente mapeada custa frame
  físico, e é só uma por thread). Aceitável pela mesma razão já aceita
  para a janela de stack de usuário de threads adicionais (espaço de
  endereço virtual é abundante, frames físicos não são desperdiçados).
- **[Risco]** Esquecer de adicionar `.tdata`/`.tbss` a `linker.ld` faria o
  `ld` colocar essas seções via tratamento padrão de orphan section,
  possivelmente sem gerar `PT_TLS` algum, ou com alinhamento inesperado. →
  **Mitigação**: tarefa explícita, verificada inspecionando os program
  headers do executável de teste compilado (`readelf -l` ou
  `kernel::elf::load_program_headers` nos próprios testes de host) antes
  de testar em boot real.

## Migration Plan

Não aplicável — capacidade aditiva. Nenhum executável sem segmento
`PT_TLS` muda de comportamento.

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.

## Resultado real da implementação

- **Mapeamento continua não-testável em host**: `init_tls_block`
  (`kernel::process`) chama `Mapper::for_root(...).map_new(...)` e depois
  escreve através do ponteiro HHDM resultante — mesma categoria de
  operação que `Mapper::translate`/`validate_user_range` já documentavam
  como "só verificável em boot real". Nenhum teste de host chamava
  `map_new` com escrita real antes desta change, e isso não muda agora:
  as tarefas 2.2/3.1/3.2 são verificadas em boot real (tarefa 4.3), não
  por teste de host.
- **`readelf -l` confirmou o `PT_TLS` real** gerado por
  `userland_c/tls_test_program.c` depois da tarefa 4.1
  (`.tdata`/`.tbss` explícitas em `linker.ld`): `TLS ... FileSiz=0x0
  MemSiz=0x4 Align=0x4` — `g_counter` (`__thread int`, sem inicializador)
  foi para `.tbss` como esperado, com o alinhamento natural de `int` (4),
  não 8 como a suposição inicial deste documento cogitava; a lógica de
  `tls_block_addresses` já tratava qualquer `align` até `2048` igualmente
  bem, então nenhuma mudança foi necessária.
- **Verificação em boot real (QEMU/OVMF) confirmou o mecanismo completo**:
  `/bin/tls.elf` reportou `tls_test_program: resultado PPP (ordem:
  thread_create,irma_escreveu_sua_copia,copia_principal_intacta)` — a
  thread-irmã escreveu e leu de volta corretamente sua própria cópia de
  `g_counter` (`222`), e a cópia da thread principal continuou `111`,
  nunca afetada. Todos os demais programas de teste já existentes
  (`file_syscalls_demo`, `libc_test_program`, `cxx_test_program`,
  `javaos_vm_test_program`, `javaos_pthread_test_program`,
  `libm_test_program`, `timedwait_test_program`, `efault_test_program`,
  `sibling_thread_test_program`, `user_posix_demo`,
  `fpu_demo_a`/`fpu_demo_b`, `safepoint`, `pthread`) reportaram
  exatamente as mesmas strings P/F de antes desta change, e o único `#PF`
  real do boot continua sendo o deliberado de `fault_test_program.c` —
  confirmando que threads sem TLS (`tls_base == 0`) não foram afetadas.
