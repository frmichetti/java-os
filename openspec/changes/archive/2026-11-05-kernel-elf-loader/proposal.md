# Proposal

## Why

O kernel hoje só consegue colocar código em modo usuário copiando manualmente os
bytes de uma rotina embutida no próprio binário do kernel para uma página de
usuário (`kernel::user_demo`, `kernel::user_posix_demo`, via
`Process::begin_new_user`/`finish_new_user`) — não existe nenhum caminho para
executar um programa real, lido do disco. A spec `kernel-processes` já declara
isso explicitamente fora do seu escopo ("sem exigir carregamento de um
executável externo nesta capability"). A decisão de `jvm-dynamic-linking-strategy`
identificou um **carregador de executável ELF estático** como pré-requisito
concreto e ainda inexistente para as duas sub-changes seguintes do roadmap
(linker ELF dinâmico mínimo e porte do HotSpot) — sem ele não há como colocar
nenhum binário real (nem o próprio linker dinâmico, nem futuramente a JVM) para
rodar sobre o kernel.

## What Changes

- Novo módulo `kernel::elf`: parsing de um binário ELF64 estático (ET_EXEC),
  validando o cabeçalho ELF (`EI_MAG`, `EI_CLASS=ELFCLASS64`,
  `EI_DATA=ELFDATA2LSB`, `e_type=ET_EXEC`, `e_machine=EM_X86_64`) e iterando
  os *program headers* do tipo `PT_LOAD`.
- Nova função de alto nível (ex.: `kernel::elf::load_and_spawn`) que: lê o
  arquivo do executável via `kernel::fat32::Fat32Filesystem::read_file`,
  valida o ELF, cria um novo processo (`Process::begin_new_user`), mapeia cada
  segmento `PT_LOAD` em páginas próprias (flags de leitura/escrita/execução
  derivadas de `p_flags`, alocação/zeragem de BSS quando `p_memsz > p_filesz`),
  copia os bytes do segmento via o alias HHDM do frame físico (mesmo padrão já
  usado por `kernel::scheduler::spawn_user_demo` para não depender da PML4 do
  processo ainda não ativa), e finaliza o processo
  (`Process::finish_new_user`) com o ponto de entrada (`e_entry`) do ELF e uma
  stack de usuário recém-mapeada.
- Tratamento de erro explícito para: arquivo não encontrado/ilegível,
  cabeçalho ELF inválido ou de arquitetura/tipo não suportados, segmento
  `PT_LOAD` sobreposto a uma região já mapeada, e falha de alocação de
  memória — sem travar o kernel.
- Programa de teste real (um pequeno binário ELF64 estático de modo usuário,
  montado fora da árvore do kernel ou com um linker script mínimo) usado para
  verificar o carregador ponta a ponta em boot real, análogo ao papel que
  `kernel::user_demo` cumpriu para a verificação de `syscall`/`sysret`.

Fora de escopo nesta change (fica para as duas próximas sub-changes do
roadmap, já antecipadas em `jvm-dynamic-linking-strategy`): resolução de
símbolos, carregamento de bibliotecas dinâmicas (`.so`), `dlopen`/`dlsym`, e
qualquer forma de relocation (só ET_EXEC não-PIE é suportado).

## Capabilities

### New Capabilities
- `kernel-elf-loader`: carregamento de um executável ELF64 estático a partir
  do sistema de arquivos para um novo processo em modo usuário, cobrindo
  parsing do cabeçalho/segmentos, mapeamento de memória e transferência de
  controle para o ponto de entrada do binário.

### Modified Capabilities

Nenhuma — `kernel-processes`, `kernel-user-mode`, `kernel-virtual-memory` e
`kernel-filesystem` já expõem todas as primitivas necessárias
(`Process::begin_new_user`/`finish_new_user`, `Mapper::map_new`,
`Fat32Filesystem::read_file`) sem precisar de nenhum requisito novo; esta
change só compõe essas primitivas num carregador, sem alterar o
comportamento que essas specs já descrevem.

## Impact

- **Novo código:** `kernel/src/elf.rs` (parsing + carregamento), um novo
  programa de teste ELF estático para verificação em boot real, e o ponto de
  disparo correspondente (análogo a `scheduler::spawn_user_demo`).
- **Dependências:** `kernel::fat32` (leitura do binário), `kernel::paging`
  (mapeamento dos segmentos), `kernel::process` (criação do processo/thread
  de usuário), `kernel::hhdm` (cópia via alias físico-direto).
- **Sem alteração** em código ou comportamento das capabilities existentes
  listadas acima.
