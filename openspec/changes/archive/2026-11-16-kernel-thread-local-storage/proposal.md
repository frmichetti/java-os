# Proposal

## Why

O próprio HotSpot (`share/runtime/thread.hpp`) declara `Thread::current()` como
`static THREAD_LOCAL Thread* _thr_current`, e `THREAD_LOCAL` expande para
`__thread` (`globalDefinitions_gcc.hpp`) — armazenamento local de thread real
do compilador, endereçado via `%fs:offset` no código gerado. Toda thread
nova do HotSpot chama `thread->initialize_thread_current()` como a
primeiríssima coisa que faz (`thread_native_entry`, `os_bsd.cpp`). Hoje este
kernel não tem nenhuma infraestrutura de TLS — nenhum `FS.base` por thread,
nenhuma leitura do segmento `PT_TLS` de um executável. Sem isso, nenhuma
segunda thread do HotSpot consegue sequer começar a existir, tornando este o
bloqueio direto para a change seguinte planejada (`pthread_create`/
`os::create_thread` real sobre `kernel-user-thread-create`, ver `ROADMAP.md`).

## What Changes

- Nova faixa de endereço virtual dedicada a blocos de TLS de thread,
  alocada por thread (uma por thread de modo usuário, mesmo padrão de
  janela global monotônica já usado em `kernel-user-thread-create` para
  stacks de usuário de threads adicionais).
- `kernel::elf::load_program_headers` passa a reconhecer o segmento `PT_TLS`
  (hoje silenciosamente ignorado, junto com qualquer outro `p_type !=
  PT_LOAD`), extraindo tamanho em memória, alinhamento e a imagem inicial
  (bytes do arquivo + preenchimento zerado até o tamanho em memória,
  mesmo padrão já usado para a área de BSS de um segmento `PT_LOAD`).
- Cada thread de modo usuário — a primeira thread de um processo recém
  carregado de um executável ELF (`spawn_from_elf`), e qualquer thread
  adicional criada dentro de um processo já existente
  (`SYSCALL_THREAD_CREATE`) — ganha seu próprio bloco de TLS, inicializado a
  partir da imagem de TLS do executável desse processo quando ele declarar
  um segmento `PT_TLS` (processo sem esse segmento não aloca bloco algum,
  comportamento idêntico ao de hoje).
- Modelo adotado: ELF TLS "Variant II" (x86-64 System V ABI) — um bloco de
  controle de thread (TCB) com um ponteiro para si mesmo no endereço de
  `FS.base`, com a imagem de `.tdata`/`.tbss` do executável logo abaixo dele
  (offsets negativos), consistente com o modelo "exec local" que
  `-static -no-pie` (flags de link já usadas por todo executável deste
  projeto) sempre produz.
- `kernel::scheduler::schedule_next` passa a trocar `FS.base` (via
  `wrmsr IA32_FS_BASE`) a cada troca de contexto, mesmo ponto onde `CR3`/
  `TSS.rsp0` já são trocados hoje.
- Novo programa de teste em `userland_c/` que declara uma variável
  `__thread int`, cria uma thread-irmã via `SYSCALL_THREAD_CREATE`, e prova
  que cada thread enxerga um valor independente dessa variável.

**Fora do escopo desta change** (fica para a change seguinte, que reaproveita
esta TLS real): `pthread_create`/`os::create_thread`/`os::pd_start_thread`
reais, qualquer arquivo vendorizado do HotSpot (`os_javaos.cpp` continua
`fatal()` em `pd_start_thread`), e `errno`/reentrância por-thread do newlib
(passam a ser possíveis depois desta change, via `__getreent()`/
`_impure_ptr` reais do próprio newlib, não implementados aqui).

## Capabilities

### New Capabilities

Nenhuma — toda mudança se encaixa em capabilities já existentes.

### Modified Capabilities

- `kernel-processes`: adiciona o requisito de que toda thread de modo
  usuário tenha seu próprio bloco de TLS, independente do de qualquer
  outra thread.
- `kernel-elf-loader`: adiciona o requisito de reconhecimento do segmento
  `PT_TLS` de um executável, extraindo os dados necessários para
  inicializar o bloco de TLS de suas threads.

## Impact

- `kernel/src/elf.rs`: reconhecimento do segmento `PT_TLS` (novo tipo de
  program header extraído, junto do já existente `PT_LOAD`).
- `kernel/src/process.rs`: novo campo em `Context` para o endereço do
  bloco de TLS da thread (`FS.base`); nova lógica de alocação/inicialização
  de bloco de TLS, chamada tanto por `Process::new`/`finish_new_user`
  quanto por `Process::spawn_user_thread`.
- `kernel/src/stack.rs` (ou novo módulo): nova janela de endereços
  dedicada a blocos de TLS, mesmo padrão de janela global monotônica já
  usado para a janela de stack de usuário de threads adicionais.
- `kernel/src/scheduler.rs`: troca de `FS.base` em `schedule_next`.
- `userland_c/`: novo programa de teste de TLS com thread-irmã.
- Nenhuma mudança de comportamento observável para um executável sem
  segmento `PT_TLS` (todo programa de teste já existente hoje).
