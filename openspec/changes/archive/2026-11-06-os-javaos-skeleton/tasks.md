# Tasks

## 1. Vendorização do código-fonte do OpenJDK

- [x] 1.1 Vendorizar `src/hotspot` da tag `jdk-27-ga` do repositório `openjdk/jdk` (sparse-checkout, 361 MiB — bem menor que o repositório inteiro) em `vendor/jdk-27-ga/`, não versionado no Git (`/vendor/` já coberto pelo `.gitignore` existente — confirmado)
- [x] 1.2 Verificado: a árvore vendorizada contém `share/runtime/os.hpp`, `os/bsd/`, `os_cpu/bsd_zero/`, `os/linux/`, `os_cpu/linux_zero/` e `cpu/zero/`

## 2. Levantamento do contrato real da interface

- [x] 2.1 Lido `share/runtime/os.hpp` (interface `pd_*` — ~18 funções estáticas que todo porte implementa: `pd_reserve_memory`, `pd_commit_memory`, `pd_uncommit_memory`, `pd_release_memory`, `pd_write`, `pd_create_stack_guard_pages`, `pd_start_thread`, `pd_print_cpu_info`, `pd_dll_unload`, etc.) e `share/runtime/osThread.hpp`/`osThreadBase.hpp` (`osThread.hpp` faz `#include OS_HEADER(osThread)` — exige um `osThread_javaos.hpp` nosso, declarando uma classe `OSThread` derivada de `OSThreadBase` que implementa `thread_id_for_printing()`, o único método puro virtual). **Achado**: `OS_HEADER`/`OS_CPU_HEADER` (macro que monta o nome do arquivo a incluir) dependem de `INCLUDE_SUFFIX_OS`/`INCLUDE_SUFFIX_CPU`, definidos via `-D` na linha de comando do compilador (`share/utilities/macros.hpp`) — não por um header gerado pelo `configure` do OpenJDK. Confirma que dá para compilar contra os headers reais com nosso próprio `g++` (`-DINCLUDE_SUFFIX_OS=_javaos -DINCLUDE_SUFFIX_CPU=_zero`), sem o build system do OpenJDK.
- [x] 2.2 Lido, como referência cruzada, `os/posix/include/jvm_md.h` (incluído por `os.hpp` via `#include "jvm_md.h"`, busca por caminho `-I`, não pela macro `OS_HEADER`) — **achado**: a versão POSIX puxa `<dirent.h>`/`<sys/socket.h>`/`<sys/param.h>`, que nossa libc (newlib) não tem; precisamos de um `os/javaos/include/jvm_md.h` próprio, bem mais simples, usando só os headers que a libc já portada realmente tem (`<unistd.h>`, `<fcntl.h>`, `<errno.h>`) e constantes hardcoded para o resto

## 3. `os_cpu/javaos_zero` — arquivos mais simples primeiro

- [x] 3.1 Escrever `globals_javaos_zero.hpp` (definições de flag específicas da combinação SO+variante) — verificar que compila sozinho (`-fsyntax-only`) contra os headers reais vendorizados. **Compila** (ver nota de flags de compilação no final desta seção).
- [x] 3.2 Escrever `atomicAccess_javaos_zero.hpp` e `orderAccess_javaos_zero.hpp` (operações atômicas/barreiras de memória — podem delegar para os intrínsecos do compilador, mesmo padrão de `os_cpu/*_zero` reais) — verificar compilação. **Compila.**
- [x] 3.3 Escrever `prefetch_javaos_zero.inline.hpp` (pode ser um no-op válido) — verificar compilação. **Compila.**
- [x] 3.4 Escrever `vm_version_javaos_zero.cpp` (identificação de versão/CPU — stub mínimo) — verificar compilação. **Compila** (um warning benigno pré-existente do próprio HotSpot sobre `offsetof` em tipo non-standard-layout, não introduzido por este porte).
- [x] 3.5a Escrever `javaThread_javaos_zero.hpp`/`.cpp` — **compila** (verificado contra a cadeia real completa, incluindo `share/runtime/javaThread.hpp` por inteiro — `frame.inline.hpp` → `oops/*` → `gc/shenandoah/*` → `runtime/mutex.hpp`/`park.hpp`/`osThread.hpp`/`semaphore.hpp`). Isso exigiu escrever, como pré-requisitos reais descobertos no caminho (não antecipados em `design.md`, registrados aqui em vez de contornados): `os/javaos/mutex_javaos.hpp`, `os/javaos/park_javaos.hpp`, `os/javaos/semaphore_javaos.hpp`/`.cpp` e `os/javaos/osThread_javaos.hpp` — todos idênticos em forma aos equivalentes `*_posix.hpp` (reaproveitam os tipos `pthread_mutex_t`/`pthread_cond_t` que o newlib vendorizado já declara), exceto `semaphore_javaos.*` (newlib não tem `<semaphore.h>`/`sem_t` — classe própria com corpos stub explícitos, mesma filosofia já decidida em design.md para `os_javaos.hpp`) e `osThread_javaos.hpp` (reduzido ao mínimo exigido por `OSThreadBase`, sem suspend/resume baseado em sinais POSIX — Java OS não tem sinais implementados).
- [x] 3.5b Escrever `os_javaos_zero.cpp`/`.inline.hpp` — **compila**, depois de escrever `os/javaos/os_javaos.hpp` (task 4.1, adiantada). Adaptado de `os_bsd_zero.cpp`/`os_linux_zero.cpp` removendo tudo que dependia de sinais POSIX reais (`PosixSignals::pd_hotspot_signal_handler`, `os::Posix::ucontext_get_pc/set_pc`, `os::Posix::default_stack_size`) — achado: o Java OS não tem sinais POSIX implementados, então esse porte não inclui `os_posix.hpp`. `os_javaos_zero.inline.hpp` é vazio, mesmo padrão de `os_bsd_zero.inline.hpp`/`os_linux_zero.inline.hpp`.
- [x] 3.6 Escrever `assembler_javaos_zero.cpp` (normalmente trivial para a variante Zero, já que não gera código de máquina específico de arquitetura) — verificar compilação. **Compila.**

**Nota sobre como a verificação `-fsyntax-only` foi feita (achado central desta change, afeta a tabela da task 5.1):**

A premissa original do design.md — "dá para compilar contra os headers reais só com g++ do host, sem o build system do OpenJDK" — era **parcialmente** verdadeira: dá para fazer o `-fsyntax-only`, mas não com o wrapper `scripts/cross/x86_64-elf-g++` (feito para programas de userland mínimos, `-ffreestanding -nostdlib`). O HotSpot é uma aplicação C++ hosted de verdade; `-ffreestanding` quebra em `<cmath>`/libstdc++ (erro `"This header is not available in freestanding mode"`). A invocação real usada (chamando `g++` do host diretamente, não o wrapper) foi:

```
g++ -fsyntax-only -std=gnu++17 -D_ALLBSD_SOURCE -D_LP64=1 -DTARGET_COMPILER_gcc -DINCLUDE_ZGC=0 \
  -I share -I os_cpu/javaos_zero -I os/javaos -I os/javaos/include -I os/posix -I cpu/zero \
  -I ../java.base/share/native/include -I ../java.base/unix/native/include \
  -DINCLUDE_SUFFIX_OS=_javaos -DINCLUDE_SUFFIX_CPU=_zero -DINCLUDE_SUFFIX_COMPILER=_gcc -DTARGET_ARCH_zero \
  <arquivo>
```

Achados reais que levaram a essa invocação (cada um é uma dependência concreta, não uma suposição):
- `share/utilities/globalDefinitions.hpp` (incluído por praticamente todo header do HotSpot) inclui `classfile_constants.h`, que **não é código-fonte committed** — é gerado a partir de `src/java.base/share/native/include/classfile_constants.h.template` pelo `configure`/`make` real do OpenJDK (placeholders `@@VERSION_CLASSFILE_MAJOR@@`/`@@VERSION_CLASSFILE_MINOR@@`). Resolvido vendorizando o template real (sparse-checkout ampliado para incluir `src/java.base/share/native/include/*`, 3 arquivos) e substituindo os placeholders pelos valores reais e públicos da especificação de class file para `jdk-27-ga` (major 71, minor 0 — confirmado por busca externa, não suposto).
- `jni.h` (puxado por `globalDefinitions_gcc.hpp`) inclui `jni_md.h`, que também não existe em `src/hotspot` — vive em `src/java.base/<os>/native/include/jni_md.h` por SO. Resolvido vendorizando `src/java.base/unix/native/include/*` (1 arquivo), já que o Java OS expõe uma superfície de syscall POSIX-like (consistente com a decisão já registrada no design.md de usar `os/bsd`/`os/posix` como referência).
- `share/utilities/globalDefinitions_gcc.hpp` exige que o port defina `LINUX`, `_ALLBSD_SOURCE`, `_AIX` ou `__APPLE__` via `-D` (decisão de linha de comando de cada porte real, não arquivo de código) — usamos `-D_ALLBSD_SOURCE`, consistente com a escolha de `os/bsd` como referência primária.
- `share/utilities/count_trailing_zeros.hpp` exige `-DTARGET_COMPILER_gcc` (outro define de linha de comando do build real).
- `cpu/zero/globalDefinitions_zero.hpp` inclui `<ffi.h>` — dependência real e documentada da variante Zero (usa libffi para despachar chamadas nativas). Resolvido instalando `libffi-dev` no WSL2 (pacote padrão de sistema, não específico deste projeto).
- `share/gc/z/zGlobals.hpp` inclui `CPU_HEADER(gc/z/zGlobals)` = `gc/z/zGlobals_zero.hpp`, que **não existe** — confirmado que `cpu/zero/gc/` só tem `g1/`, `shared/` e `shenandoah/`, nunca teve `z/` (ZGC não é suportado pela variante Zero upstream, faz sentido: ZGC depende de barreiras de carga geradas em código de máquina, que Zero não gera). Resolvido com `-DINCLUDE_ZGC=0` (mesmo mecanismo de feature-flag que o build real usaria para desabilitar ZGC nesta combinação).

## 4. `os/javaos` — camada principal de abstração de SO

- [x] 4.1 Escrever `os_javaos.hpp`/`os_javaos.cpp`/`os_javaos.inline.hpp` (núcleo da interface `os::`) com assinatura conforme `share/runtime/os.hpp`, corpos stub (a maioria `abort()`/erro explícito) — **compila**. **Achado real**: `os_bsd.hpp`/`os_linux.hpp` NÃO reimplementam a interface `os::` inteira — só declaram uma classe aninhada auxiliar (`os::Bsd`/`os::Linux`) com helpers específicos daquele SO; os ~18 métodos `pd_*` já são membros da própria classe `os::` (declarada em `share/runtime/os.hpp`), só precisam de definição fora de linha em `os_javaos.cpp` (todos cobertos: `pd_reserve_memory`, `pd_attempt_reserve_memory_at`, `pd_commit_memory` (2 sobrecargas), `pd_commit_memory_or_exit` (2), `pd_uncommit_memory`, `pd_release_memory`, `pd_attempt_map_memory_to_file_at`, `pd_map_memory`, `pd_unmap_memory`, `pd_disclaim_memory` (no-op real, não stub), `pd_realign_memory`, `pd_pretouch_memory` (implementação real trivial), `pd_reserve_memory_special`, `get_summary_cpu_info`/`get_summary_os_info` (implementação real), `pd_write`, `initialize_initial_active_processor_count` (implementação real), `pd_create_stack_guard_pages`, `pd_start_thread`, `pd_print_cpu_info` (implementação real), `pd_dll_unload` — maioria `fatal()`, 4 com corpo real por serem triviais/seguros). **Segundo achado real**: `share/runtime/os.hpp` (compartilhado) tem uma cadeia fixa `#if defined(AIX) #elif defined(BSD) #elif defined(LINUX) #elif defined(_WINDOWS)` de forward declarations da classe `os::<OS>` — sem ramo para SO novo. Um Java OS real precisaria editar esse arquivo compartilhado para ter uma `os::Javaos`; como nada neste esqueleto precisa dela, `os_javaos.hpp` não declara essa classe (documentado no próprio arquivo), evitando a edição a um arquivo compartilhado por enquanto.
- [x] 4.2 Escrever `osThread_javaos.hpp` conforme `share/runtime/osThreadBase.hpp` — **compila**. Reduzido ao mínimo exigido por `OSThreadBase` (só `thread_id_for_printing()`), sem suspend/resume baseado em sinais POSIX (acham real: Java OS não tem sinais). Sem `.cpp` separado — todos os métodos são inline no header, ao contrário de `osThread_bsd.cpp` (que só tinha `set_unique_thread_id()`, uma feature não portada aqui).
- [x] 4.3 Escrever `globals_javaos.hpp` (flags específicas de SO) — **compila** (já escrito e verificado durante a 3.5, adiantado por ser pré-requisito transitivo de `atomicAccess.hpp`).
- [x] 4.4 Avaliado com base no que as tasks 3.5/4.1 revelaram: além de `globals_javaos.hpp`, foram necessários `mutex_javaos.hpp`, `park_javaos.hpp`, `semaphore_javaos.hpp`/`.cpp` (todos pré-requisitos reais e transitivos de `share/runtime/javaThread.hpp`, descobertos durante a 3.5, não antecipados em design.md) — nenhum de `vmStructs_*`/outros foi necessário até agora.

## 5. Relato honesto do resultado

- [x] 5.1 Produzir uma tabela (em `design.md` desta change ou num arquivo de resultado) listando cada arquivo planejado, se compilou ou não, e — para os que não compilaram — qual dependência externa faltou. **Feito**: todos os 19 arquivos compilam; ver seção "Resultado real da implementação" em `design.md`.
- [x] 5.2 Confirmar que `cargo xtask test` e `cargo test -p kernel --lib` continuam passando. **Confirmado**: `cargo test -p kernel --lib` — 258 passed; `cargo xtask test` — smoke test de boot real passou. Esperado, já que esta change não tocou nenhum arquivo Rust/kernel (só `vendor/` e os arquivos OpenSpec).

## 6. Documentação

- [x] 6.1 Atualizar `docs/architecture/overview.md` seção 6 — feito, novo bullet com os achados reais e a lista de 7 desvios do plano original.
- [x] 6.2 Atualizar `ROADMAP.md` — feito: nova subseção "Esqueleto `os_javaos`/`os_cpu/javaos_zero` do HotSpot" em "já implementado", "destaque: o que falta para a JVM rodar" renumerado e atualizado (6 itens, incluindo o novo gap de mutex/park/semaphore real).

## Workflow follow-up

- Arquivar esta change (`opsx:archive`) depois que as tasks acima estiverem concluídas e revisadas — mesmo que o resultado seja parcial, desde que honestamente documentado (ver spec delta, "Arquivo que não compila é relatado como tal").
