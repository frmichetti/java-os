# Proposal

## Why

A decisão de `jvm-dynamic-linking-strategy` rejeitou o build totalmente
estático do JDK/Leyden (dependeria de `dlopen("/proc/self/fd/...")`,
exclusivo de Linux) e optou por um **linker ELF dinâmico mínimo implementado
no próprio kernel**. `kernel-elf-loader` entregou o pré-requisito dessa
decisão — um carregador de executável ELF64 estático (`ET_EXEC`, sem
nenhuma relocation) — mas deixou explicitamente fora de escopo qualquer
carregamento de biblioteca compartilhada. O HotSpot "de fábrica" depende de
bibliotecas compartilhadas (`libjvm.so` carregando `libjava.so` via
`dlopen`, e JNI carregando bibliotecas nativas de aplicações): sem um
mecanismo de carregamento dinâmico, não há como colocar o próprio HotSpot —
nem qualquer biblioteca `.so` real — para rodar sobre o kernel. Esta change
entrega esse mecanismo.

## What Changes

- Novo módulo `kernel::dylib`: parsing da seção dinâmica (`PT_DYNAMIC`/
  `.dynamic` — `DT_NEEDED`, `DT_SYMTAB`, `DT_STRTAB`, `DT_RELA`/`DT_RELASZ`,
  `DT_JMPREL`/`DT_PLTRELSZ`) de um objeto compartilhado ELF64 (`ET_DYN`),
  reaproveitando o parsing de cabeçalho/`program headers` já existente em
  `kernel::elf` em vez de duplicá-lo.
- Carregamento de um `.so` num endereço de base escolhido pelo kernel
  (objeto `ET_DYN`, position-independent): mapeamento dos segmentos
  `PT_LOAD` relativos a essa base (reaproveitando
  `kernel::elf::load_segments`), aplicação **eager** (sem *lazy binding* via
  PLT) das relocações x86-64 mínimas necessárias (`R_X86_64_RELATIVE`,
  `R_X86_64_GLOB_DAT`, `R_X86_64_JUMP_SLOT`, `R_X86_64_64`), e resolução de
  símbolos contra as bibliotecas já carregadas no mesmo processo
  (dependências declaradas via `DT_NEEDED` são carregadas primeiro,
  recursivamente).
- Estado de bibliotecas carregadas passa a existir **por espaço de
  endereçamento** (lista de bibliotecas carregadas e um alocador de base
  para a próxima), identificado pelo CR3 físico atualmente ativo
  (`kernel::paging::current_root()`) no momento da *syscall* — equivalente a
  "por processo" na prática (cada processo tem exatamente um `AddressSpace`
  com um `root` único), sem exigir nenhuma mudança no escalonador: mesmo
  padrão de tabela dinâmica já usado por `kernel::pthread_handles`, só que
  chaveada por `root` em vez de um índice sequencial.
- Duas novas *syscalls* expondo o mecanismo a modo usuário: `dlopen`
  (caminho no sistema de arquivos → `handle` opaco da biblioteca carregada,
  ou erro) e `dlsym` (`handle` + nome do símbolo → endereço resolvido, ou
  erro).
- Programa(s) de teste reais: uma biblioteca compartilhada ELF64 de teste
  (`.so`, `ET_DYN`, com pelo menos um símbolo exportado e um dado para
  exercitar `R_X86_64_RELATIVE`) e extensão do programa de teste existente
  (ou um novo) para chamar `dlopen`/`dlsym` e executar código vindo da
  biblioteca carregada, verificado em boot real.

Fora de escopo nesta change (consistente com "mínimo"): *lazy binding* via
resolvedor de PLT, *symbol versioning* (GNU versioning), TLS dinâmico
(`DTV`/`__tls_get_addr`), `dlclose`/contagem de referências, cache de
bibliotecas já carregadas (duas chamadas de `dlopen` para o mesmo caminho
carregam duas instâncias independentes), resolução de símbolo contra o
executável principal (só biblioteca-com-biblioteca), tabelas de hash
(`DT_HASH`/`DT_GNU_HASH` — busca de símbolo é varredura linear), e o próprio
porte do HotSpot (fica para a sub-change seguinte do roadmap).

## Capabilities

### New Capabilities
- `kernel-dynamic-linker`: carregamento de bibliotecas compartilhadas ELF64
  (`ET_DYN`) a partir do sistema de arquivos para dentro de um processo já
  em execução, incluindo resolução/aplicação de relocações, carregamento de
  dependências, e resolução de símbolo por nome — exposto a modo usuário via
  `dlopen`/`dlsym`.

### Modified Capabilities

Nenhuma. `kernel::elf::ElfHeader::parse` passa a aceitar `ET_DYN` além de
`ET_EXEC` na camada pura de parsing (reaproveitada por esta change), mas
`kernel::scheduler::spawn_from_elf` — a única função pública coberta pela
spec `kernel-elf-loader` — continua rejeitando explicitamente qualquer
`e_type` diferente de `ET_EXEC`, exatamente como hoje. O comportamento
observável da capability `kernel-elf-loader` não muda; só um detalhe de
implementação interno (reaproveitado por um módulo novo) passa a validar o
tipo em uma camada diferente.

## Impact

- **Novo código:** `kernel/src/dylib.rs` (parsing da seção dinâmica,
  relocações, resolução de símbolo, carregamento, tabela de estado por
  espaço de endereçamento), duas novas entradas em `kernel::syscall`
  (`SYSCALL_DLOPEN`, `SYSCALL_DLSYM`), um novo crate de teste (biblioteca(s)
  compartilhada(s) `.so`) e extensão do pipeline de build (`xtask`) para
  compilar e copiar essas bibliotecas para a imagem.
- **Modificado (implementação, não contrato público):**
  `kernel::elf::ElfHeader`/`ElfHeader::parse` passam a expor `e_type` e
  aceitar `ET_DYN`. Nenhuma mudança em `kernel::process`/`kernel::scheduler`.
- **Dependências:** `kernel::elf` (parsing de cabeçalho/segmentos reusado),
  `kernel::fat32` (leitura do `.so` do disco), `kernel::paging` (mapeamento,
  e `current_root()` para identificar o espaço de endereçamento chamador),
  `kernel::hhdm` (não usado para escrever relocações — ver design.md).
