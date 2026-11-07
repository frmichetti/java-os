# Proposal

## Why

O esqueleto `os/javaos`/`os_cpu/javaos_zero` do HotSpot (change `os-javaos-skeleton`,
arquivada em 2026-11-06) prova que o porte **compila** contra a interface real do
HotSpot, mas nenhum dos ~18 métodos `pd_*` tem comportamento real — a maioria é
`fatal("... nao implementado neste esqueleto")`. O próprio `ROADMAP.md` (seção "O que
está previsto" → "Próximo passo imediato") e o relato final de `os-javaos-skeleton`
(design.md, "Próximo passo real") apontam o mesmo item como a continuação natural:
implementar de verdade os `pd_*` de memória sobre as syscalls
`SYSCALL_VM_RESERVE`/`COMMIT`/`DECOMMIT`/`PROTECT`/`RELEASE` que o kernel já expõe e
verifica (`kernel-virtual-memory`, `kernel-jvm-posix-syscalls`). Sem isso, qualquer
código do HotSpot que chame `os::reserve_memory`/`commit_memory`/etc. (usado
extensivamente pela JVM para heap, metaspace, pilhas de thread) ainda derruba o
processo com `fatal()` em vez de alocar memória de verdade.

## What Changes

- Expor, em `userland_c/glue.c` (mais um novo header de uso interno,
  `userland_c/include/javaos_vm.h`), wrappers em C com vinculação `extern "C"` para as
  cinco operações de VM já existentes no kernel: reservar, comprometer, descomprometer,
  proteger e liberar uma região. `SYS_VM_RESERVE`/`SYS_VM_COMMIT` já existem em
  `glue.c` (usados por `sbrk`); `SYS_VM_DECOMMIT`/`SYS_VM_PROTECT`/`SYS_VM_RELEASE`
  ainda não têm número de syscall nem wrapper ali.
- Implementar de verdade, em `vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp`,
  os corpos de `pd_reserve_memory`, `pd_commit_memory` (as duas sobrecargas),
  `pd_commit_memory_or_exit` (as duas sobrecargas), `pd_uncommit_memory` e
  `pd_release_memory`, chamando os wrappers acima em vez de `fatal()`.
- Implementar de verdade `os::protect_memory` (método não-`pd_`, mas específico de
  plataforma — hoje ausente do arquivo, não apenas stub; ver Impact) usando
  `SYSCALL_VM_PROTECT`. **Limitação documentada, não corrigida nesta change**:
  `ProtType::MEM_PROT_NONE` não tem equivalente no esquema de permissões do kernel
  (`prot_to_page_flags` só decodifica gravável/executável, uma página mapeada é sempre
  legível) — `protect_memory(..., MEM_PROT_NONE)` SHALL devolver `false` em vez de
  fingir que a página ficou inacessível.
- Manter como stub explícito (`fatal()`, sem mudança de comportamento) todo `pd_*` de
  memória sem primitiva de kernel equivalente: `pd_attempt_reserve_memory_at` (sem
  reserva em endereço fixo), `pd_attempt_map_memory_to_file_at`/`pd_map_memory`/
  `pd_unmap_memory` (sem mapeamento de arquivo em memória), `pd_reserve_memory_special`
  (sem páginas grandes), `pd_realign_memory` (sem primitiva de realinhamento de região
  já reservada). Isso não é uma omissão desta change — é a mesma prática já documentada
  pela change `os-javaos-skeleton` de relatar lacunas em vez de escondê-las.
- Adicionar um novo programa de demonstração em modo usuário no kernel (mesmo padrão de
  `kernel::user_posix_demo`) que exercita diretamente os cinco novos wrappers de
  `javaos_vm.h`/`glue.c` — reservar, comprometer, escrever, proteger como somente
  leitura, confirmar que a escrita agora falha (mesmo mecanismo de `#PF` já coberto por
  `kernel-virtual-memory`), descomprometer, liberar — carregado como ELF64 real e
  verificado em boot real via QEMU/serial. Isso prova que a base sobre a qual os `pd_*`
  se apoiam funciona de ponta a ponta; a tradução de `os_javaos.cpp` em si continua
  verificada apenas por compilação (`-fsyntax-only` contra os headers reais do
  HotSpot, mesmo método já usado por `os-javaos-skeleton` — ver design.md para o porquê
  de não ser possível compilar/linkar o arquivo real contra o toolchain cruzado nesta
  change).

## Capabilities

### New Capabilities
- `os-javaos-memory`: comportamento real (não mais stub) dos hooks de memória da
  camada `os`/`os_cpu` do porte HotSpot (`os_javaos.cpp`) — reservar, comprometer,
  descomprometer, proteger e liberar memória virtual, apoiados nas syscalls de VM que o
  kernel já expõe, incluindo os casos em que nenhuma primitiva de kernel equivalente
  existe e o comportamento continua sendo uma falha explícita e documentada.

### Modified Capabilities
(nenhuma — `os-javaos-skeleton` não muda: seus requisitos são sobre assinatura e
compilação, não sobre comportamento; `kernel-virtual-memory` não muda: esta change só
consome as syscalls já especificadas e verificadas por ela, sem alterar seu contrato.)

## Impact

- **Código afetado**: `vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp` (árvore
  vendorizada do HotSpot — edição pontual permitida pelo próprio propósito da change,
  mesma prática de `os-javaos-skeleton`); `userland_c/glue.c` e um novo
  `userland_c/include/javaos_vm.h`; um novo módulo de demonstração em
  `kernel/src/` (nome exato a definir em tasks.md) registrado em `kernel/src/lib.rs`.
- **Achado real a documentar nesta change** (não uma mudança de escopo): ao inspecionar
  `os_javaos.cpp` durante esta pesquisa, `os::protect_memory` — exigido por
  `share/runtime/os.hpp:559` e usado por `os::guard_memory`/`create_stack_guard_pages`
  no código compartilhado — **não existe no arquivo**, nem como stub. A tabela de
  compilação por arquivo do relato final de `os-javaos-skeleton` não cobre essa lacuna
  porque nenhum caminho de compilação `-fsyntax-only` arquivo-a-arquivo a exercitou.
  Esta change passa a defini-lo.
- **Sem mudança em**: ABI de syscall do kernel, `kernel-virtual-memory`, qualquer
  programa de usuário existente (`user_posix_demo`, `sbrk`/heap da libc).
- **Dependências**: nenhuma nova — usa apenas as cinco syscalls de VM já implementadas
  e testadas por `kernel-posix-virtual-memory`/`kernel-jvm-posix-syscalls`.
