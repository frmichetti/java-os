# Design

## Context

Ver `proposal.md` — "Why" para a motivação. Esta change resolve um
achado registrado pela change `jvmti-generated-headers` (arquivada):
`jimage.hpp` nunca foi vendorizado de `src/java.base`, bloqueando
`classLoader.hpp`/`universe.cpp`/`threads.cpp`.

Estado atual confirmado por leitura de código real e por compilação real
(não suposição):

- `share/classfile/classLoader.hpp:28` inclui `#include "jimage.hpp"`;
  `class JImageFile;` é só forward-declarado ali (linha 44) — o header em
  si não precisa da definição completa da classe, só do ponteiro opaco.
- `share/classfile/classLoader.cpp` usa exatamente 4 `typedef`s de
  ponteiro de função de `jimage.hpp` (`JImageOpen_t`/`JImageClose_t`/
  `JImageFindResource_t`/`JImageGetResource_t`) e o tipo
  `JImageLocationRef` — confirmado por `grep` no arquivo real.
- `ClassLoader::load_jimage_library()` (`classLoader.cpp:903-931`, lido
  diretamente) carrega as 4 funções (`JIMAGE_Open`/`JIMAGE_Close`/
  `JIMAGE_FindResource`/`JIMAGE_GetResource`) em runtime via
  `os::lookup_function` (build estaticamente linkado) ou
  `os::dll_load`/`dll_lookup` (build dinâmico) — **nunca** por vinculação
  em tempo de compilação/link contra uma implementação de `libjimage`.
  Confirma que vendorizar só o header basta.
- Conteúdo real de `jimage.hpp` (buscado de
  `https://raw.githubusercontent.com/openjdk/jdk/jdk-27-ga/src/java.base/share/native/libjimage/jimage.hpp`,
  o mesmo caminho e tag do repositório oficial): ~170 linhas, só
  `#include "jni.h"` como dependência (já vendorizado em
  `share/native/include`) — declara `JImageFile` (opaca), `JImageLocationRef`,
  as 4 funções `extern "C"` reais (`JIMAGE_Open`/etc.) e seus 4
  `typedef`s de ponteiro, mais macros de erro (`JIMAGE_NOT_FOUND`/
  `JIMAGE_BAD_MAGIC`/etc.) e `JIMAGE_MAX_PATH`.
- **Verificado numa cópia de teste fora do repositório** (não commitada):
  com esse conteúdo presente numa pasta acessível via `-I`,
  `share/classfile/classLoader.cpp` e `share/memory/universe.cpp`
  compilam via `g++ -fsyntax-only -std=gnu++17` (mesma invocação de
  `os-javaos-skeleton`/`jvmti-generated-headers`) com **zero erros** em
  ambos. `share/runtime/threads.cpp` continua bloqueado, mas só pelo
  achado de JFR já documentado (`jfrfiles/jfrEventClasses.hpp`) — nenhum
  erro novo relacionado a `jimage`.

## Goals / Non-Goals

**Goals:**
- `jimage.hpp` real vendorizado em
  `vendor/jdk-27-ga/src/java.base/share/native/libjimage/jimage.hpp`.
- `classLoader.cpp`/`universe.cpp` verificados por compilação real com a
  nova entrada de `-I`.

**Non-Goals:**
- Qualquer implementação nativa de `libjimage` (`.cpp`) — nunca
  necessária em tempo de compilação, só em runtime (ver "Context").
- Resolver `share/runtime/threads.cpp` — continua bloqueado por
  `jfrfiles/jfrEventClasses.hpp` (achado de JFR, não relacionado,
  já documentado em `jvmti-generated-headers`).
- Qualquer outro arquivo do roadmap item 4 (`classfile`/`oops`) além dos
  dois já verificados (`classLoader.cpp`/`universe.cpp`) — essa
  verificação cobre só o que já compila hoje com este único arquivo a
  mais; outros arquivos de `classfile`/`oops` podem ter suas próprias
  dependências não investigadas, fora de escopo aqui.
- Integrar `universe_init()`/`vm_init_globals()`/`init_globals()` a
  qualquer build real — esta change só adiciona um arquivo vendorizado,
  não altera nenhum ponto de integração.

## Decisions

### Vendorizar só o header, sem buscar nem vendorizar `libjimage.cpp`
Já confirmado (ver "Context"): o HotSpot carrega as 4 funções via
`os::lookup_function`/`dll_load` em runtime, nunca linka contra uma
implementação real em tempo de compilação. Buscar a implementação nativa
de `libjimage` (um subsistema próprio, com sua própria árvore de
dependências de parsing de imagem de módulos) seria trabalho não
incidental a "fazer `classLoader.cpp`/`universe.cpp` compilarem" — mesmo
racional já usado em `os-javaos-skeleton` para não buscar implementações
inteiras quando só a assinatura/declaração é exigida por um ponto de
compilação específico.

### Caminho de vendorização espelha exatamente o repositório real
`vendor/jdk-27-ga/src/java.base/share/native/libjimage/jimage.hpp` —
mesmo caminho relativo do repositório oficial
(`src/java.base/share/native/libjimage/jimage.hpp`), consistente com como
`share/native/include`/`unix/native/include` já foram vendorizados
(espelhando a árvore real, não um layout inventado por este projeto).
Exige uma nova entrada de `-I` no comando de verificação
(`-I ../java.base/share/native/libjimage`), já que o `#include
"jimage.hpp"` em `classLoader.hpp` é uma inclusão entre aspas resolvida
pela busca em `-I`, não um caminho relativo (`classLoader.hpp` não vive
no mesmo diretório de `jimage.hpp`).

## Risks / Trade-offs

- **[Risco]** Vendorizar um arquivo de `src/java.base` por vez, sob
  demanda, pode exigir repetir esse processo para outros headers
  transitivos ainda não descobertos, à medida que mais arquivos de
  `classfile`/`oops` forem verificados no futuro. → **Mitigação
  aceita**: mesmo padrão já aceito desde `os-javaos-skeleton`
  (`jni_md.h`/`classfile_constants.h`) — vendorização pontual sob
  demanda, não uma importação antecipada de toda `src/java.base`.
- **[Trade-off]** O arquivo vendorizado não é versionado no Git (mesma
  convenção do resto de `vendor/jdk-27-ga`) — qualquer ambiente novo
  precisa buscá-lo de novo. Aceitável, mesma categoria de dependência já
  aceita para o resto da árvore vendorizada.

## Migration Plan

Não aplicável — aditivo (um novo arquivo vendorizado, uma nova entrada de
`-I` no comando de verificação). Nenhum código/arquivo existente muda de
comportamento.

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.

## Resultado real da implementação

Implementado exatamente conforme planejado — os resultados de
compilação já haviam sido confirmados durante a própria exploração que
motivou esta change (numa cópia de teste fora do repositório), e se
repetiram de forma idêntica ao vendorizar o arquivo de verdade no local
final. Achados reais:

- **Conteúdo idêntico, nenhuma surpresa**: o arquivo baixado diretamente
  para `vendor/jdk-27-ga/src/java.base/share/native/libjimage/jimage.hpp`
  tem exatamente 173 linhas, igual à cópia de teste usada durante a
  exploração — confirma que o download é determinístico (mesmo
  commit/tag, mesmo conteúdo).
- **Verificação de compilação idêntica à da exploração**:
  `share/classfile/classLoader.cpp` e `share/memory/universe.cpp`
  compilam sem erro (`g++ -fsyntax-only -std=gnu++17`, com a nova
  entrada `-I ../java.base/share/native/libjimage`);
  `share/runtime/threads.cpp` continua bloqueado exatamente pelo achado
  de JFR já documentado (`jfrfiles/jfrEventClasses.hpp`), nenhum erro
  novo relacionado a `jimage`.
- Nenhuma mudança necessária em `kernel/`, `userland_c/`, ou em
  qualquer arquivo `.cpp`/`.hpp` do HotSpot — escopo permaneceu
  inteiramente dentro de um único arquivo novo vendorizado.
