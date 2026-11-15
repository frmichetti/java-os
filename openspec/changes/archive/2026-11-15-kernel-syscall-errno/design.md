# Design

## Context

Ver proposal.md para a motivação completa. `kernel::syscall` hoje devolve
`SYSCALL_ERROR = u64::MAX` para praticamente toda falha, mais dois
sentinelas ad-hoc (`SYSCALL_TIMEOUT = u64::MAX - 1`, `SYSCALL_EFAULT =
u64::MAX - 2`, da change `kernel-user-pointer-validation`). Internamente,
várias funções já calculam um erro específico — `VmmError` (7 variantes),
`Fat32Error` (5 variantes), `DylibError` (14 variantes) — descartado na
fronteira da *syscall*. `userland_c/glue.c` já precisa de `errno` real para
a libc C (`newlib`) funcionar corretamente; hoje adivinha, às vezes errado
(ver proposal.md).

## Goals / Non-Goals

**Goals:**
- Toda *syscall* que falha devolve um código de erro de `<errno.h>` real
  (os mesmos valores numéricos do `sys/errno.h` do newlib vendorizado),
  codificado como `-errno` (convenção do kernel Linux).
- `userland_c/glue.c` propaga esse valor real para `errno`, em vez de
  adivinhar por função.
- Toda distinção de erro hoje só visível internamente (ex.: "já estava
  tomado" vs. "`handle` desconhecido" em `SYSCALL_MUTEX_TRYLOCK`) fica
  observável pelo chamador.

**Non-Goals:**
- Introduzir uma categoria de erro nova que não exista em nenhuma condição
  de falha já detectada hoje — esta change só torna granular o que já é
  distinguido internamente (ou facilmente distinguível com o código já
  existente), nunca inventa uma checagem nova.
- Reverter a decisão de `kernel-handle-ownership` de usar o mesmo sentinela
  para "`handle` desconhecido" e "`handle` de outro processo" — ambos
  continuam o mesmo código de erro (`EINVAL`/`EBADF`, conforme a tabela
  abaixo), nunca distintos entre si (ver design.md daquela change, "nunca um
  código novo").
- Qualquer mudança na numeração, argumentos, ou valor de sucesso de
  qualquer *syscall* — só a forma do valor de falha muda.
- Mapear exaustivamente cada uma das 14 variantes de `DylibError` para um
  código distinto — a maioria representa uma biblioteca ELF malformada de
  alguma forma, todas colapsadas em `ENOEXEC` (ver tabela abaixo,
  "Colapsando variantes de baixo nível").

## Decisions

### Codificação: `-errno` como `u64`, convenção do Linux

```rust
/// Codifica um código de erro POSIX (`<errno.h>` do newlib vendorizado,
/// `vendor/newlib-.../newlib/libc/include/sys/errno.h`) como valor de
/// retorno de syscall — mesma convenção usada pelas syscalls reais do
/// kernel Linux: `-errno` como inteiro de 64 bits assinado, reinterpretado
/// como `u64`. Qualquer valor de sucesso de qualquer syscall (endereço,
/// handle, contagem de bytes, nanossegundos) está muito abaixo da faixa
/// reservada por essa codificação (`errno` é sempre < 300, então o valor
/// codificado é sempre > u64::MAX - 300) — nenhuma ambiguidade possível.
pub fn syscall_err(errno: i32) -> u64 {
    (-(errno as i64)) as u64
}
```

`SYSCALL_ERROR`/`SYSCALL_TIMEOUT`/`SYSCALL_EFAULT` são removidos — cada
antigo uso vira uma chamada a `syscall_err(<ERRNO_ESPECÍFICO>)`. **Alternativa
rejeitada**: manter os três sentinelas antigos como códigos adicionais,
coexistindo com os novos `errno` reais — rejeitada por criar duas
convenções permanentes no mesmo arquivo (qual delas um código novo deveria
usar?), quando uma migração única e completa custa o mesmo esforço de
qualquer forma (praticamente todo `sys_*` já precisa ser tocado).

### Mapeamento de erro por syscall

Convenção da tabela: `(causa) → ERRNO`. Causas que já não existem antes
desta change (ex.: distinguir "já tomado" de "`handle` desconhecido" em
`SYSCALL_MUTEX_TRYLOCK`) estão marcadas **(novo)**.

| Syscall | Causa | Errno |
|---|---|---|
| `SYSCALL_GET_TICKS`/`CLOCK_WALL`/`CLOCK_MONOTONIC`/`MUTEX_CREATE`/`COND_CREATE` | nunca falham | — |
| Qualquer syscall, número não reconhecido | dispatch cai no caso `_` | `ENOSYS` |
| Qualquer syscall que recebe ponteiro (`WRITE`/`DLOPEN`/`DLSYM`/`OPEN`/`READ`) | `user_range_is_valid` rejeita | `EFAULT` (sem mudança de causa, só de valor concreto — era `SYSCALL_EFAULT`) |
| `VM_RESERVE` | tamanho zero (`VmmError::Misaligned`) | `EINVAL` |
| `VM_RESERVE` | sem espaço virtual livre (`OutOfVirtualSpace`) | `ENOMEM` |
| `VM_COMMIT`/`VM_DECOMMIT`/`VM_PROTECT`/`VM_RELEASE` | endereço fora de reserva viva (`region_containing` falha, `StaleHandle`) | `EINVAL` |
| `VM_COMMIT`/`VM_DECOMMIT`/`VM_PROTECT` | `Misaligned`/`OutOfBounds`/`AlreadyCommitted`/`NotCommitted` | `EINVAL` |
| `VM_COMMIT` | sem memória física (`OutOfFrames`) **(novo — distinto de `EINVAL`)** | `ENOMEM` |
| `VM_RELEASE` | endereço não é a base exata da reserva | `EINVAL` |
| `MUTEX_LOCK`/`MUTEX_UNLOCK`/`COND_NOTIFY_ONE`/`COND_NOTIFY_ALL` | `handle` desconhecido/de outro processo | `EINVAL` |
| `MUTEX_TRYLOCK` | `handle` desconhecido/de outro processo **(novo — distinto de `EBUSY`)** | `EINVAL` |
| `MUTEX_TRYLOCK` | mutex já tomado | `EBUSY` (mesmo código que `pthread_mutex_trylock(3)` POSIX real) |
| `COND_WAIT` | `handle` de cond/mutex desconhecido | `EINVAL` |
| `COND_WAIT` | prazo expirou **(era `SYSCALL_TIMEOUT`)** | `ETIMEDOUT` |
| `DLOPEN`/`OPEN` | caminho não é UTF-8 válido | `EINVAL` |
| `DLOPEN`/`OPEN` | falha ao montar o sistema de arquivos, ou `Fat32Error::NotFat32`/`UnsupportedSectorSize` | `EIO` |
| `DLOPEN`/`OPEN` | disco não inicializado (`with_disk` → `None`) | `ENODEV` |
| `DLOPEN`/`OPEN` | `Fat32Error::NotFound` | `ENOENT` |
| `DLOPEN`/`OPEN` | `Fat32Error::NotAFile` | `EISDIR` |
| `DLOPEN`/`OPEN` | `Fat32Error::NotADirectory` | `ENOTDIR` |
| `DLOPEN` | `DylibError::CircularDependency` | `ELOOP` |
| `DLOPEN` | qualquer outra variante de `DylibError` — ELF/segmento/relocação malformados (`UnsupportedLibrary`, `ProgramHeaderTableOutOfBounds`, `MissingDynamicSegment`, `DynamicSegmentOutOfBounds`, `MissingRequiredTag`, `UnsupportedPltRelType`, `AddressOutOfBounds`, `UnterminatedString`, `UnsupportedRelocationType`, `UnresolvedSymbol`, `LibraryTooLarge`) — ver "Colapsando variantes de baixo nível" | `ENOEXEC` |
| `DLSYM` | nome não é UTF-8 válido | `EINVAL` |
| `DLSYM` | `DylibError::InvalidHandle` | `EBADF` |
| `DLSYM` | `DylibError::SymbolNotFound` | `ENOENT` |
| `READ`/`CLOSE` | `handle` desconhecido/de outro processo | `EBADF` (mesmo código real de `read(2)`/`close(2)` POSIX para descritor inválido) |

### Colapsando variantes de baixo nível (`DylibError` → `ENOEXEC`)

11 das 14 variantes de `DylibError` representam, de uma forma ou de outra,
"este arquivo não é uma biblioteca compartilhada ELF64 carregável" — a
mesma categoria que sistemas reais reportam como `ENOEXEC` ("Exec format
error") para um `dlopen`/`execve` com um binário malformado. Distinguir
cada uma dessas 11 variantes com um código de erro próprio exigiria
inventar significado POSIX que não existe (não há um errno padrão para
"segmento `PT_DYNAMIC` fora dos limites do arquivo" especificamente) — a
granularidade real e útil para quem chama `dlopen` é "não encontrado" vs.
"não é uma biblioteca válida" vs. "dependência circular", não o motivo
exato da invalidade.

## Risks / Trade-offs

- **[Risco] Uma syscall de sucesso devolver, por acidente, um valor dentro
  da faixa reservada a erro (`u64::MAX - 300` em diante)** → Mitigação: a
  única syscall que devolve um endereço de usuário bruto sem tradução é
  `SYSCALL_VM_RESERVE`/`SYSCALL_DLSYM`, ambas sempre dentro de janelas de
  endereço virtual gerenciadas bem abaixo de `u64::MAX` (`VMM_WINDOW_START
  = 0xffff_c000_0000_0000` + 64 GiB, e o slab de biblioteca carregada,
  igualmente longe) — nunca perto da faixa reservada. Nenhuma mudança
  necessária nessas janelas.
- **[Risco] `userland_c/efault_test_program.c` (change
  `kernel-user-pointer-validation`) compara o retorno bruto contra
  `SYSCALL_EFAULT` (`-3L`), que deixa de existir** → Mitigação: atualizar
  esse arquivo para comparar contra `-14L` (`EFAULT`), com um comentário
  explicando a mudança — tarefa própria abaixo.
- **[Risco] Divergência entre o valor de `errno` que `kernel/src/syscall.rs`
  usa e o valor real de `<errno.h>` do newlib, se algum dia divergirem** →
  Mitigação: nenhum cabeçalho compartilhado entre Rust e C existe neste
  projeto (Non-Goal já documentado em changes anteriores) — os valores
  numéricos são copiados manualmente para constantes Rust, com um
  comentário apontando o arquivo de origem exato
  (`vendor/newlib-.../sys/errno.h`), mesmo padrão já usado para os números
  de syscall em `glue.c`.

## Migration Plan

Nenhum passo de migração em runtime (nada persiste entre versões do
kernel). Para o código-fonte: todo chamador que só verifica sucesso (o caso
comum, e o único jeito de "não quebrar" hoje, já que nenhum programa de
teste existente inspeciona o valor exato de `SYSCALL_ERROR`) continua
funcionando sem mudança. Os dois chamadores que inspecionam um valor de
erro específico (`userland_c/glue.c`'s `pthread_cond_timedwait` compara
contra `SYSCALL_TIMEOUT`; `userland_c/efault_test_program.c` compara contra
`SYSCALL_EFAULT`) são atualizados nas tarefas desta change.

## Resultado real da implementação

**Testes de host**: `cargo test -p kernel --lib` passou com 288 testes (287
de antes desta change + 1 novo, `syscall_err_encodes_errno_as_negative_i64_reinterpreted_as_u64`
— a maioria das mudanças renomeou/ajustou testes já existentes em vez de
adicionar novos, já que praticamente toda distinção de causa de erro já
tinha um teste próprio antes, só comparando contra o mesmo sentinela
genérico).

**Boot real** (`cargo xtask test`): confirmado, com um achado real grave no
meio do caminho (ver abaixo) — depois de corrigido, todo programa de teste
reporta exatamente o mesmo padrão de sucesso de antes desta change, e o
novo esquema de erro granular funciona como projetado.

### Achado real grave: `r < 0` não é um teste de erro válido neste kernel

A primeira rodada de verificação em boot real revelou regressões reais:
`javaos_vm_test_program` (`resultado FFFFFFFP`), `libc_test_program`
(`FPFF`), `user_posix_demo` (`PPPPPPPPPPFF`) e `file_syscalls_demo`
(`PPPPPFFF`) — todos `P` completo antes desta change.

**Causa raiz**: `userland_c/glue.c`'s primeira versão de `is_syscall_error`
era `return r < 0;` — correta para o kernel Linux real (onde endereços de
usuário vivem na metade inferior do espaço de endereçamento, bit 63 sempre
zero), mas **errada para este kernel**: `kernel::vmm::VMM_WINDOW_START =
0xffff_c000_0000_0000` tem o bit 63 ligado — qualquer endereço dentro dessa
janela (devolvido por `SYSCALL_VM_RESERVE`, e portanto por `sbrk`/`malloc`)
aparenta **negativo** quando reinterpretado como `long` assinado de 64
bits em C, mesmo sendo um endereço de sucesso perfeitamente válido. Isso
fazia `is_syscall_error` classificar como erro toda reserva de memória
virtual bem-sucedida — derrubando `malloc` (e, por tabela, `fopen`/`fread`,
que dependem dele internamente) e as cinco operações de `javaos_vm_*`.

**Correção**: `is_syscall_error` passou a verificar a faixa estreita
reservada de verdade pela codificação (`r <= -1L && r >= -300L`, mesma
faixa de `kernel::syscall::syscall_err`), não "qualquer valor negativo".
Esta é a lição geral: ao adotar a convenção `-errno` num kernel que não
segue a mesma disciplina de layout de endereço do Linux (metade superior
reservada só para o próprio kernel, nunca para o espaço de usuário), o
teste "é um erro?" do lado C **não pode** ser uma checagem de sinal
genérica — precisa ser a mesma checagem de faixa estreita usada do lado
Rust (`v > u64::MAX - 300`, equivalente a `r >= -300L` em `long` assinado).

**Achado relacionado, no próprio kernel**: `kernel::file_syscalls_demo`/
`kernel::user_posix_demo` (demos em assembly do próprio kernel, não
`glue.c`) tinham comparações `cmp rax/r12/r13/r14, -1` hardcoded contra o
antigo sentinela único — corrigidas para o código de erro específico
esperado em cada cenário (`EBADF`/`ENOENT`/`EINVAL`), ou para uma checagem
de sinal nos casos em que o valor de sucesso é sempre um inteiro pequeno
não-negativo (handle de mutex/condvar/arquivo — nunca um endereço da janela
de VMM, então a checagem de sinal genérica é segura só nesses casos
específicos).

### Outros achados reais menores

- **`Fat32Error` tem uma sexta variante não antecipada**: `ReadFailed`
  ("uma leitura de bloco falhou ao consultar o disco"), ausente do
  catálogo original da tabela de mapeamento — mapeada para `EIO`, junto de
  `NotFat32`/`UnsupportedSectorSize` (mesma categoria "falha de
  infraestrutura de armazenamento").
- **Lacunas de verificação conhecidas, aceitas**: a distinção `EINVAL`/
  `ENOMEM` em `SYSCALL_VM_RESERVE`/`SYSCALL_VM_COMMIT` não tem teste de
  host dedicado (simular um alocador de frames/espaço virtual esgotado
  exigiria infraestrutura que não existe hoje em `vmm::tests` para este
  caso) nem é exercitada por nenhum programa de boot real existente — só
  verificada por revisão de código, reaproveitando os mesmos `match`/
  variantes de `VmmError` já testados em `kernel::vmm`. Da mesma forma, o
  `ENOTDIR` de `SYSCALL_OPEN`/`SYSCALL_DLOPEN` (componente intermediário do
  caminho não é diretório) não é exercitado por nenhum teste existente nem
  novo — a imagem de disco de boot real só tem `/TEST.TXT` na raiz, sem
  nenhum caminho com esse formato. Mesma categoria de aceitação de lacuna
  já usada em changes anteriores para caminhos que só tocam hardware real.
