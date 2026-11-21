# Proposal

## Why

`jvmtifiles/jvmti.h`/`jvmtifiles/jvmtiEnv.hpp` são arquivos **gerados** do
HotSpot (a partir de `share/prims/jvmti.xml` via `jvmtiGen.java` +
`jvmtiH.xsl`/`jvmtiHpp.xsl` — todos já vendorizados), nunca executados neste
projeto desde que a árvore `vendor/jdk-27-ga` foi criada. Sem eles,
`jvmtiExport.hpp`/`jvmtiEnv.hpp` (incluídos por quase todo arquivo de
`share/prims/jvmti*.cpp` e por vários arquivos centrais de
`share/runtime` — `thread.cpp`, `threads.cpp`, `javaThread.cpp`) nem
chegam a ser syntax-checados por `g++ -fsyntax-only`, o mesmo padrão de
verificação já usado por toda a linha de changes `os-javaos-*`. O
gerador real (`jvmtiGen.java`) e o spec real (`jvmti.xml`) já estão na
árvore vendorizada — só nunca foram rodados.

Separadamente, o roadmap (item 6.2) ainda não tinha confirmado qual
coletor de lixo usar no primeiro porte. `share/gc/epsilon` (15 arquivos)
já compila limpo contra nossa árvore sem nenhuma mudança — confirmar isso
formalmente, por escrito, evita redescobrir o mesmo fato depois.

## What Changes

- Novo script reproduzível `scripts/jvmti/generate-jvmti-headers.sh`
  (mesma convenção de `scripts/cross/build-newlib.sh`): compila
  `jvmtiGen.java` com `javac` e o roda duas vezes contra `jvmti.xml`
  (uma com `jvmtiH.xsl` → `jvmti.h`, outra com `jvmtiHpp.xsl` →
  `jvmtiEnv.hpp`), escrevendo em
  `vendor/jdk-27-ga/src/hotspot/share/jvmtifiles/` (vendorizado, não
  versionado no Git — mesma convenção de `os_javaos.cpp`).
- Documentação (comentário do script/README) de que isso exige um JDK no
  `PATH` (qualquer versão com `javax.xml.transform`/`javax.xml.parsers`
  padrão — nenhuma API específica de versão) — nova dependência de
  ferramental deste projeto, ausente do ambiente até esta change.
- Verificação por compilação real (`g++ -fsyntax-only`, mesma invocação
  de `os-javaos-skeleton`) dos 21 arquivos `share/prims/jvmti*.cpp`: 18
  compilam limpo agora (incluindo `jvmtiExport.cpp`); os 3 restantes
  continuam bloqueados por **duas famílias completamente distintas** de
  headers gerados, cada uma documentada como achado novo e
  explicitamente fora de escopo (ver "Non-Goals" em design.md):
  `jimage.hpp` (nunca vendorizado de `src/java.base`, bloqueia
  `classLoader.hpp`/`universe.cpp`/`threads.cpp` — roadmap item 4,
  `classfile`/`oops`) e `jfrfiles/jfrEventClasses.hpp` (outra família de
  arquivo gerado, para JFR, incluída incondicionalmente por
  `jfr/jfrEvents.hpp` — bloqueia `thread.cpp`/`javaThread.cpp`,
  subsistema nunca antes mapeado no roadmap).
- Atualização do `ROADMAP.md`: confirma `share/gc/epsilon` como o
  coletor de lixo escolhido para o primeiro porte (decisão registrada,
  nenhum código novo escrito nele — ele já compila como está).

**Fora do escopo desta change**: `jimage.hpp`/`jfrfiles/jfrEventClasses.hpp`
(cada um seu próprio subsistema, não GC); integrar `epsilon` de fato a
qualquer build real ou a `GCConfig`/`universe_init()`; qualquer parte de
`share/runtime/init.cpp` (`vm_init_globals`/`init_globals`/`init_globals2`)
em si — ainda bloqueada por `jimage.hpp`, não resolvido aqui.

## Capabilities

### New Capabilities
- `hotspot-generated-sources`: garante que os arquivos **gerados** do
  HotSpot vendorizado (começando por JVMTI) são produzidos pelo gerador
  real do próprio OpenJDK (`jvmtiGen.java`/XSLT contra `jvmti.xml`), nunca
  por um stand-in simplificado por este projeto — mesmo princípio já
  exigido para código-fonte por `os-javaos-skeleton`, agora também para
  código gerado.
- `hotspot-epsilon-gc`: confirma e registra `share/gc/epsilon` como o
  coletor de lixo escolhido para o primeiro porte da JVM, verificado por
  compilação real contra a árvore vendorizada.

## Impact

- `scripts/jvmti/generate-jvmti-headers.sh`: novo script.
- `vendor/jdk-27-ga/src/hotspot/share/jvmtifiles/jvmti.h`,
  `.../jvmtiEnv.hpp`: novos arquivos gerados (vendorizados, não
  versionados no Git).
- `ROADMAP.md`: registra a geração real de JVMTI e a escolha do coletor
  de lixo.
- Nova dependência de ferramental: um JDK instalado no ambiente de build
  (WSL, usado por toda verificação de compilação real deste projeto).
- Nenhuma mudança em `kernel/`, `userland_c/`, ou em qualquer arquivo
  `.cpp`/`.hpp` do HotSpot em si — escopo inteiramente dentro da árvore
  vendorizada (arquivos gerados) e do ferramental de build.
