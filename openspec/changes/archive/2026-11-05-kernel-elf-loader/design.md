# Design

## Context

Ver `proposal.md` — "Why" para a motivação (pré-requisito identificado por
`jvm-dynamic-linking-strategy`).

Primitivas já existentes e reaproveitadas sem alteração:
- `kernel::fat32::Fat32Filesystem::read_file` (`kernel/src/fat32.rs:246`) lê o
  arquivo inteiro para um `Vec<u8>` em memória do kernel — não há streaming.
- `kernel::process::Process::begin_new_user`/`finish_new_user`
  (`kernel/src/process.rs:279`, `:288`) já separam a criação do espaço de
  endereçamento (fase 1) do mapeamento de código + criação da thread inicial
  (fase 2) — exatamente a divisão de que um carregador precisa, já usada por
  `kernel::scheduler::spawn_user_demo` (`kernel/src/scheduler.rs:218`) para
  copiar manualmente um programa de teste.
- `kernel::paging::Mapper::for_root` + `map_new` (`kernel/src/paging.rs:174`,
  `:241`) mapeiam uma página nova em qualquer `AddressSpace` por `root`
  (endereço físico da PML4), mesmo antes dela estar ativa em `CR3` —
  **mas não zeram o frame que alocam** (ver "Decisões — Zeragem explícita do
  BSS" abaixo; achado ao revisar esta função para esta change).
- `kernel::hhdm::phys_to_virt` dá, a partir do frame físico recém-alocado, um
  endereço virtual já válido com a PML4 *atual* (a do processo chamador, não a
  do novo processo) para onde copiar bytes — mesmo padrão de
  `spawn_user_demo`, necessário porque a PML4 do novo processo só é ativada
  quando o escalonador o escolhe pela primeira vez.

`kernel-processes` declara explicitamente que não cobre carregamento de
executável externo; esta change preenche exatamente essa lacuna, compondo as
primitivas acima — não precisa de nenhuma primitiva nova em
`kernel::paging`, `kernel::process` ou `kernel::fat32`.

## Goals / Non-Goals

**Goals:**
- Interpretar um executável ELF64 estático (`ET_EXEC`, x86-64) e colocá-lo
  rodando em modo usuário, com segmentos mapeados com as permissões corretas.
- Reportar todo erro de carregamento (I/O, formato, sobreposição, memória)
  como `Result`, nunca como pânico do kernel.
- Verificação ponta a ponta em boot real (QEMU), análoga à de
  `kernel::user_demo` para `kernel-syscalls`.

**Non-Goals (explícitos em proposal.md, reforçados aqui):**
- Nenhuma relocation: só `ET_EXEC` não-PIE. `ET_DYN` (PIE) e qualquer
  resolução de símbolo ficam para a sub-change do linker dinâmico.
- Nenhum carregamento de biblioteca compartilhada (`.so`), `dlopen`/`dlsym`,
  nem `PT_INTERP`/interpretador ELF (`PT_INTERP` presente é erro de "recurso
  não suportado", não um caminho de dynamic linking parcial).
- Nenhuma otimização de I/O (ex.: mapear sob demanda em vez de ler o arquivo
  inteiro primeiro): aceitável para os binários de teste desta change;
  revisitar se/quando um executável real (ex.: o próprio linker dinâmico, ou
  HotSpot) for grande o bastante para isso importar.
- Nenhum argumento de linha de comando (`argv`/`envp`) passado ao processo —
  o ponto de entrada começa sem nenhum estado inicial além da stack vazia.

## Decisions

### Parsing manual dos cabeçalhos ELF, sem dependência externa
O kernel já é `no_std` com todas as suas abstrações de baixo nível escritas à
mão (ver `kernel::gpt`, `kernel::fat32`). O cabeçalho ELF64 e a tabela de
*program headers* são, como BPB/GPT, layouts binários fixos e pequenos — não
há necessidade de trazer uma dependência externa (ex. crate `elf` / `goblin`)
só para ler um punhado de campos (`e_type`, `e_machine`, `e_entry`,
`e_phoff`, `e_phnum`, e por entrada `p_type`, `p_flags`, `p_offset`,
`p_vaddr`, `p_filesz`, `p_memsz`). Mesmo padrão de separação já usado em
`kernel::fat32`: uma camada pura de parsing (structs + `parse(&[u8]) ->
Result<_, ElfError>`), sem nenhum acesso a disco ou memória do processo
destino, testável com `cargo test -p kernel --lib` sem hardware.

**Alternativa considerada:** usar uma crate `no_std` existente (`elf` crate,
por exemplo). Rejeitada por ora pelo mesmo motivo que motivou escrever
`kernel::gpt`/`kernel::fat32` à mão: o formato necessário aqui é um
subconjunto pequeno e estável (sem relocations, sem seções, só o cabeçalho e
`PT_LOAD`), e a dependência externa traria superfície não usada (parsing de
seções, símbolos, relocations) sem necessidade nesta change.

### Cópia dos segmentos via alias HHDM, não via `AddressSpace` ativa
Mesma técnica já usada por `spawn_user_demo`: como a PML4 do novo processo
ainda não está em `CR3` no momento do carregamento, os bytes de cada
segmento são copiados para o frame físico recém-alocado através do seu alias
HHDM (sempre válido, independente de qual PML4 está ativa), não através do
endereço virtual de usuário final do segmento. A função de carregamento
mapeia cada página do segmento com `Mapper::for_root` (que já opera por
`root` explícito, sem exigir `CR3` trocado) e escreve nela via
`hhdm::phys_to_virt(frame)`.

### `load_and_spawn` vive em `kernel::scheduler`, não em `kernel::elf`
`spawn_user_demo` registra o processo/thread criados chamando `register_thread`
(função privada do módulo `scheduler`) e empurrando em `scheduler::PROCESSES`
(`pub(crate)`, mas só `scheduler` chama `register_thread` hoje). Em vez de
alargar a visibilidade desses dois pontos só para que um módulo externo os
alcance, a função de alto nível que lê o executável e já devolve um
`ThreadId` pronto para rodar (equivalente a `spawn_user_demo`, mas a partir de
um arquivo real) vive em `kernel::scheduler` — `kernel::elf` fica restrito
a parsing do ELF (camada pura) e ao cálculo/mapeamento dos segmentos
(camada que só depende de `FrameAllocator`/`Mapper`, sem tocar o
escalonador), exatamente a mesma divisão que `kernel::fat32`/`kernel::gpt` já
seguem (camada pura + camada de integração, cada uma num módulo com o
conjunto de dependências que lhe cabe).

### Granularidade de mapeamento por página, permissões por segmento
`Mapper::map_new` mapeia uma página de cada vez; um segmento `PT_LOAD` cobre
um intervalo `[p_vaddr, p_vaddr + p_memsz)` possivelmente maior que uma
página. O carregador itera esse intervalo em passos de `PAGE_SIZE` (truncando
`p_vaddr` para baixo e `p_vaddr + p_memsz` para cima no limite de página, como
exigido por `Mapper::map_new`/`MapError::Misaligned`), aplicando a mesma
`PageFlags` (derivada de `p_flags`: `PF_R`→sempre presente implicitamente,
`PF_W`→`writable`, `PF_X`→`executable`) a todas as páginas desse segmento.
Overlap entre segmentos (dois `PT_LOAD` mapeando a mesma página com flags
potencialmente diferentes) é tratado como erro (`MapError::AlreadyMapped`
propagado) em vez de merge de permissões — não há caso de uso real para
executáveis estáticos simples gerados por um linker bem-comportado, e
silenciosamente escolher uma das duas permissões esconderia um executável
malformado.

### Zeragem explícita do BSS
**Achado ao revisar `Mapper::map_new` para esta change:** ao contrário do que
uma leitura apressada do resto do kernel sugere, `map_new` **não** zera o
frame que aloca — só devolve o frame cru de `FrameAllocator::alloc` e o
mapeia. Quem precisa de conteúdo zerado sempre zera explicitamente por conta
própria: `AddressSpace::new` (`core::ptr::write_bytes` na PML4 nova) e
`Mapper::next_table_or_create` (idem, para cada tabela intermediária nova).
Nenhum chamador existente de `map_new` (`kernel::heap`, `kernel::stack`,
`spawn_user_demo`) depende de conteúdo zerado — cada um ou não liga para o
conteúdo inicial (heap, stack) ou sobrescreve a página inteira antes de
qualquer leitura (`copy_into` do programa de teste). Esta change é a primeira
a precisar de conteúdo zerado vindo de uma página de `map_new` (o BSS), então
segue a mesma convenção do resto do kernel: zera explicitamente, via o alias
HHDM, cada página inteira do segmento assim que mapeada, **antes** de copiar
os bytes do arquivo por cima — nunca assume que `map_new` já devolve zero.
Verificado em boot real (tarefa 4.2), já que o mapeamento real só é
exercitável lá (ver tasks.md, grupo 2, nota de verificação): um segmento de
teste com `p_memsz > p_filesz` cujo conteúdo lido de volta após `p_filesz` é
zero.

### Programa de teste: binário ELF real, fora da árvore do kernel
Ao contrário de `kernel::user_demo` (assembly embutido via
`global_asm!`, copiado em memória), o programa de teste desta change precisa
ser um arquivo ELF64 estático real, gravado no sistema de arquivos da imagem
de disco (`boot/dist/java-os.img`), para exercitar o caminho completo
(leitura via `kernel::fat32` → parsing → mapeamento). Monta-se com um
segundo binário Rust `#![no_std]`/`#![no_main]`, com seu próprio
`x86_64-unknown-none`-like target e linker script fixando um endereço de
carga estático (sem PIE), reaproveitando o pipeline de montagem de imagem de
`cargo xtask build-image` (adicionar o binário compilado à partição FAT32).
Mesmo papel que `user_demo` cumpriu para `kernel-syscalls`, mas como artefato
real em disco em vez de código embutido.

## Risks / Trade-offs

- **[Risco]** Um ELF malformado com `e_phnum`/`e_phoff` apontando fora dos
  limites do buffer lido causaria leitura fora dos limites do `Vec<u8>` em
  Rust seguro → **[Mitigação]** todo acesso ao buffer do executável usa
  indexação com verificação de limites (`.get(..)` + `Option`/`Result`, nunca
  indexação direta que entra em pânico) em vez de assumir que os offsets
  declarados são válidos; cada acesso fora dos limites declarados pelo
  próprio tamanho do arquivo é um erro de formato, nunca um pânico.
- **[Risco]** Ler o arquivo inteiro em um único `Vec<u8>`
  (`Fat32Filesystem::read_file`) antes de mapear qualquer coisa consome
  memória do heap do kernel proporcional ao tamanho do executável →
  **[Mitigação]** aceitável para os binários de teste desta change (poucos
  KiB); revisitar (leitura sob demanda por segmento) só quando um executável
  real e grande (linker dinâmico, HotSpot) tornar isso um problema prático —
  não otimizar antecipadamente.
- **[Risco]** Criar o processo com `begin_new_user` antes de todos os
  segmentos terem sido mapeados com sucesso deixa um `AddressSpace`
  parcialmente populado em caso de erro no meio do loop de segmentos. O
  `AddressSpace` parcial nunca é registrado em `scheduler::PROCESSES` nem tem
  thread criada (só acontece via `finish_new_user`, chamado por último), então
  nenhum processo "fantasma" fica visível ao escalonador ou executável — mas
  `AddressSpace` não implementa `Drop` (nenhum código do kernel hoje libera os
  frames de um espaço de endereçamento; não existe ainda mecanismo algum de
  encerrar um processo) → **[Mitigação]** os frames já mapeados antes do erro
  ficam vazados (nunca devolvidos ao `FrameAllocator`) — aceitável nesta
  change porque é exatamente a mesma lacuna pré-existente em todo o resto do
  kernel (nenhum processo pode ser encerrado ainda, por nenhum caminho), não
  uma regressão introduzida aqui. Liberar frames de um `AddressSpace`
  descartado é um problema de encerramento de processo em geral, fora do
  escopo desta change — revisitar quando essa capability existir.

## Migration Plan

Change puramente aditiva (novo módulo `kernel::elf`, novo binário de teste),
sem migração de dados nem de API existente. Nenhum rollback especial além de
reverter o commit.

## Achados durante a implementação

1. **`Mapper::map_new` não zera o frame que aloca** (achado ao revisar essa
   função para o mapeamento de segmentos): já coberto em detalhe em
   "Decisões — Zeragem explícita do BSS". `kernel::elf::load_segment` zera
   cada página nova explicitamente via HHDM antes de copiar qualquer coisa.
2. **`register_thread`/`PROCESSES` são privados/`pub(crate)` de
   `kernel::scheduler`**: a função de alto nível (`spawn_from_elf`) teve que
   viver em `kernel::scheduler`, não em `kernel::elf` como o nome original
   do design previa — já coberto em "Decisões — `load_and_spawn` vive em
   `kernel::scheduler`".
3. **Mapeamento real não é testável em host** (`HHDM_OFFSET` não
   inicializado em `cargo test -p kernel --lib`, mesma limitação já
   documentada pelos próprios testes de `kernel::paging`): só a matemática
   pura de página (`segment_page_range`/`segment_page_flags`/
   `page_ranges_overlap`) tem teste de host; o mapeamento real
   (`load_segment`) só foi verificável em boot real — ver item 6 abaixo.
4. **`cargo` não rastreia `linker.ld` como dependência de build**: editar o
   linker script do `elf_test_program` sem invalidar o cache de build (ele
   só é referenciado via uma string de caminho em `rustflags`, não como um
   arquivo de entrada do crate) faz `cargo build` reusar o binário antigo
   silenciosamente — sem erro, sem aviso. É preciso `rm -rf
   target/x86_64-java_os_user` (ou qualquer mudança que force a reexecução
   do link) depois de editar `linker.ld` para a mudança ter efeito.
   Descoberto ao depurar por que uma correção no linker script (achado 5,
   abaixo) não parecia surtir efeito nenhum em `readelf -l`.
5. **Um linker sem separação explícita de página entre seções de permissão
   diferente produz segmentos `PT_LOAD` que colidem na mesma página física**:
   a primeira versão de `elf_test_program` (`.text` seguido imediatamente de
   `.bss`, sem alinhamento) gerava dois `PT_LOAD` (`R E` e `RW`) cujo
   intervalo de página, depois de `segment_page_range` arredondar para
   `PAGE_SIZE`, era idêntico (`[0x400000, 0x401000)` para ambos) — rejeitado
   corretamente por `check_no_overlap` como `ElfError::OverlappingSegments`.
   Corrigido adicionando `. = ALIGN(0x1000);` entre `.text` e `.rodata`/
   `.data`/`.bss` em `linker.ld`, forçando cada segmento de permissão
   diferente a começar em sua própria página — mesma prática que linkers de
   uso geral (`ld`/`lld` com `-z separate-code`) aplicam por padrão e que
   este linker script minimalista não aplicava. Confirma que a rejeição de
   sobreposição da spec (`kernel-elf-loader`, "Segmentos carregáveis se
   sobrepõem") é a escolha certa: silenciosamente mesclar as permissões das
   duas páginas teria mascarado este problema de linkagem em vez de expô-lo.
6. **Verificação em boot real (tarefa 4.2), confirmada via
   `cargo xtask run -- -serial stdio -display none`**: com os dois achados
   acima corrigidos, `/bin/test.elf` (2 segmentos `PT_LOAD`: `.text` em
   `R E` e `.bss` em `RW`, em páginas separadas) foi lido do disco via
   `kernel::fat32`, validado, mapeado e executado com sucesso — saída serial
   observada, em ordem:
   ```
   carregador ELF: /bin/test.elf carregado e em execucao
   BSS: zerado corretamente pelo carregador - PASSOU
   ola do executavel ELF real, carregado do disco!
   ```
   seguida da mesma linha repetindo em laço (prova de que a *syscall* de
   escrita e a de consulta de ticks continuam funcionando a partir do
   processo carregado). A linha "BSS: zerado..." confirma especificamente o
   cenário "Segmento de dados com área de BSS" da spec em boot real, não
   apenas pela matemática pura de `segment_page_range` testada em host (ver
   achado 3). Nenhum pânico, nenhuma trava, banner de boot (`boot OK`)
   presente — `cargo xtask test` (tarefa 4.3) confirmou que o smoke test
   automatizado de boot continua passando com o novo ponto de disparo.
