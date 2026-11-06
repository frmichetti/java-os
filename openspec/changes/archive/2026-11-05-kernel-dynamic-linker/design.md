# Design

## Context

Ver `proposal.md` — "Why" para a motivação (pré-requisito final do item 8 do
roadmap antes do porte do HotSpot).

Primitivas já existentes e reaproveitadas:
- `kernel::elf::ElfHeader::parse`/`load_program_headers`
  (`kernel/src/elf.rs`): parsing puro de cabeçalho ELF64 e program headers
  `PT_LOAD`, hoje restrito a `ET_EXEC`.
- `kernel::elf::load_segments` (`kernel/src/elf.rs:273`): mapeia uma lista de
  `ProgramHeader` (endereços virtuais já absolutos) numa `AddressSpace` via
  `root`, checando sobreposição, zerando BSS explicitamente — genérico o
  bastante para reaproveitar sem nenhuma mudança, desde que os segmentos
  passados já tenham seus `vaddr` somados à base de carga escolhida (ver
  Decisões).
- `kernel::fat32::Fat32Filesystem::read_file`/`mount_boot_partition` e
  `kernel::storage::with_disk`: leitura de arquivo, mesmo padrão já usado por
  `kernel::scheduler::spawn_from_elf`.
- `kernel::paging::current_root()` (`kernel/src/paging.rs:406`): devolve o
  CR3 físico atualmente ativo — crucial para esta change (ver Decisões,
  "Estado por espaço de endereçamento via `current_root()`").
- Padrão de tabela dinâmica de `kernel::pthread_handles`: `Vec` que só
  cresce, `Box::leak`/índice como `handle` opaco — reaproveitado para o
  `handle` de biblioteca carregada (índice no `Vec` de bibliotecas do espaço
  de endereçamento chamador) e para o padrão geral de "tabela dinâmica
  indexada por posição".
- Convenção de *syscall* já estabelecida (`kernel::syscall`): número em
  `RAX`, argumentos em `RDI`/`RSI`/`RDX` (`arg0`/`arg1`/`arg2` em
  `dispatch`), retorno em `RAX`, sentinela `SYSCALL_ERROR` para qualquer
  falha, leitura de ponteiro de usuário sem validação (`sys_write` já
  estabelece esse precedente — isolamento de falhas é Non-Goal conhecido de
  todo o kernel, não desta change).

`kernel-elf-loader` entrega o carregador de `ET_EXEC`, mas seu `design.md`
já antecipa esta change como a peça final do item 8 antes do porte do
HotSpot — ver seção 6 de `docs/architecture/overview.md`.

## Goals / Non-Goals

**Goals:**
- Carregar uma biblioteca compartilhada ELF64 (`ET_DYN`) num processo já em
  execução, resolvendo e aplicando suas relocações de forma eager (sem PLT
  lazy).
- Carregar recursivamente as dependências declaradas (`DT_NEEDED`) antes de
  relocar quem depende delas, com detecção explícita de dependência
  circular.
- Expor `dlopen`/`dlsym` como *syscalls*, com estado de bibliotecas
  isolado por espaço de endereçamento chamador.
- Verificação ponta a ponta em boot real com pelo menos duas bibliotecas
  reais (uma dependendo da outra via `DT_NEEDED`), exercitando ao menos uma
  relocação `R_X86_64_RELATIVE` (intra-biblioteca) e uma `R_X86_64_JUMP_SLOT`/
  `R_X86_64_GLOB_DAT` (símbolo resolvido entre bibliotecas).

**Non-Goals (explícitos em proposal.md, detalhados aqui):**
- *Lazy binding* via resolvedor de PLT: toda relocação é resolvida e escrita
  no momento do carregamento (`dlopen`), nunca na primeira chamada.
- *Symbol versioning* (`.gnu.version`/`DT_VERNEED`/`DT_VERDEF`): símbolos são
  resolvidos só por nome.
- TLS dinâmico (`DTV`, `__tls_get_addr`, `R_X86_64_TPOFF64`/`DTPMOD64`/etc.).
- `dlclose`/contagem de referências: uma vez carregada, uma biblioteca nunca
  é descarregada nem tem memória liberada nesta change (mesma lacuna já
  aceita para processos — ver `kernel-elf-loader/design.md`, "Riscos —
  processo parcial": nenhum caminho do kernel libera recursos de um
  processo/biblioteca ainda).
- Cache/deduplicação de `dlopen` por caminho: duas chamadas de `dlopen` para
  o mesmo caminho carregam duas instâncias independentes, cada uma com sua
  própria base e cópia das relocações. **Within uma única árvore de
  dependências de uma chamada de `dlopen`**, uma dependência (`DT_NEEDED`)
  repetida (diamante) também é recarregada de novo para cada referência —
  mesma simplificação aplicada uniformemente; só uma dependência circular
  verdadeira (A→B→A) é detectada e rejeitada (não pode ser contornada
  recarregando, causaria recursão infinita).
- `DT_HASH`/`DT_GNU_HASH` como estrutura de busca: usados só para obter a
  contagem de símbolos (`DT_HASH.nchain`, ver Decisões); a resolução de nome
  em si é sempre varredura linear. **Bibliotecas linkadas só com
  `--hash-style=gnu`** (sem `DT_HASH` clássico) são rejeitadas
  explicitamente — ver Decisões, "Contagem de símbolos via `DT_HASH`".
- Resolução de símbolo do executável principal (`ET_EXEC`) para bibliotecas
  carregadas via `dlopen`: só biblioteca resolve contra biblioteca já
  carregada no mesmo espaço de endereçamento.
- `RPATH`/`RUNPATH`/variável de busca configurável: dependências `DT_NEEDED`
  são sempre resolvidas em `/lib/<nome-declarado>`, caminho fixo não
  configurável.
- Argumentos/variáveis de ambiente, `PT_INTERP` (nenhum programa desta
  change é iniciado via interpretador ELF — o próprio kernel desempenha esse
  papel via as *syscalls* `dlopen`/`dlsym`, chamadas explicitamente pelo
  código do processo).

## Decisions

### Reaproveitar `kernel::elf` ampliando `ElfHeader` em vez de duplicar parsing
`ElfHeader::parse` passa a aceitar `e_type` igual a `ET_EXEC` **ou**
`ET_DYN` (ambos ELF64/x86-64/little-endian — só esses dois tipos fazem
sentido para o que o kernel carrega), expondo o campo `e_type` lido. A
distinção "isto deve ser especificamente `ET_EXEC`" (usada por
`kernel::scheduler::spawn_from_elf`) ou "isto deve ser especificamente
`ET_DYN`" (usada pelo novo `kernel::dylib`) passa a ser responsabilidade de
cada chamador, não do parser compartilhado. `load_program_headers`
(filtragem de `PT_LOAD`) é reaproveitada sem nenhuma mudança — já é
agnóstica a `e_type`.

**Por que não é uma mudança de contrato de `kernel-elf-loader`:**
`spawn_from_elf` continua checando `header.e_type == ET_EXEC`
explicitamente e devolvendo o mesmo `ElfError::UnsupportedExecutable` de
antes para qualquer outro tipo — o comportamento observável dessa capability
não muda; só a validação de tipo migrou de dentro do parser compartilhado
para o chamador. Ver proposal.md, "Modified Capabilities" (nenhuma).

**Localizar o segmento `PT_DYNAMIC`:** como `load_program_headers` descarta
qualquer entrada que não seja `PT_LOAD`, uma função nova e pequena em
`kernel::dylib` (`find_dynamic_segment`) repete o mesmo padrão de varredura
com limites verificados já usado em `load_program_headers`, procurando a
primeira entrada do tipo `PT_DYNAMIC` (valor 2). Alternativa considerada:
generalizar `kernel::elf::load_program_headers` para filtrar por `p_type`
arbitrário. Rejeitada por ora — a duplicação é pequena (~15 linhas) e evita
alargar a API pública de `kernel::elf` por conta de um único chamador.

### Segmentos de um `ET_DYN` são relativos à base 0; a base de carga é escolhida pelo kernel
Diferente de um `ET_EXEC` (endereços absolutos, decididos pelo linker que
gerou o executável), os `p_vaddr` de um `ET_DYN` são relativos a uma base
hipotética 0 — o kernel escolhe onde carregar. Antes de chamar
`kernel::elf::load_segments` (que espera endereços já absolutos), uma função
pura (`rebase_segments(segments, base) -> Vec<ProgramHeader>`) soma `base` a
cada `p_vaddr`, preservando `offset`/`filesz`/`memsz`/`flags` — reaproveita
`load_segments` (incluindo sua checagem de sobreposição e zeragem de BSS)
sem nenhuma mudança nele.

### Estado por espaço de endereçamento via `current_root()`, não via escalonador
**Achado durante o design** (evitou uma mudança maior originalmente
cogitada): não há hoje nenhum caminho de uma thread em execução até o
`Process` que a possui (`kernel::scheduler::register_thread` descarta
`Thread::process` ao só guardar `Context` em `CONTEXTS`) — adicionar esse
rastreamento exigiria uma nova tabela paralela no escalonador. Em vez disso,
como `dlopen`/`dlsym` sempre executam como *syscall* dentro do próprio
processo-alvo (nunca entre processos), o CR3 **já está** correto no momento
da chamada: `kernel::paging::current_root()` devolve exatamente o `root` do
chamador, sem tocar o escalonador. `kernel::dylib` mantém uma tabela global
própria, chaveada por esse `root` (`u64`):
```
static DYLIB_STATE: Spinlock<BTreeMap<u64, ProcessDylibState>>
struct ProcessDylibState { libraries: Vec<LoadedLibrary>, next_base: u64 }
```
criada sob demanda (`entry(root).or_insert_with(...)`) no primeiro `dlopen`
de cada espaço de endereçamento — mesmo padrão de "tabela que só cresce,
`handle` = índice" de `kernel::pthread_handles`, só que com uma camada a
mais de chave (`root`) para isolar processos entre si, cobrindo
"Isolamento de Bibliotecas Carregadas entre Processos Diferentes" da spec
sem precisar que `Process` saiba nada sobre bibliotecas dinâmicas.

### Relocações são escritas no endereço virtual final diretamente, não via HHDM
Diferença importante em relação a `kernel::elf::load_segment` (que grava o
*conteúdo inicial* de um segmento via o alias HHDM, porque roda **durante a
criação do processo**, antes de sua `AddressSpace` estar ativa em `CR3`):
aplicar uma relocação acontece **depois** dos segmentos já mapeados, como
parte de uma *syscall* `dlopen` que já está rodando com o `CR3` do processo
alvo ativo (é o próprio processo chamando). O endereço final
`base + r_offset` já é diretamente endereçável e gravável pelo próprio
código do kernel nesse exato ponto — `kernel::dylib::apply_relocation`
escreve com um `*mut u64`/`*mut i64` comum nesse endereço, sem nenhuma
tradução HHDM.

### Contagem de símbolos via `DT_HASH`, sem implementar a tabela de hash em si
`Elf64_Sym` não tem um "tamanho de tabela" declarado diretamente; a
especificação ELF garante que o cabeçalho de `DT_HASH` (`nbucket: u32`,
`nchain: u32`, seguido pelos buckets/chains) sempre tem `nchain` igual ao
número de entradas de `.dynsym`. `kernel::dylib` lê só esses dois `u32`
iniciais de `DT_HASH` para obter a contagem de símbolos, ignorando
completamente buckets/chains — a própria busca de símbolo por nome é uma
varredura linear sobre `.dynsym[0..nchain]`, nunca usa o hash de fato.
**Consequência explícita (Non-Goal):** uma biblioteca linkada só com
`--hash-style=gnu` (sem `DT_HASH` clássico) não declara `nchain` em lugar
nenhum que este kernel saiba interpretar — rejeitada com
`DylibError::MissingSysvHash` em vez de arriscar um comprimento de tabela
inventado. As bibliotecas de teste desta change são explicitamente linkadas
com `--hash-style=sysv` (ou `both`) via `rustflags` do crate de teste.

### `DT_NEEDED` resolvido em `/lib/<nome>`, carregado recursivamente, com detecção de ciclo
Cada entrada `DT_NEEDED` guarda só um nome (ex.: `libbase.so`), sem caminho.
`kernel::dylib::load_library` resolve isso para o caminho fixo
`/lib/<nome>` (Non-Goal: sem `RPATH`/busca configurável) e chama a si mesma
recursivamente **antes** de aplicar as relocações da biblioteca dependente —
garantindo que os símbolos da dependência já estejam no
`ProcessDylibState.libraries` quando a resolução de símbolo da dependente
rodar. Uma pilha transitória de caminhos "em carregamento" (parâmetro local,
nunca persistido em `ProcessDylibState`) detecta dependência circular
(`DylibError::CircularDependency`) comparando o caminho resolvido antes de
cada carregamento recursivo — sem essa checagem, um ciclo A→B→A causaria
recursão infinita e estouro de stack (travando o kernel, violando a spec
"Falha... Não Compromete a Estabilidade").

### Resolução de relocação: fórmula uniforme `symbol_value + addend`
`R_X86_64_RELATIVE` (tipo 8, sem símbolo): `valor = base + addend`.
`R_X86_64_GLOB_DAT`/`R_X86_64_JUMP_SLOT`/`R_X86_64_64` (tipos 6/7/1, com
símbolo): `valor = resolve_symbol(nome) + addend`. Produtores reais
(`rust-lld`) sempre emitem `addend = 0` para `GLOB_DAT`/`JUMP_SLOT`, então
somar o campo uniformemente é inofensivo para esses dois tipos e correto
para `R_X86_64_64` (que de fato usa `addend` com valor não-zero em alguns
casos). Qualquer outro tipo de relocação (ex.: `R_X86_64_TPOFF64`, TLS) é
rejeitado explicitamente (`DylibError::UnsupportedRelocationType`) em vez de
aplicado incorretamente ou ignorado silenciosamente.

### Exportação de símbolo: qualquer entrada nomeada e definida
Um símbolo de `.dynsym` é tratado como "exportado por esta biblioteca" (visível
para `dlsym`/resolução de outra biblioteca) se `st_name != 0` (tem nome) e
`st_shndx != SHN_UNDEF` (está definido aqui, não é uma referência externa).
Não distingue vinculação (`GLOBAL`/`WEAK`/`LOCAL`) nem visibilidade
(`DEFAULT`/`HIDDEN`) — simplificação explícita (Non-Goal): um símbolo
`LOCAL` tecnicamente não deveria ser visível fora do módulo, mas as
bibliotecas de teste desta change não dependem dessa distinção, e tratá-la
corretamente exigiria decodificar `st_info` com mais cuidado sem nenhum
benefício demonstrável nesta etapa.

## Risks / Trade-offs

- **[Risco]** Carregar a mesma dependência (`DT_NEEDED`) duas vezes num
  diamante (duas instâncias independentes, cada uma com sua própria cópia de
  estado global) produz comportamento diferente de um `ld.so` real para
  bibliotecas que dependem de serem *singletons* (ex.: duas cópias de um
  alocador global não coordenado) → **[Mitigação]** nenhuma — aceito
  explicitamente como Non-Goal consistente com "duas chamadas de `dlopen`
  para o mesmo caminho carregam instâncias independentes"; as bibliotecas de
  teste desta change não têm esse tipo de estado compartilhado. Revisitar
  (deduplicação real por caminho resolvido) quando uma biblioteca real
  (porte do HotSpot) exigir.
- **[Risco]** `DYLIB_STATE` (chaveado por `root`) nunca remove uma entrada —
  mesma lacuna de "nenhum processo é encerrado ainda" já aceita em
  `kernel-elf-loader`; se um `root` fosse reaproveitado por um `AddressSpace`
  futuro (não acontece hoje — `FrameAllocator` nunca reusa um frame ainda
  vivo), o estado antigo vazaria para o processo novo → **[Mitigação]**
  impossível na prática hoje (nenhum `AddressSpace` é destruído, logo nenhum
  `root` é reciclado); revisitar junto com encerramento de processo.
- **[Risco]** Slab fixo de endereço virtual por biblioteca (tamanho
  constante, escolhido arbitrariamente) pode ser pequeno demais para uma
  biblioteca real grande (ex.: um futuro `libjvm.so`) → **[Mitigação]**
  aceitável para as bibliotecas de teste desta change (poucos KiB);
  `kernel::dylib::load_library` falha explicitamente
  (`DylibError::LibraryTooLarge`) em vez de corromper memória adjacente se
  os segmentos mapeados excederem o slab — revisitar (dimensionar pelo
  tamanho real somado dos `PT_LOAD`, ou um alocador de verdade em vez de bump)
  quando uma biblioteca grande precisar carregar.
- **[Risco]** Leitura de ponteiro de usuário (`path`/`nome de símbolo` de
  `dlopen`/`dlsym`) sem validação, incluindo UTF-8 inválido → **[Mitigação]**
  mesmo precedente já aceito por `kernel::syscall::sys_write` (ver design.md
  de `kernel-syscalls`): `core::str::from_utf8` falhando é um erro explícito
  da *syscall* (`SYSCALL_ERROR`), não um pânico; um ponteiro de usuário
  realmente inválido (não mapeado) ainda causa falta de página não tratada
  para código de modo usuário — Non-Goal conhecido e já documentado de todo
  o kernel, não desta change.

## Migration Plan

Change puramente aditiva (novo módulo `kernel::dylib`, dois campos novos
expostos por `ElfHeader`, duas novas *syscalls*, novos crates de teste), sem
migração de dados nem de API existente. Nenhum rollback especial além de
reverter o commit.

## Achados durante a implementação

1. **Resolução de símbolo precisa incluir os próprios exportados da
   biblioteca em carregamento, não só as já carregadas antes dela** — achado
   ao depurar a primeira tentativa de boot real, que falhava com
   `DylibError::UnresolvedSymbol`. Causa: `base_add` (`libbase.so`) é um
   símbolo `GLOBAL` exportado, então o compilador emite `R_X86_64_64`
   (resolvida por símbolo) em vez de `R_X86_64_RELATIVE` para a referência a
   ele dentro de `BASE_ADD_FN` — **mesmo sendo uma auto-referência**, porque
   um símbolo `GLOBAL` é, em tese, interponível, e o linker não assume que a
   definição local é necessariamente a que vai prevalecer. Como
   `entry.libraries.push(...)` só acontece no final de `load_library`
   (depois das relocações, por construção — ver "Carregamento real de uma
   biblioteca", tarefa 4.3), a biblioteca ainda não está em `DYLIB_STATE`
   quando suas próprias relocações rodam, então uma busca só em
   `DYLIB_STATE` nunca a encontra. **Corrigido**: `exported_symbols` agora é
   calculado *antes* do loop de relocações (não mais só no final), e o
   `resolve_symbol` usado pela aplicação de relocações busca primeiro nos
   símbolos recém-calculados da própria biblioteca, só então em
   `DYLIB_STATE` (bibliotecas carregadas antes dela). Consequência prática:
   relocações auto-referentes a símbolos `GLOBAL` **são** suportadas, ao
   contrário do que uma leitura apressada do mecanismo sugeriria.
2. **Target `cdylib` custom exige `"dynamic-linking": true` no JSON** — sem
   esse campo, `rustc` recusa produzir um `cdylib` com
   `error: cannot produce cdylib for ... as the target ... does not support
   these crate types`, mesmo com `"relocation-model": "pic"` já presente.
   Descoberto inspecionando `rustc -Z unstable-options --print
   target-spec-json --target x86_64-unknown-linux-gnu` em busca do campo que
   faltava.
3. **Nomes de biblioteca precisam caber em 8.3 (FAT32)** — a primeira versão
   usava os nomes de pacote completos (`libelf_test_library_base.so`, muito
   longo para um nome curto FAT32). Como `kernel::fat32` só lê entradas de
   nome curto (ignora entradas de nome longo VFAT — ver spec
   `kernel-filesystem`), e a entrada `DT_NEEDED` gravada pelo linker é
   literalmente o nome do arquivo `.so` usado em tempo de link (via `-l
   <nome>`), o nome precisa ser curto **desde a origem**, não só renomeado
   ao copiar para a imagem — um arquivo renomeado na imagem não bateria mais
   com a string gravada em `DT_NEEDED`. Corrigido definindo `[lib] name =
   "base"`/`"user"` no `Cargo.toml` de cada crate de teste (nome do pacote
   continua descritivo; só o nome do artefato/biblioteca minificado),
   produzindo `libbase.so`/`libuser.so` (7+2 caracteres, cabem exatamente em
   8.3) desde a compilação.
4. **`RUSTFLAGS` (env var) substitui por completo, não combina com,
   `rustflags` de `.cargo/config.toml`** — relevante porque
   `elf_test_library_user` precisa de `-L`/`-l` (para linkar contra
   `libbase.so`) só para ela, não para `elf_test_library_base`. Em vez de
   declarar flags fixas em `.cargo/config.toml` para
   `x86_64-java_os_user_pic` (que seriam silenciosamente descartadas na
   build que também define `RUSTFLAGS`), nenhuma entrada foi adicionada lá:
   a ABI sem SSE/softfloat já vem inteiramente do próprio JSON do target
   (`features`/`rustc-abi`, não de `rustflags`), e `--hash-style=sysv`/
   `-L`/`-l` são passados via `RUSTFLAGS` só no comando de build do `xtask`
   que precisa deles.
5. **Verificação em boot real, confirmada via
   `cargo xtask run -- -serial stdio -display none`**, depois das correções
   acima: `dlopen("/lib/libuser.so")` carregou recursivamente `libbase.so`
   (sua dependência `DT_NEEDED`), aplicou com sucesso uma relocação
   `R_X86_64_RELATIVE` (`INTERNAL_DOUBLE_FN`), uma `R_X86_64_64`
   auto-referente (`BASE_ADD_FN` → `base_add`, achado 1 acima) e uma
   `R_X86_64_JUMP_SLOT` resolvida entre as duas bibliotecas
   (`user_call_base_add` → `base_add`); `dlsym` resolveu
   `user_call_base_add`, chamada com `(2, 3)` através da relocação resolvida
   e retornando `5` corretamente. Saída serial observada:
   ```
   BSS: zerado corretamente pelo carregador - PASSOU
   DLOPEN/DLSYM: chamada entre bibliotecas com resultado correto - PASSOU
   ```
   Nenhum pânico, nenhuma trava, banner de boot (`boot OK`) presente —
   `cargo xtask test` (tarefa 6.4) confirmou que o smoke test automatizado
   de boot continua passando.
