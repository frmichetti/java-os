# Design

## Context

Até aqui, todo o código de modo usuário do Java OS (libc C, runtime C++, programas de teste) foi escrito do zero ou portado de uma biblioteca pequena e desacoplada (newlib). O HotSpot é diferente: é uma base de código de milhões de linhas com acoplamento profundo entre suas partes "compartilhadas" (`share/`) e as específicas de plataforma (`os/<plataforma>`, `cpu/<arquitetura>`, `os_cpu/<plataforma>_<arquitetura>`).

**Pesquisa feita nesta sessão** (via busca na web e inspeção direta da árvore de código-fonte real do OpenJDK no GitHub, já que não há como compilar o HotSpot de verdade neste ambiente de desenvolvimento):

- **Bootstrap JDK**: compilar o OpenJDK pelo seu próprio sistema de build (`configure`/`make`, baseado em autoconf) exige um JDK já funcional instalado no host (build de JDK versão N tipicamente usa um JDK N-1 ou N como "boot JDK"). Autoconf 2.69+ e GNU Make 3.81+ também são exigidos. Nenhum desses pré-requisitos existe hoje no ambiente de desenvolvimento deste projeto.
- **Estrutura de código específico de plataforma**: confirmada por inspeção direta da árvore atual (`github.com/openjdk/jdk`, consulta via API do GitHub): três diretórios — `os/<plataforma>` (ex. `os/linux`, `os/bsd`), `cpu/<arquitetura>` (ex. `cpu/x86`, `cpu/zero`), e `os_cpu/<plataforma>_<arquitetura-ou-variante>` (ex. `os_cpu/linux_x86`, `os_cpu/bsd_zero`).
- **A variante Zero já existe e é reaproveitável sem nenhum código novo**: `cpu/zero` (interpretador puro em C++, sem geração de código de máquina) é independente de arquitetura de CPU — usado por `os_cpu/linux_zero`, `os_cpu/bsd_zero`, etc. Isso elimina de vez a parte historicamente mais difícil de um porte novo (escrever um gerador de código/assembler específico de arquitetura) — confirmado por fonte externa: a variante Zero é "tipicamente usada para fazer o bootstrap de plataformas/CPUs novas", exatamente o nosso caso.
- **Tamanho real do conjunto mínimo de arquivos**: `os_cpu/linux_zero`/`os_cpu/bsd_zero` têm, cada um, só ~10 arquivos (`assembler_*_zero.cpp`, `atomicAccess_*_zero.hpp`, `globals_*_zero.hpp`, `javaThread_*_zero.cpp/.hpp`, `orderAccess_*_zero.hpp`, `os_*_zero.cpp/.inline.hpp`, `prefetch_*_zero.inline.hpp`, `vm_version_*_zero.cpp`) — um conjunto administrável. `os/bsd` (~14 arquivos, sem suporte a cgroups) é um template menor que `os/linux` (~29 arquivos, inclui cgroups/hugepages/etc., opcionais).
- **Achado mais importante, que muda o plano original**: `share/runtime/os.hpp` (a interface real que todo porte implementa) tem 1187 linhas e já inclui `utilities/globalDefinitions.hpp`, `utilities/exceptions.hpp`, `utilities/ostream.hpp`, `runtime/osInfo.hpp`, e referencia `class frame`/`class JvmtiAgent` do resto do código compartilhado. **Não é possível compilar um `os_javaos.cpp` real "isolado"** no sentido de "sem nenhum header do HotSpot" — isso exigiria reescrever uma versão simplificada da interface, o que não validaria nada real sobre integração com o HotSpot de verdade.

Ver proposal.md para motivação; ver spec delta de `os-javaos-skeleton` para o contrato comportamental completo.

## Goals / Non-Goals

**Goals:**
- Arquivos novos em `os/javaos`/`os_cpu/javaos_zero`, com assinaturas de função conformes à interface real do HotSpot.
- Pelo menos um subconjunto desses arquivos comprovadamente compila contra os headers reais e inalterados do HotSpot vendorizado, usando o toolchain já existente do projeto (`scripts/cross/x86_64-elf-g++`).

**Non-Goals:**
- Rodar o `configure`/`make` do próprio OpenJDK — exigiria um JDK de bootstrap e toda a infraestrutura de autoconf/make, fora de escopo aqui (fica para uma sub-change futura, se/quando a estratégia desta change se provar viável).
- Garantir que **todos** os arquivos planejados cheguem a compilar — dado o acoplamento real descoberto nesta pesquisa, alguns podem exigir partes do código compartilhado do HotSpot que ainda não foram examinadas; isso é um resultado aceitável, desde que documentado com precisão (ver spec delta, "Arquivo que não compila é relatado como tal").
- Produzir um `libjvm.so`/executável `java` funcional, nem rodar bytecode — isso é responsabilidade de sub-changes muito posteriores.
- Implementar comportamento real e correto para qualquer função do port além do mínimo para compilar (a maioria dos corpos são stubs/`abort()` deliberados).

## Decisions

### Vendorizar só `src/hotspot`, não o repositório inteiro do OpenJDK

O repositório `openjdk/jdk` completo inclui `src/java.base`, `src/java.desktop`, testes, documentação, etc. — gigabytes desnecessários para o que esta change precisa (só os headers de `src/hotspot/share` e os arquivos de referência de `src/hotspot/os`/`os_cpu`/`cpu`). Um `git clone` com `--filter=blob:none`/sparse-checkout restrito a `src/hotspot` (ou um download pontual via a API/`codeload` do GitHub de arquivos específicos) evita baixar o resto da árvore.

### Tag pinada: `jdk-27-ga`

A tag GA mais recente confirmada no momento desta pesquisa (`jdk-28+*` são builds de desenvolvimento ainda não lançados; `jdk-27-ga` e `jdk-26-ga` são as GAs mais recentes). Qualquer tag GA razoavelmente recente serve igualmente bem para este propósito (só precisamos da estrutura/interface, não de uma versão específica de feature); `jdk-27-ga` escolhida por ser a mais recente confirmada.

### `os/bsd`+`os_cpu/bsd_zero` como referência primária, `os/linux`+`os_cpu/linux_zero` como referência cruzada

`os/bsd` é menor (sem cgroups/hugepages, que são otimizações específicas de Linux sem equivalente no Java OS) — ponto de partida mais limpo. `os/linux`/`os_cpu/linux_zero` servem de segunda referência para qualquer função cujo equivalente em `os/bsd` não deixe claro o contrato esperado. Nenhum dos dois é copiado literalmente — cada arquivo de `os_javaos` é escrito do zero, com a assinatura de função exigida pela interface compartilhada, e um corpo mínimo próprio.

### Compilação direta com `x86_64-elf-g++`, sem o `configure`/`make` do OpenJDK

Mesmo wrapper já usado para `cxx-minimal-runtime`, com `-I`/`-isystem` apontando para os diretórios relevantes da árvore vendorizada (`src/hotspot/share`, `src/hotspot/os/posix` quando reaproveitado, `src/hotspot/os/javaos`, `src/hotspot/os_cpu/javaos_zero`, `src/hotspot/cpu/zero`). Isso evita a necessidade de um JDK de bootstrap ou do sistema de build do OpenJDK nesta etapa — ao custo de não validar que o arquivo se encaixa corretamente no grafo de build real do OpenJDK (make/gmk), só que compila como unidade de tradução C++ contra os headers reais.

### `cpu/zero` reaproveitado sem modificação

Confirmado que `cpu/zero` não depende de nenhuma arquitetura de CPU específica — é reaproveitado tal como está da árvore vendorizada, sem nenhuma cópia ou arquivo novo nosso.

## Risks / Trade-offs

- **[Risco] Pode-se descobrir, só ao tentar compilar, que a maioria dos arquivos exige partes do código compartilhado do HotSpot não examinadas nesta pesquisa (ex.: classes de coleta de lixo, JIT, ou infraestrutura de logging)** → Mitigação: aceito como resultado possível e honesto desta change — tasks.md rastreia arquivo por arquivo, e a spec delta exige que falhas sejam documentadas, não escondidas.
- **[Risco] A tag `jdk-27-ga` pode não ser a ideal para o porte real mais adiante** (ex.: se uma LTS mais estável for preferível para o porte de verdade) → Mitigação: aceito por ora — esta change só precisa da estrutura/interface, trocar a versão vendorizada mais adiante (se necessário) não invalida o aprendizado sobre a forma do porte.
- **[Risco] Vendorizar `src/hotspot` sozinho (sem o resto da árvore) pode faltar algum header referenciado que vive fora dele** (ex.: algo de `src/java.base/share/native`) → Mitigação: se isso acontecer, documentar e vendorizar o diretório adicional pontualmente — não é esperado que a interface `os::`/`os_cpu` dependa de nada fora de `src/hotspot`.

## Migration Plan

Puramente aditivo — nenhum código existente muda. Nenhum passo de migração necessário.

## Fontes consultadas

- Documentação de build do OpenJDK (`doc/building.md` do repositório `openjdk/jdk`, via mirrors) — requisito de boot JDK, autoconf 2.69+, GNU Make 3.81+.
- Árvore de código-fonte real do `openjdk/jdk` no GitHub, consultada via API (listagem de diretórios de `src/hotspot/os`, `os_cpu`, `cpu/zero`) e leitura direta de `share/runtime/os.hpp`.
- Discussões de lista de e-mail do OpenJDK (`hotspot-dev`, 2007-2008) sobre a estrutura geral de um porte novo — tratadas como contexto histórico, não como lista autoritativa de arquivos (reconfirmada contra a árvore atual).
- Lista de e-mail `jdk-dev` (novembro 2024) sobre o status do projeto Zero — "efetivamente obsoleto" como projeto isolado, mas integrado e buildável dentro do OpenJDK mainline via `--with-jvm-variants=zero`.

Os nomes exatos de função/assinatura de cada arquivo devem ser reconfirmados contra o código-fonte vendorizado no momento da implementação (a pesquisa desta change consultou a árvore via GitHub, mas a implementação trabalha sobre a cópia vendorizada local).

## Resultado real da implementação (relato honesto — task 5.1)

Ao contrário do previsto no risco "não é esperado que a interface `os::`/`os_cpu` dependa de nada fora de `src/hotspot`" (acima), a verificação por compilação real revelou que ela depende, sim, de um punhado de arquivos de `src/java.base` — documentado abaixo. Todos os 16 arquivos novos desta change **compilam** (`-fsyntax-only`) contra a árvore real vendorizada. Nenhum arquivo planejado ficou sem verificar.

### Invocação de compilação real usada

Não foi possível usar o wrapper `scripts/cross/x86_64-elf-g++` (feito para programas de userland `-ffreestanding -nostdlib`) — o HotSpot é uma aplicação C++ hosted de verdade. Usado `g++` do host (WSL2/Ubuntu) diretamente:

```
g++ -fsyntax-only -std=gnu++17 -D_ALLBSD_SOURCE -D_LP64=1 -DTARGET_COMPILER_gcc \
  -DINCLUDE_ZGC=0 -DINCLUDE_JFR=0 -DZERO -DCC_INTERP \
  -I share -I share/include -I os_cpu/javaos_zero -I os/javaos -I os/javaos/include \
  -I os/posix -I cpu/zero \
  -I ../java.base/share/native/include -I ../java.base/unix/native/include \
  -DINCLUDE_SUFFIX_OS=_javaos -DINCLUDE_SUFFIX_CPU=_zero -DINCLUDE_SUFFIX_COMPILER=_gcc \
  -DTARGET_ARCH_zero \
  <arquivo>
```

Toda macro `-D` acima corresponde a uma decisão real de build (as mesmas que o `configure` do OpenJDK passaria para esta combinação SO+variante), não a um artifício para forçar a compilação — ver tasks.md, seção 3, para a justificativa de cada uma.

### Tabela de compilação por arquivo

| Arquivo | Compila? | Observação |
|---|---|---|
| `os_cpu/javaos_zero/globals_javaos_zero.hpp` | Sim | — |
| `os_cpu/javaos_zero/atomicAccess_javaos_zero.hpp` | Sim | — |
| `os_cpu/javaos_zero/orderAccess_javaos_zero.hpp` | Sim | — |
| `os_cpu/javaos_zero/prefetch_javaos_zero.inline.hpp` | Sim | — |
| `os_cpu/javaos_zero/vm_version_javaos_zero.cpp` | Sim | Vazio (igual à referência) |
| `os_cpu/javaos_zero/assembler_javaos_zero.cpp` | Sim | Vazio (igual à referência) |
| `os_cpu/javaos_zero/javaThread_javaos_zero.hpp` | Sim | Fragmento incluído dentro de `JavaThread` |
| `os_cpu/javaos_zero/javaThread_javaos_zero.cpp` | Sim | — |
| `os_cpu/javaos_zero/os_javaos_zero.cpp` | Sim | Sem suporte a sinais POSIX (ver abaixo) |
| `os_cpu/javaos_zero/os_javaos_zero.inline.hpp` | Sim | Vazio (igual à referência) |
| `os/javaos/globals_javaos.hpp` | Sim | — |
| `os/javaos/mutex_javaos.hpp` | Sim | Sem `.cpp` — corpos ficam em `os_javaos.cpp` (não escritos; `PlatformMutex`/`PlatformMonitor` não são exercidos por nenhum teste desta change) |
| `os/javaos/park_javaos.hpp` | Sim | Idem — corpos de `PlatformEvent`/`PlatformParker` não escritos |
| `os/javaos/semaphore_javaos.hpp` | Sim | Classe própria (newlib não tem `<semaphore.h>`) |
| `os/javaos/semaphore_javaos.cpp` | Sim | Corpos stub explícitos (`fatal()`) |
| `os/javaos/osThread_javaos.hpp` | Sim | Reduzido ao mínimo de `OSThreadBase`; sem `.cpp` |
| `os/javaos/os_javaos.hpp` | Sim | Não declara `class os::Javaos` (ver achado abaixo) |
| `os/javaos/os_javaos.cpp` | Sim | 18 `pd_*` definidos: 14 stub (`fatal()`), 4 com corpo real trivial e seguro (`pd_disclaim_memory`, `pd_pretouch_memory`, `get_summary_cpu_info`/`get_summary_os_info`, `initialize_initial_active_processor_count`, `pd_print_cpu_info`) |
| `os/javaos/os_javaos.inline.hpp` | Sim | 6 métodos inline exigidos por `os.hpp`, valores conservadores próprios (não delega para `os_posix.inline.hpp`) |
| `os/javaos/include/jvm_md.h` | Sim (incluído por outros, não testado isoladamente) | Sem `<sys/socket.h>` (sem rede) |

**Não escrito nesta change** (mencionado em tasks.md mas não exercido por nenhum caminho de compilação real encontrado): corpos de `PlatformMutex`/`PlatformMonitor` (`mutex_javaos.hpp`) e `PlatformEvent`/`PlatformParker` (`park_javaos.hpp`). Ambos exigiriam funções `pthread_mutex_*`/`pthread_cond_*` que a libc (newlib) declara mas ainda não implementa de verdade (sem wrapper ligado às syscalls do kernel em `userland_c/glue.c`) — implementá-los de verdade é trabalho de uma change futura, quando algum caminho de execução precisar de sincronização real.

### Achados reais que exigiram desviar do plano original (cada um documentado no arquivo relevante)

1. **`classfile_constants.h` não é código-fonte committed** — gerado pelo `configure`/`make` real a partir de `src/java.base/share/native/include/classfile_constants.h.template`. Resolvido ampliando o sparse-checkout para incluir esse diretório (3 arquivos) e substituindo os dois placeholders (`@@VERSION_CLASSFILE_MAJOR@@`=71, `@@VERSION_CLASSFILE_MINOR@@`=0 — valores reais e públicos da especificação de class file para `jdk-27-ga`, confirmados por busca externa).
2. **`jni_md.h` vive em `src/java.base/<os>/native/include/`, não em `src/hotspot`** — resolvido vendorizando `src/java.base/unix/native/include/` (1 arquivo), consistente com a superfície POSIX-like do Java OS.
3. **O HotSpot não é `-ffreestanding`** — é uma aplicação hosted; o wrapper de cross-compilação existente (feito para `userland_c/`) não serve para compilar contra os headers reais do HotSpot.
4. **Macros de build que o `configure` real passaria, mas que não são óbvias de antemão**: `_ALLBSD_SOURCE` (família de SO), `TARGET_COMPILER_gcc`, `ZERO`/`CC_INTERP` (desliga o interpretador template, inexistente na variante Zero), `INCLUDE_ZGC=0` (confirmado: `cpu/zero` nunca teve `gc/z`), `INCLUDE_JFR=0` (evita `jfrfiles/jfrEventIds.hpp`, gerado a partir de metadados JFR por uma ferramenta do build, fora de escopo).
5. **`libffi-dev`**: dependência real e documentada da variante Zero (`cpu/zero/globalDefinitions_zero.hpp` inclui `<ffi.h>`) — instalada no WSL2 (pacote padrão de sistema).
6. **`os/bsd/os_bsd.hpp`/`os/linux/os_linux.hpp` não reimplementam a interface `os::` inteira** — só adicionam uma classe aninhada auxiliar (`os::Bsd`/`os::Linux`); os métodos `pd_*` já são membros de `os::` (declarada em `share/runtime/os.hpp`), com corpo definido em `os_<os>.cpp`.
7. **`share/runtime/os.hpp` (compartilhado) só prevê `os::<OS>` para AIX/BSD/LINUX/_WINDOWS** (cadeia fixa de `#if/#elif`, sem ramo `else`) — um porte novo de verdade precisaria editar esse arquivo compartilhado para ganhar uma `os::Javaos`. Como nada nesta change precisa dela, `os_javaos.hpp` não a declara, evitando a edição por ora (documentado no próprio arquivo para não ser uma surpresa numa change futura).
8. **`share/runtime/javaThread.hpp` exige, transitivamente, `mutex_javaos.hpp`, `park_javaos.hpp`, `semaphore_javaos.hpp` e `osThread_javaos.hpp`** (todos da task 4) antes mesmo de `javaThread_javaos_zero.cpp` (task 3.5) compilar — os grupos de tasks 3 e 4 não são tão independentes quanto o plano original assumia; a ordem real de implementação intercalou os dois grupos.
9. **newlib vendorizado tem `<dirent.h>`/`<sys/param.h>`/`<pthread.h>`** (ao contrário do suposto na task 2.2) — só `<semaphore.h>` realmente falta.

### Próximo passo real

Implementar de verdade (não mais stub) os `pd_*` de memória (`pd_reserve_memory`/`pd_commit_memory`/etc., hoje `fatal()`) sobre as syscalls `SYSCALL_VM_RESERVE`/`COMMIT`/`DECOMMIT`/`PROTECT`/`RELEASE` que o kernel já expõe (`kernel/src/syscall.rs`) — ou vendorizar/instalar um JDK de bootstrap e tentar rodar o `configure` real do OpenJDK apontando para este porte, para descobrir o próximo nível de dependências reais (ex. geração de `jfrEventIds.hpp`, que esta change contornou com `INCLUDE_JFR=0`, mas que um build de produção real precisaria resolver de verdade).
