# Proposal

## Why

`share/jfr/jfrEvents.hpp` inclui `jfrfiles/jfrEventClasses.hpp`
**incondicionalmente** (independente da flag `-DINCLUDE_JFR=0` já usada
desde `os-javaos-skeleton`) — achado registrado como bloqueio conhecido
pela change `jvmti-generated-headers` (arquivada). Isso impedia
`share/runtime/thread.cpp`/`javaThread.cpp` (que incluem
`jvmtifiles/jvmtiEnv.hpp`, que por sua vez puxa `jfr/jfrEvents.hpp`) e 2
dos 3 `jvmti*.cpp` ainda bloqueados (`jvmtiEnv.cpp`/
`jvmtiRedefineClasses.cpp`) de serem sequer syntax-checados.

Uma investigação revelou que `jfrfiles/jfrEventClasses.hpp` é, assim como
`jvmti.h`/`jvmtiEnv.hpp`, um arquivo **gerado** — mas desta vez pelo
gerador real `build.tools.jfr.GenerateJfrFiles`
(`make/src/classes/build/tools/jfr/GenerateJfrFiles.java` no repositório
real, **fora** de `src/hotspot` — nunca vendorizado), contra o spec real
`share/jfr/metadata/metadata.xml` (**já vendorizado**, junto com
`metadata.xsd`). Buscado e testado o gerador real numa cópia fora do
repositório (mesma disciplina de `jvmti-generated-headers`): gera 5
headers (`jfrEventClasses.hpp`/`jfrEventControl.hpp`/`jfrEventIds.hpp`/
`jfrPeriodic.hpp`/`jfrTypes.hpp`) a partir do spec já vendorizado, sem
precisar de nenhum arquivo adicional. Com eles presentes, **todos os 26
arquivos ainda bloqueados pelas duas changes anteriores passam a
compilar limpo** — `thread.cpp`/`javaThread.cpp`/`threads.cpp` (os três
arquivos centrais de `share/runtime` envolvidos na criação da VM) e os
21 `share/prims/jvmti*.cpp` por completo.

## What Changes

- Vendorizar `GenerateJfrFiles.java` real (do repositório oficial
  `openjdk/jdk`, tag `jdk-27-ga`,
  `make/src/classes/build/tools/jfr/GenerateJfrFiles.java`) em
  `vendor/jdk-27-ga/make/src/classes/build/tools/jfr/GenerateJfrFiles.java`
  — mesmo caminho relativo do repositório real, mesma convenção pontual
  já usada para `jimage.hpp` (`jimage-header-vendoring`).
- Novo script reproduzível `scripts/jfr/generate-jfr-headers.sh` (mesma
  convenção de `scripts/jvmti/generate-jvmti-headers.sh`): compila
  `GenerateJfrFiles.java` e o roda com `--mode headers` contra
  `share/jfr/metadata/metadata.xml`/`metadata.xsd` (já vendorizados),
  produzindo os 5 headers em
  `vendor/jdk-27-ga/src/hotspot/share/jfr/jfrfiles/` (vendorizado, não
  versionado no Git).
- Verificação por compilação real (`g++ -fsyntax-only`, com a nova
  entrada de `-I`) de **todos os 26 arquivos** antes bloqueados pelas
  duas changes anteriores: os 21 `share/prims/jvmti*.cpp` (agora 21/21,
  não mais 18/21) e `share/runtime/thread.cpp`/`javaThread.cpp`/
  `threads.cpp` — **nenhum erro em nenhum**.
- Atualizar `ROADMAP.md`: registrar que a cadeia inteira de bloqueios de
  compilação identificada por `jvmti-generated-headers`/
  `jimage-header-vendoring` está fechada.

**Fora do escopo desta change**: `MODE: metadata` de `GenerateJfrFiles`
(gera `metadata.bin`, consumido pelo módulo Java `jdk.jfr`, não por
código C++ do HotSpot — não referenciado por nenhum arquivo verificado
nesta linha de changes). Qualquer integração real de JFR
(`-DINCLUDE_JFR=1`, gravação de eventos de verdade) — os arquivos
seguem compilando com `INCLUDE_JFR=0`, JFR continua desligado em
tempo de execução.

## Capabilities

### Modified Capabilities
- `hotspot-generated-sources`: adiciona o requisito de que arquivos
  gerados de JFR também são produzidos pelo gerador real do OpenJDK,
  generalizando o requisito já existente (hoje só menciona JVMTI).

## Impact

- `vendor/jdk-27-ga/make/src/classes/build/tools/jfr/GenerateJfrFiles.java`:
  novo arquivo vendorizado (não versionado no Git).
- `scripts/jfr/generate-jfr-headers.sh`: novo script.
- `vendor/jdk-27-ga/src/hotspot/share/jfr/jfrfiles/`: 5 novos arquivos
  gerados (vendorizados, não versionados no Git).
- `ROADMAP.md`: registra o fechamento da cadeia de bloqueios.
- Nenhuma mudança em `kernel/`, `userland_c/`, ou em qualquer arquivo
  `.cpp`/`.hpp` do HotSpot em si.
