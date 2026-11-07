# Design

## Context

Ver `proposal.md` para a motivação. Dois fatos do estado atual moldam toda a
abordagem técnica desta change:

1. **`os_javaos.cpp` só é verificado por compilação, nunca por link ou execução real.**
   A change `os-javaos-skeleton` compilou cada arquivo do esqueleto com o `g++` do
   *host* (WSL2/Ubuntu) em modo `-fsyntax-only`, contra os headers reais e inalterados
   do HotSpot vendorizado — um "aplicativo hosted" normal, não o alvo
   `x86_64-java_os`/`x86_64-java_os_user` do projeto. Isso prova que as assinaturas e
   os tipos batem com a interface real do HotSpot, mas nunca executou, nem sequer
   montou, nenhuma instrução para o alvo real.
2. **O wrapper de cross-compilação C++ do projeto (`scripts/cross/x86_64-elf-g++`,
   usado por `cxx-minimal-runtime`/`userland_c`) é `-ffreestanding -nostdlib`, sem
   SSE/exceções/RTTI.** `os_javaos.cpp` inclui `share/runtime/os.hpp`, que por sua vez
   inclui `utilities/globalDefinitions.hpp`/`exceptions.hpp`/`ostream.hpp` e referencia
   `class frame`/`outputStream` do resto do HotSpot compartilhado — uma árvore hosted
   de milhões de linhas, nunca pensada para `-ffreestanding`. Tentar recompilar
   `os_javaos.cpp` de verdade com esse wrapper (para linkar contra a libc/glue reais e
   rodar em QEMU) exigiria resolver toda essa cadeia de dependência — exatamente o
   trabalho que `os-javaos-skeleton` já identificou como fora de escopo até que o
   `configure`/`make` real do OpenJDK (ou um substituto equivalente) entre em cena.

Esta change não muda nenhum dos dois fatos acima — ela entrega comportamento real para
os `pd_*` de memória dentro do método de verificação já estabelecido (compilação
`-fsyntax-only` contra os headers reais), e comprova separadamente, com um programa
próprio do projeto, que a base de syscalls sobre a qual esses `pd_*` se apoiam
funciona de ponta a ponta em boot real.

## Goals / Non-Goals

**Goals:**
- Corpo real (sem `fatal()`) para os seis hooks de memória com primitiva de kernel
  equivalente: `pd_reserve_memory`, `pd_commit_memory` (2 sobrecargas),
  `pd_commit_memory_or_exit` (2 sobrecargas), `pd_uncommit_memory`,
  `pd_release_memory`, e a função não-`pd_` `os::protect_memory`.
- Uma pequena API em C (`userland_c/include/javaos_vm.h` + implementação em
  `userland_c/glue.c`) que encapsula as cinco syscalls de VM, reaproveitável tanto
  pelo `os_javaos.cpp` (via declaração, para o `-fsyntax-only` do host) quanto por um
  programa de demonstração verificado em boot real.
- Verificação em boot real (QEMU) de que a sequência reservar → comprometer →
  escrever/reler → proteger (somente leitura) → descomprometer → liberar, mais os
  casos de erro (liberar endereço que não é base, comprometer fora de reserva),
  funciona através da API em C exatamente como os novos corpos de `pd_*` vão
  depender dela. **Não inclui** provocar de propósito uma escrita numa página
  protegida como somente leitura — ver Decisions, "sem escrita real após
  `protect`", por quê.

**Non-Goals:**
- Compilar/linkar `os_javaos.cpp` com o toolchain cruzado do projeto, ou rodar o
  arquivo real do HotSpot dentro do Java OS — permanece bloqueado pela mesma cadeia de
  dependência hosted descoberta por `os-javaos-skeleton` (Non-Goal explícito daquela
  change, não revisto aqui).
- Vendorizar/instalar um JDK de bootstrap ou rodar o `configure` real do OpenJDK — é a
  alternativa mencionada pelo `ROADMAP.md` para descobrir o próximo nível de
  dependências; esta change persegue o outro braço ("implementar de verdade os `pd_*`
  sobre as syscalls já existentes"), não os dois.
- Implementar qualquer `pd_*` sem primitiva de kernel equivalente (reserva em endereço
  fixo, mapeamento de arquivo, páginas grandes) — permanecem `fatal()`, comportamento
  inalterado e agora coberto por requisito explícito na spec.
- Corrigir a limitação de `MEM_PROT_NONE` no esquema de permissões do kernel (só
  grava/executa, sempre legível) — documentada e tratada como falha explícita, não
  corrigida; uma correção de verdade pertence a uma change futura sobre
  `kernel-virtual-memory`.
- `PlatformMutex`/`PlatformMonitor`/`PlatformEvent`/`PlatformParker` (item 2 do roadmap
  médio prazo) e qualquer outro `pd_*` não relacionado a memória (`pd_start_thread`,
  `pd_create_stack_guard_pages`, `pd_dll_unload`, `pd_write`) — fora de escopo desta
  change, que é especificamente sobre memória.

## Decisions

### Uma API em C dedicada (`javaos_vm.h`) em vez de syscalls inline dentro de `os_javaos.cpp`

`userland_c/glue.c` já expõe `SYS_VM_RESERVE`/`SYS_VM_COMMIT` como constantes
privadas do arquivo (usadas só por `sbrk`) — não há hoje nenhum símbolo exportado que
outro arquivo (C ou C++) possa chamar para emitir essas syscalls. Duas alternativas:

- **(Escolhida) Expor um pequeno header C com vinculação `extern "C"`**
  (`javaos_vm_reserve`/`javaos_vm_commit`/`javaos_vm_decommit`/`javaos_vm_protect`/
  `javaos_vm_release`), implementado em `glue.c` reaproveitando a mesma
  `javaos_syscall` já existente ali. `os_javaos.cpp` só precisa da *declaração* para
  compilar (`-fsyntax-only` nunca linka), e a implementação real passa a existir no
  mesmo lugar de todo o resto da ponte syscall↔libc do projeto.
- (Rejeitada) Duplicar a sequência `syscall` inline dentro de `os_javaos.cpp` — erra
  por duas razões: (1) duplica a convenção de ABI de syscall (número em RAX, args em
  RDI/RSI/RDX/R10) que já vive centralizada em `glue.c`, igual ao comentário existente
  sobre manter `glue.c` e `kernel/src/syscall.rs` sincronizados manualmente; (2) o
  arquivo do HotSpot deixaria de ser plausivelmente portável para quando ele realmente
  for compilado/linkado pelo toolchain cruzado (Non-Goal desta change, mas não uma
  razão para fechar essa porta de propósito).

### Números de syscall para decommit/protect/release ficam hardcoded em `glue.c`, mesma convenção já aceita

`glue.c` já documenta explicitamente (comentário em torno da linha 34) que os números
de syscall são "mantidos em sincronia manualmente com `kernel/src/syscall.rs` (sem
cabeçalho compartilhado entre Rust e C nesta change)". Esta change só estende a mesma
lista já existente (`SYS_VM_RESERVE=2`, `SYS_VM_COMMIT=3`) com `SYS_VM_DECOMMIT=4`,
`SYS_VM_PROTECT=5`, `SYS_VM_RELEASE=6` — os mesmos valores de
`kernel::syscall::SYSCALL_VM_DECOMMIT`/`PROTECT`/`RELEASE`. Introduzir um cabeçalho
compartilhado Rust↔C é uma mudança estrutural maior, fora do escopo pontual desta
change (mesmo Non-Goal implícito já aceito por `libc-minimal-port`).

### `os::protect_memory(MEM_PROT_NONE)` devolve `false` em vez de aproximar com `MEM_PROT_READ`

Duas opções ao receber um pedido que o kernel não consegue satisfazer fielmente:
aproximar silenciosamente (ex.: tratar `MEM_PROT_NONE` como `MEM_PROT_READ`, já que é
"o mais perto" que o kernel permite) ou recusar explicitamente. Escolhido recusar
(`false`) — aproximar sem avisar violaria a premissa de segurança de quem pediu
`MEM_PROT_NONE` (tipicamente para criar uma página de guarda) sem que o chamador tenha
como perceber a diferença; falhar de forma explícita deixa o HotSpot decidir o que
fazer com a indisponibilidade dessa primitiva, mesma filosofia dos `pd_*` sem
primitiva equivalente (`fatal()` explícito em vez de comportamento parcial
silencioso).

### Verificação em boot real via um programa C real (ELF64, newlib+glue), não um `kernel::*_demo` em assembly

Dado o Non-Goal de linkar o arquivo real do HotSpot nesta change (ver Context), a
verificação em boot real precisa de um programa que exercite exatamente a mesma
sequência de chamadas que os novos corpos de `pd_*` vão fazer — reserva → commit →
escrita/releitura → protect(somente leitura) → decommit → release, mais os casos de
erro — sem ser, literalmente, `os_javaos.cpp`. Duas alternativas:

- **(Rejeitada, corrigida durante a implementação) Um novo módulo `kernel::*_demo` em
  `global_asm!`**, mesmo padrão de `user_posix_demo.rs`. Pareceu a escolha óbvia por
  analogia, mas um módulo desses é um blob de código de posição independente,
  copiado byte a byte do binário do kernel para uma página de usuário (ver
  `copy_into`/`program_start_addr` de `user_posix_demo.rs`) — **sem nenhum passo de
  link**. Ele não pode chamar uma função de `javaos_vm.h`/`glue.c` (compilada à parte
  pelo toolchain cruzado, como objeto ELF relocável) porque não existe nenhum link
  entre os dois: só pode emitir a instrução `syscall` diretamente, o que já é
  exatamente o que `user_posix_demo.rs` (`kernel-jvm-posix-syscalls`) já verifica para
  as mesmas cinco syscalls de VM. Repetir isso em outro módulo não provaria nada novo
  sobre os wrappers de C que os `pd_*` de fato vão chamar.
- **(Escolhida) Um programa C real** (`userland_c/javaos_vm_test_program.c`),
  compilado pelo wrapper `scripts/cross/x86_64-elf-gcc` e linkado estaticamente com
  `crt0.o`/`glue.o`/`libc.a` do newlib vendorizado — mesmo mecanismo exato de
  `userland_c/libc_test_program.c` (`libc-minimal-port`) e
  `userland_c/cxx_test_program.cpp` (`cxx-minimal-runtime`). Carregado como ELF64 real
  via `kernel::elf`/`spawn_from_elf` (mesmo `kernel-elf-loader` já usado pelos dois
  programas citados), este programa chama diretamente
  `javaos_vm_reserve`/`commit`/`decommit`/`protect`/`release` — a mesma API, com a
  mesma convenção de chamada C, que `os_javaos.cpp` vai usar. É a única forma de
  provar que a API em si (não só a syscall bruta por trás dela) funciona de ponta a
  ponta.

A tradução C++ de `os_javaos.cpp` em si continua coberta só pela verificação de
compilação já estabelecida (ver Non-Goals) — a mesma honestidade de "o que foi
verificado e como" já praticada pelo relato final de `os-javaos-skeleton`.

### Sem escrita real após `protect(somente leitura)` no programa de verificação

O programa de verificação (`javaos_vm_test_program.c`) protege uma página como
somente leitura, mas **não tenta escrever nela depois** — ao contrário do que o
plano original desta change presumia. Uma escrita de modo usuário contra uma página
somente leitura gera um `#PF` real que, hoje, **derruba o kernel inteiro** (nenhum
isolamento de falha por processo existe ainda — ver `ROADMAP.md`, médio prazo).
`kernel::user_posix_demo` (`kernel-jvm-posix-syscalls`), a demonstração já existente
mais parecida com esta, tem exatamente a mesma cautela: protege como somente leitura
e nunca escreve depois. O comportamento de permissão em si (uma escrita após
`protect(MEM_PROT_READ)` falha) já é um requisito garantido e verificado por
`kernel-virtual-memory`; esta change só precisa confirmar que `javaos_vm_protect`
chega até a mesma syscall e recebe sucesso, não precisar re-provar a aplicação da
permissão arriscando a estabilidade do kernel inteiro de novo.

## Risks / Trade-offs

- **[Risco] A implementação real de `pd_*` em `os_javaos.cpp` nunca é verificada
  executando o arquivo real** (só por `-fsyntax-only` + um programa equivalente, não
  idêntico) → Mitigação: aceito como resultado honesto, documentado explicitamente na
  spec e no relato de implementação (mesma prática de `os-javaos-skeleton`); o `Risco`
  residual é que uma diferença sutil entre o programa de demonstração e o corpo real
  de `os_javaos.cpp` passe despercebida — mitigado mantendo os dois o mais próximos
  possível linha a linha (tasks.md vai exigir isso explicitamente).
- **[Risco] `MEM_PROT_NONE` sempre falhando pode impedir, mais adiante, qualquer
  código do HotSpot que dependa de páginas de guarda via `protect_memory`** (ex.:
  `os::guard_memory`, usado por `create_stack_guard_pages`) → Mitigação: aceito
  deliberadamente (ver Non-Goals) — o próprio kernel já tem seu próprio mecanismo de
  guard page via desmapeamento (`kernel::stack`), não via `mprotect`; se/quando o
  HotSpot precisar de guard pages de verdade, o caminho mais correto é uma função
  `pd_create_stack_guard_pages` que desmapeia (não protege) a sub-região, trabalho de
  uma change futura fora de escopo aqui.
- **[Risco] Divergência futura entre os números de syscall hardcoded em `glue.c` e
  `kernel/src/syscall.rs`** — já um risco aceito do projeto desde `libc-minimal-port`,
  não introduzido nem agravado por esta change além de três novas constantes.

## Migration Plan

Puramente aditivo — nenhum código existente (kernel, libc, runtime C++, programas de
teste já existentes) muda de comportamento. Nenhum passo de migração necessário.

## Resultado real da implementação (relato honesto — tasks 3.7/4.1)

Os seis hooks de memória com primitiva de kernel equivalente agora têm corpo real em
`vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp`, verificados por
`g++ -fsyntax-only -std=gnu++17` contra os headers reais e inalterados do HotSpot
vendorizado (mesma invocação de `os-javaos-skeleton`, com `-I <repo>/userland_c/include`
adicional para resolver `javaos_vm.h`) — sem erros, um único warning pré-existente dos
próprios headers do HotSpot (`-Winvalid-offsetof` em `share/utilities/exceptions.hpp`),
alheio a esta change.

### Tabela de compilação/comportamento por hook de memória (atualização de
`os-javaos-skeleton`)

| Hook | Antes desta change | Depois desta change |
|---|---|---|
| `pd_reserve_memory` | `fatal()` | Real — `javaos_vm_reserve` |
| `pd_commit_memory` (2 sobrecargas) | `fatal()` | Real — `javaos_vm_commit` |
| `pd_commit_memory_or_exit` (2 sobrecargas) | `fatal()` | Real — delega a `pd_commit_memory`, `vm_exit_out_of_memory` na falha |
| `pd_uncommit_memory` | `fatal()` | Real — `javaos_vm_decommit` |
| `pd_release_memory` | `fatal()` | Real — `javaos_vm_release` |
| `os::protect_memory` | **Ausente do arquivo** (não era stub — não existia) | Real — `javaos_vm_protect`, `MEM_PROT_NONE` recusado |
| `pd_attempt_reserve_memory_at` | `fatal()` | `fatal()` (sem reserva em endereço fixo) |
| `pd_attempt_map_memory_to_file_at` | `fatal()` | `fatal()` (sem mmap de arquivo) |
| `pd_map_memory` | `fatal()` | `fatal()` (sem mmap de arquivo) |
| `pd_unmap_memory` | `fatal()` | `fatal()` (sem mmap de arquivo) |
| `pd_reserve_memory_special` | `fatal()` | `fatal()` (sem páginas grandes) |
| `pd_realign_memory` | `fatal()` | `fatal()` (sem primitiva de realinhamento — achado só percebido ao enumerar todos os `pd_*` de memória durante a implementação; `proposal.md` original não o citava) |
| `pd_disclaim_memory`/`pd_pretouch_memory` | Já real (no-op/dica, de `os-javaos-skeleton`) | Inalterado |

### Verificação em boot real

`userland_c/javaos_vm_test_program.c`, carregado como `/bin/javaosvm.elf` via
`kernel::elf`/`spawn_from_elf`, reportou no console serial (QEMU, `cargo xtask
build-image` + `qemu-system-x86_64 ... -display none -serial stdio`):

```
javaos_vm: /bin/javaosvm.elf carregado e em execucao
javaos_vm_test_program: resultado PPPPPPPP (ordem: reserve,commit,pattern,protect,decommit,release,release_mid_err,commit_err)
```

Todos os 8 cenários passaram (reservar, comprometer, escrever/reler, proteger,
descomprometer, liberar, e as duas falhas esperadas — liberar endereço no meio de uma
reserva, comprometer endereço fora de qualquer reserva). Nenhum `EXCECAO`/pânico no
log completo de boot; o timer continuou avançando normalmente depois (`wait_queue:
consumidora despertou` seguiu incrementando até o fim da sessão de QEMU).

### Achados reais que exigiram desviar do plano original

1. **Um módulo `kernel::*_demo` em `global_asm!` (plano original de tasks.md, grupo 2)
   não pode exercitar a API em C** — é um blob de código de posição independente
   copiado sem nenhum passo de link; não existe caminho para ele chamar uma função
   compilada à parte. Corrigido ainda na fase de implementação: o programa de
   verificação virou um ELF C real (`userland_c/javaos_vm_test_program.c`), mesmo
   mecanismo de `libc_test_program.c`/`cxx_test_program.cpp`. Ver Decisions,
   "Verificação em boot real via um programa C real", para o racional completo.
2. **Uma escrita real após `protect(somente leitura)` no programa de verificação
   derrubaria o kernel inteiro** (sem isolamento de falha por processo ainda) — o
   plano original de tasks.md citava esse passo; removido do programa de verificação
   antes de rodar em boot real, não depois de uma falha observada. Ver Decisions, "Sem
   escrita real após `protect(somente leitura)`".
3. **`os::protect_memory` realmente não existia** no arquivo (confirmado por `grep`
   antes de escrever qualquer código) — não só um achado de pesquisa do `proposal.md`,
   confirmado de novo na implementação.
4. **`pd_realign_memory` é mais um `pd_*` de memória sem primitiva de kernel
   equivalente**, não citado pela enumeração original do `proposal.md` — adicionado à
   lista de stubs explicitamente documentados (ver tabela acima e spec delta) só
   percebido ao conferir a lista completa de 18 `pd_*` do comentário de topo de
   `os_javaos.cpp` contra o que esta change de fato cobria.
