# Design

## Context

Ver `proposal.md` — "Why" para a motivação. Esta change resolve o último
achado registrado pela change `jvmti-generated-headers` (arquivada):
`jfrfiles/jfrEventClasses.hpp` nunca foi gerado, bloqueando
`thread.cpp`/`javaThread.cpp`/2 `jvmti*.cpp`.

Estado atual confirmado por leitura de código real e por compilação real
(não suposição):

- `share/jfr/jfrEvents.hpp:32` inclui `#include "jfrfiles/jfrEventClasses.hpp"`
  **sem nenhum guard `#if INCLUDE_JFR`** ao redor — confirmado por
  `grep` no arquivo real, mesmo achado já registrado em
  `jvmti-generated-headers`.
- **O gerador real vive fora de `src/hotspot`**, diferente do gerador de
  JVMTI (`jvmtiGen.java`, em `src/hotspot/share/prims`): é
  `build.tools.jfr.GenerateJfrFiles`
  (`make/src/classes/build/tools/jfr/GenerateJfrFiles.java` no
  repositório real — achado confirmado via `make/ToolsHotspot.gmk`,
  que define `TOOL_JFR_GEN := $(JAVA_SMALL) -cp ... build.tools.jfr.GenerateJfrFiles`,
  e via `make/hotspot/gensrc/GensrcJfr.gmk`, que mostra a invocação real:
  `--mode headers --xml metadata.xml --xsd metadata.xsd --output <dir>`).
  `make/` nunca foi vendorizado neste projeto (só `src/hotspot`/
  `src/java.base` pontual) — precisa ser buscado.
- **O spec de entrada já está vendorizado**: `share/jfr/metadata/metadata.xml`
  (1632 linhas) e `metadata.xsd` já existem em
  `vendor/jdk-27-ga/src/hotspot/share/jfr/metadata/` (vendorizados desde
  que `src/hotspot` foi inicialmente vendorizado) — só o gerador em si
  faltava.
- `GenerateJfrFiles.java` (1053 linhas, buscado do repositório real) é
  autocontido: só usa `java.io`/`java.util`/`javax.xml`/`org.xml.sax`
  padrão — mesma categoria de simplicidade de `jvmtiGen.java`, nenhuma
  outra classe de `build.tools.*` importada.
- `--mode headers` gera 5 arquivos (confirmado pelo comentário do
  próprio cabeçalho do arquivo real: "MODE: headers... OUTPUT FILES: -
  jfrEventIds.hpp... jfrTypes.hpp... jfrPeriodic.hpp...
  jfrEventControl.hpp... jfrEventClasses.hpp"), todos no mesmo diretório
  de saída (`jfrfiles/`, confirmado por `JFR_OUTPUTDIR := .../gensrc/jfrfiles`
  em `GensrcJfr.gmk`) — só `jfrEventClasses.hpp` é referenciado pelos
  arquivos que verificamos, mas os outros 4 são gerados juntos na mesma
  invocação (nenhum motivo para gerar separadamente).
- **Verificado numa cópia de teste fora do repositório** (não
  commitada): compilado e rodado `GenerateJfrFiles` com `--mode headers`
  contra o `metadata.xml`/`metadata.xsd` já vendorizados, produzindo os
  5 arquivos (`jfrEventClasses.hpp` sozinho tem 567 KB — gera uma
  classe C++ para cada tipo de evento JFR definido no spec). Com eles
  presentes (mais uma entrada de `-I`), **todos os 26 arquivos** ainda
  bloqueados por `jvmti-generated-headers`/`jimage-header-vendoring`
  passam a compilar via `g++ -fsyntax-only -std=gnu++17` com **zero
  erros**: os 21 `share/prims/jvmti*.cpp` por completo (antes 18/21) e
  `share/runtime/thread.cpp`/`javaThread.cpp`/`threads.cpp`.

## Goals / Non-Goals

**Goals:**
- `GenerateJfrFiles.java` real vendorizado em
  `vendor/jdk-27-ga/make/src/classes/build/tools/jfr/GenerateJfrFiles.java`.
- Os 5 headers de `--mode headers` gerados em
  `vendor/jdk-27-ga/src/hotspot/share/jfr/jfrfiles/`.
- Todos os 26 arquivos antes bloqueados verificados por compilação real,
  fechando a cadeia completa de achados das duas changes anteriores.

**Non-Goals:**
- `MODE: metadata` de `GenerateJfrFiles` (gera `metadata.bin`, consumido
  pelo módulo Java `jdk.jfr` em tempo de execução — nenhum arquivo C++
  verificado nesta linha de changes o referencia).
- Qualquer integração real de JFR (`-DINCLUDE_JFR=1`, gravação de
  eventos de verdade, `JfrRecorder`/`JfrCheckpointManager` etc.) — os
  arquivos continuam compilando com `INCLUDE_JFR=0`; só os headers
  precisavam existir para o `#include` incondicional resolver.
- Vendorizar o restante de `make/` — só o único arquivo do gerador,
  mesma disciplina pontual de `jimage-header-vendoring`.

## Decisions

### Vendorizar `GenerateJfrFiles.java` espelhando o caminho real de `make/`
`vendor/jdk-27-ga/make/src/classes/build/tools/jfr/GenerateJfrFiles.java`
— mesmo caminho relativo do repositório oficial, mesmo racional de
`jimage-header-vendoring` (espelhar a árvore real, não inventar um
layout). Primeira vez que este projeto vendoriza algo de `make/` (até
agora só `src/hotspot`/`src/java.base`) — documentado explicitamente,
não silenciosamente.

### Script dedicado `scripts/jfr/generate-jfr-headers.sh`
Mesmo padrão de `scripts/jvmti/generate-jvmti-headers.sh`: compila
`GenerateJfrFiles.java` com `javac`, roda com `--mode headers` apontando
`--xml`/`--xsd` para `share/jfr/metadata/` (já vendorizado) e `--output`
para `share/jfr/jfrfiles/` (novo, vendorizado, não versionado no Git).

### Gerar os 5 arquivos de `--mode headers` juntos, não só `jfrEventClasses.hpp`
A ferramenta real sempre gera os 5 juntos numa única invocação — tentar
gerar só um isoladamente exigiria modificar o gerador real (contra o
princípio desta linha de changes: nunca adaptar a ferramenta geradora).
Os outros 4 (`jfrEventIds.hpp`/`jfrTypes.hpp`/`jfrPeriodic.hpp`/
`jfrEventControl.hpp`) não têm custo adicional de manter — podem servir
de base para uma integração real de JFR no futuro, se algum dia for
escopada.

## Risks / Trade-offs

- **[Risco]** Primeira vendorização de algo fora de `src/hotspot`/
  `src/java.base` (`make/`) — pode supor convenções diferentes se mais
  arquivos de `make/` forem necessários no futuro. → **Mitigação**:
  documentado explicitamente nesta change; mesmo racional de vendorização
  pontual já aceito, só estendido a um novo diretório-raiz.
- **[Trade-off]** Os headers gerados não são versionados no Git (mesma
  convenção do resto de `vendor/jdk-27-ga`) — qualquer ambiente novo
  precisa rodar os dois scripts de geração (JVMTI e JFR) de novo.
  Aceitável, mesma categoria de dependência já aceita.

## Migration Plan

Não aplicável — aditivo (um novo arquivo vendorizado, um novo script,
novos arquivos gerados). Nenhum código/arquivo existente muda de
comportamento.

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.

## Resultado real da implementação

Confirmado por execução real (não suposição), dentro do repositório (não
mais numa cópia de teste fora dele):

- `GenerateJfrFiles.java` vendorizado em
  `vendor/jdk-27-ga/make/src/classes/build/tools/jfr/GenerateJfrFiles.java`
  (1053 linhas, idêntico ao buscado na exploração).
- `scripts/jfr/generate-jfr-headers.sh` criado e executado com sucesso
  (JDK usado: OpenJDK 25.0.5.1, `openjdk-25-jdk-headless` já instalado
  via `apt` em change anterior). Gerou os 5 arquivos esperados em
  `vendor/jdk-27-ga/src/hotspot/share/jfr/jfrfiles/`:
  `jfrEventClasses.hpp` (567 KB), `jfrEventControl.hpp` (8.6 KB),
  `jfrEventIds.hpp` (6.4 KB), `jfrPeriodic.hpp` (8.6 KB),
  `jfrTypes.hpp` (3.3 KB).
- Verificação por `g++ -fsyntax-only -std=gnu++17` (mesma invocação já
  usada nas duas changes anteriores, com `-I share/jfr` adicionado —
  resolve o `#include "jfrfiles/jfrEventClasses.hpp"` quotado, já que
  `-I` deve apontar para o diretório **pai** de `jfrfiles/`, não para
  `jfrfiles/` em si):
  - Todos os 21 `share/prims/jvmti*.cpp` compilam sem erro — incluindo
    os 2 que faltavam (`jvmtiEnv.cpp`/`jvmtiRedefineClasses.cpp`),
    confirmando 21/21 (antes 18/21).
  - `share/runtime/thread.cpp`, `share/runtime/javaThread.cpp` e
    `share/runtime/threads.cpp` compilam sem erro.
  - Nenhum erro novo encontrado em nenhum dos 24 arquivos verificados
    nesta change (só os avisos pré-existentes de
    `-Winvalid-offsetof`, já conhecidos de changes anteriores e não
    relacionados a JFR).
- **Nenhum achado adicional além do previsto em "Context"/"Decisions"**:
  a cadeia de 26 arquivos (21 `jvmti*.cpp` + `thread.cpp`/
  `javaThread.cpp`/`threads.cpp`) está agora totalmente fechada só com
  os 3 artefatos vendorizados pelas 3 changes desta linha
  (`jvmti.h`/`jvmtiEnv.hpp`, `jimage.hpp`, os 5 headers JFR) — nenhuma
  cascata adicional de dependências de JFR foi necessária.
- Scripts de verificação temporários (`_verify_jvmti.sh`/
  `_verify_runtime.sh`) foram usados e removidos após a verificação,
  mesma disciplina das changes anteriores.
