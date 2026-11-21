# Design

## Context

Ver `proposal.md` — "Why" para a motivação. Esta change nasceu de uma
exploração (`openspec explore`) do próximo passo depois de
`os-javaos-attach-thread`: mapear `vm_init_globals()`/`init_globals()`/
`init_globals2()` (`share/runtime/init.cpp`) e a escolha de GC. A
exploração revelou que o bloqueio real não era "qual GC" — era um
arquivo gerado ausente (`jvmti.h`) que `universe.cpp` (necessário
**independente** da escolha de GC) também exige.

Estado atual confirmado por compilação real (não suposição):

- `vendor/jdk-27-ga/src/hotspot/share/jvmtifiles/` **não existia** antes
  desta change — `jvmtifiles/jvmti.h`/`jvmtifiles/jvmtiEnv.hpp` nunca
  foram gerados neste projeto.
- O gerador real (`share/prims/jvmtiGen.java`, uma classe Java standalone
  usando só `javax.xml.transform`/`javax.xml.parsers` — nenhuma API
  específica de versão de JDK) e o spec real (`share/prims/jvmti.xml`,
  com DTD **interna** — sem nenhuma referência externa via rede) já
  estavam vendorizados, assim como as duas folhas de estilo XSLT que
  importam `jvmtiLib.xsl` (`jvmtiH.xsl` → `jvmti.h`;
  `jvmtiHpp.xsl` → `jvmtiEnv.hpp`, confirmado pelo nome da classe que
  cada uma emite — `class JvmtiEnv`).
- Único parâmetro XSLT de nível superior exigido por ambas:
  `majorversion` (usado só para compor uma string de versão no header
  gerado, nunca para processamento condicional do spec — seguro passar
  `27`, a mesma versão da tag vendorizada `jdk-27-ga`).
- Nenhum JDK estava instalado no ambiente WSL usado para toda
  verificação de compilação real deste projeto — instalado durante esta
  change (`openjdk-25-jdk-headless` via `apt`, autorizado explicitamente
  pelo usuário; `apt-get install` sozinho falhou por um mirror com 404
  transitório, resolvido com `apt-get update` antes).
- Depois de gerados os dois headers, rodei `g++ -fsyntax-only` (mesma
  invocação de `os-javaos-skeleton` e sucessoras) contra os 21 arquivos
  `share/prims/jvmti*.cpp`: **18 compilam limpo** (`jvmtiAgent`,
  `jvmtiAgentList`, `jvmtiClassFileReconstituter`, `jvmtiCodeBlobEvents`,
  `jvmtiDeferredUpdates`, `jvmtiEnvThreadState`, `jvmtiEventController`,
  `jvmtiExport`, `jvmtiExtensions`, `jvmtiGetLoadedClasses`, `jvmtiImpl`,
  `jvmtiManageCapabilities`, `jvmtiRawMonitor`, `jvmtiTagMap`,
  `jvmtiTagMapTable`, `jvmtiThreadState`, `jvmtiTrace`, `jvmtiUtil`). Os
  3 restantes (`jvmtiEnv.cpp`, `jvmtiEnvBase.cpp`,
  `jvmtiRedefineClasses.cpp`) falham por motivos **não relacionados a
  JVMTI em si** — ver "Non-Goals".
- `share/prims/jvmtiExport.cpp` (um dos 18) é significativo: é incluído
  transitivamente por vários arquivos centrais de `share/runtime`
  (`thread.cpp`/`threads.cpp`/`javaThread.cpp` incluem
  `jvmtifiles/jvmtiEnv.hpp` diretamente).
- `share/gc/epsilon` (15 arquivos — 6 `.cpp`: `epsilonArguments`,
  `epsilonBarrierSet`, `epsilonHeap`, `epsilonInitLogger`,
  `epsilonMemoryPool`, `epsilonMonitoringSupport`) **já compilava limpo
  mesmo antes desta change** — independente de JVMTI, confirmado de novo
  aqui com `-DINCLUDE_EPSILONGC=1 -DINCLUDE_G1GC=0 -DINCLUDE_PARALLELGC=0
  -DINCLUDE_SERIALGC=0 -DINCLUDE_SHENANDOAHGC=0` (mesmo padrão de
  `-DINCLUDE_*=0/1` já usado para `-DINCLUDE_ZGC=0`/`-DINCLUDE_JFR=0`).
  Por comparação, `share/gc/serial` (29 arquivos) tem 2 de 11 `.cpp`
  bloqueados pela mesma família de arquivo gerado que bloqueava
  `jvmtiExport.cpp` antes desta change (`gcVMOperations.hpp` →
  `jvmtiExport.hpp` → `jvmti.h`) — confirma Epsilon como a escolha de
  menor atrito, não só de menor contagem de arquivos.

## Goals / Non-Goals

**Goals:**
- `jvmtifiles/jvmti.h`/`jvmtifiles/jvmtiEnv.hpp` reais, gerados pelo
  gerador vendorizado, via um script reproduzível.
- 18 dos 21 `share/prims/jvmti*.cpp` verificados por compilação real.
- `share/gc/epsilon` confirmado e registrado como o GC escolhido.

**Non-Goals:**
- **`jimage.hpp`** — `share/classfile/classLoader.hpp` inclui
  `jimage.hpp` (a API de leitura do formato de imagem de módulos do
  JDK — `jrt-fs`/`lib/modules`), que nunca foi vendorizado de
  `src/java.base` (só `share/native/include`/`unix/native/include`
  foram, pontualmente, por changes anteriores). Bloqueia
  `jvmtiEnvBase.cpp` (1 dos 3 restantes) e, por extensão,
  `universe.cpp`/`threads.cpp` inteiros — pertence ao roadmap item 4
  (`classfile`/`oops`, ~83+125 arquivos), não a esta change.
- **`jfrfiles/jfrEventClasses.hpp`** — achado novo, nunca antes mapeado
  em nenhum lugar do roadmap: `share/jfr/jfrEvents.hpp` inclui
  `jfrfiles/jfrEventClasses.hpp` **incondicionalmente** (confirmado por
  leitura de código — nenhum `#if INCLUDE_JFR` envolve esse include,
  apesar de `-DINCLUDE_JFR=0` já ser passado em toda verificação deste
  projeto desde `os-javaos-skeleton`) — outra família de arquivo gerado,
  desta vez do subsistema JFR (Java Flight Recorder), com seu próprio
  gerador/spec (não investigado em detalhe). Bloqueia `jvmtiEnv.cpp`/
  `jvmtiRedefineClasses.cpp` (os outros 2 dos 3 restantes) e, por
  extensão, `thread.cpp`/`javaThread.cpp` inteiros. Subsistema
  inteiramente separado de JVMTI/GC — fica para quando for escopado.
- Integrar `epsilon` de fato a `GCConfig`/`Universe::initialize_heap()`
  ou a qualquer build real — esta change só confirma que o código-fonte
  já existente compila, não adiciona nenhum ponto de integração novo.
- Qualquer parte de `vm_init_globals()`/`init_globals()`/`init_globals2()`
  em si — `universe.cpp` continua bloqueado por `jimage.hpp`, não
  resolvido aqui.
- Gerar qualquer outro arquivo de `jvmtifiles/` além de `jvmti.h`/
  `jvmtiEnv.hpp` (ex.: `jvmtiEnter.cpp`/`jvmtiEnv.cpp` gerados —
  `jvmtiEnter.xsl`/`jvmtiEnv.xsl` existem mas nenhum arquivo verificado
  nesta change precisa deles).

## Decisions

### Script dedicado `scripts/jvmti/generate-jvmti-headers.sh`, não integrado a `build-newlib.sh`
Diretório próprio (`scripts/jvmti/`, não `scripts/cross/`) — gera
headers do **HotSpot vendorizado**, nada a ver com a cross-compilação de
`userland_c`/newlib. Mesma disciplina de idempotência/reprodutibilidade
de `build-newlib.sh`: roda `javac`/`java` diretamente (sem exigir Maven/
Gradle/qualquer outro ferramental), escreve em
`vendor/.../share/jvmtifiles/`, documenta no próprio script a dependência
de um JDK no `PATH`.

```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PRIMS="$ROOT/vendor/jdk-27-ga/src/hotspot/share/prims"
OUT_DIR="$ROOT/vendor/jdk-27-ga/src/hotspot/share/jvmtifiles"
BUILD_DIR="$ROOT/target/jvmtigen"

mkdir -p "$OUT_DIR" "$BUILD_DIR"
javac -d "$BUILD_DIR" "$PRIMS/jvmtiGen.java"

java -cp "$BUILD_DIR" jvmtiGen -IN "$PRIMS/jvmti.xml" -XSL "$PRIMS/jvmtiH.xsl" \
  -OUT "$OUT_DIR/jvmti.h" -PARAM majorversion 27
java -cp "$BUILD_DIR" jvmtiGen -IN "$PRIMS/jvmti.xml" -XSL "$PRIMS/jvmtiHpp.xsl" \
  -OUT "$OUT_DIR/jvmtiEnv.hpp" -PARAM majorversion 27
```

### `majorversion = 27`, não uma string arbitrária
Mesma versão da tag vendorizada (`jdk-27-ga`) — consistente mesmo sendo
só cosmético (ver "Context": o parâmetro nunca controla processamento
condicional do spec, só compõe uma string de versão no header final).

### Verificação permanece por arquivo individual, não por `universe_init()` fim a fim
Mesma disciplina já estabelecida por toda a linha `os-javaos-*`: a
"prova" desta change é que arquivos específicos (`jvmti*.cpp`,
`epsilon/*.cpp`) compilam via `-fsyntax-only`, não que qualquer
subsistema inteiro (`universe_init`) funciona fim a fim — isso
continua bloqueado por `jimage.hpp` (Non-Goal), não escondido nem
contornado artificialmente.

## Risks / Trade-offs

- **[Risco]** Os headers gerados (`jvmti.h`/`jvmtiEnv.hpp`) não são
  versionados no Git (vendorizados, mesma convenção de todo o resto de
  `vendor/jdk-27-ga`) — qualquer ambiente novo precisa rodar o script de
  novo. → **Mitigação**: script documentado e reproduzível, mesma
  categoria de dependência já aceita para `scripts/cross/build-newlib.sh`
  (que também produz artefatos vendorizados não versionados).
- **[Risco]** A dependência de um JDK é nova para este projeto (até
  agora só precisava do cross-toolchain C/C++ e do toolchain Rust). →
  **Mitigação**: qualquer JDK razoavelmente recente serve (só
  `javax.xml.transform`/`javax.xml.parsers` padrão, nenhuma API
  específica de versão) — baixo risco de quebrar em ambientes futuros.
- **[Trade-off]** `jimage.hpp`/`jfrfiles/jfrEventClasses.hpp` ficam como
  bloqueios conhecidos, não resolvidos — aceitável: são dois
  subsistemas genuinamente separados (classfile/oops e JFR), cada um
  merecendo sua própria exploração/change, não uma solução apressada
  aqui só para "fechar" `universe.cpp` por inteiro.

## Migration Plan

Não aplicável — aditivo (novos arquivos gerados, novo script). Nenhum
código/arquivo existente muda de comportamento.

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.

## Resultado real da implementação

Implementado exatamente conforme planejado — todos os números (18/21
`jvmti*.cpp`, 6/6 `epsilon/*.cpp`) já haviam sido confirmados durante a
própria exploração que motivou esta change, e se repetiram de forma
idêntica ao reexecutar a partir do script real e versionado (prova de
reprodutibilidade, não só de execução manual pontual). Achados reais:

- **Instalação do JDK via `apt` precisou de `apt-get update` primeiro**:
  a primeira tentativa de `apt-get install -y openjdk-25-jdk-headless`
  falhou com 404 em `security.ubuntu.com` (índice de pacotes
  desatualizado no ambiente WSL) — resolvido rodando `apt-get update`
  antes. Achado secundário sobre o próprio processo: ao encadear os dois
  comandos com `&&` a partir de um prompt do Windows (`wsl -- sudo
  apt-get update && sudo apt-get install ...`), o `&&` foi interpretado
  pelo shell do **Windows**, não pelo WSL — só o primeiro comando rodou
  dentro do WSL; o segundo tentou rodar `sudo` nativo do Windows (que
  está desabilitado nesse ambiente). Corrigido envolvendo a cadeia
  inteira num único `wsl -- bash -lc "..."`.
- **Reprodutibilidade confirmada de verdade**: os dois headers gerados
  pelo script real (`scripts/jvmti/generate-jvmti-headers.sh`) têm
  exatamente o mesmo tamanho em bytes dos gerados manualmente durante a
  exploração (`jvmti.h`: 83241 bytes; `jvmtiEnv.hpp`: 14672 bytes) —
  confirma que o processo é determinístico e que nenhum passo manual
  extra (além de ter um JDK no `PATH`) é necessário.
- **Verificação em dois níveis, nenhuma surpresa em nenhum**:
  - 18/21 `share/prims/jvmti*.cpp` compilam limpo
    (`g++ -fsyntax-only -std=gnu++17`); os 3 restantes
    (`jvmtiEnv.cpp`, `jvmtiRedefineClasses.cpp` →
    `jfrfiles/jfrEventClasses.hpp`; `jvmtiEnvBase.cpp` → `jimage.hpp`)
    falham exatamente pelas duas causas já identificadas na exploração,
    nenhum erro novo.
  - 6/6 `share/gc/epsilon/*.cpp` continuam compilando limpo, confirmação
    formal sem nenhuma mudança de código.
- Nenhuma mudança necessária em `kernel/`, `userland_c/`, ou em qualquer
  arquivo `.cpp`/`.hpp` do HotSpot — escopo permaneceu inteiramente
  dentro de `scripts/jvmti/` (novo) e `vendor/jdk-27-ga/.../jvmtifiles/`
  (gerado, vendorizado).
