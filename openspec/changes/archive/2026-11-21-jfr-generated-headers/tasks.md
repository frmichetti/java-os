# Tasks

## 1. Vendorizar `GenerateJfrFiles.java` real

- [x] 1.1 Buscar o conteúdo real de `GenerateJfrFiles.java` do
  repositório oficial `openjdk/jdk` na tag `jdk-27-ga`
  (`make/src/classes/build/tools/jfr/GenerateJfrFiles.java`) e salvá-lo
  em
  `vendor/jdk-27-ga/make/src/classes/build/tools/jfr/GenerateJfrFiles.java`,
  preservando o conteúdo exatamente como está no repositório real (ver
  design.md, "Context"/"Decisions").

## 2. Script de geração e headers reais

- [x] 2.1 Criar `scripts/jfr/generate-jfr-headers.sh` (mesma convenção
  de `scripts/jvmti/generate-jvmti-headers.sh`): compila
  `GenerateJfrFiles.java` com `javac` e o roda com `--mode headers`
  contra `share/jfr/metadata/metadata.xml`/`metadata.xsd` (já
  vendorizados), `--output` apontando para
  `vendor/jdk-27-ga/src/hotspot/share/jfr/jfrfiles/`. Rodar o script e
  confirmar que os 5 arquivos (`jfrEventClasses.hpp`/
  `jfrEventControl.hpp`/`jfrEventIds.hpp`/`jfrPeriodic.hpp`/
  `jfrTypes.hpp`) existem ao final.

## 3. Verificação por compilação real

- [x] 3.1 Verificar com `g++ -fsyntax-only -std=gnu++17` (mesma
  invocação de `os-javaos-skeleton` e sucessoras, com a nova entrada de
  `-I` apontando para `share/jfr/jfrfiles`) que os 21
  `share/prims/jvmti*.cpp` compilam **todos** sem erro (regressão: os
  18 já confirmados por `jvmti-generated-headers`, mais os 2 que só
  faltavam por este achado — `jvmtiEnv.cpp`/`jvmtiRedefineClasses.cpp`).
- [x] 3.2 Verificar com a mesma invocação que
  `share/runtime/thread.cpp`, `share/runtime/javaThread.cpp` e
  `share/runtime/threads.cpp` compilam sem erro (regressão para
  `threads.cpp`, que dependia tanto deste achado quanto de
  `jimage-header-vendoring` — confirmar que ambos resolvidos juntos
  bastam).

## 4. Documentação

- [x] 4.1 Atualizar `ROADMAP.md`: registrar que a cadeia completa de
  bloqueios de compilação identificada por `jvmti-generated-headers`/
  `jimage-header-vendoring` está fechada — todos os 26 arquivos antes
  bloqueados compilam agora.
- [x] 4.2 Registrar em `design.md` desta change (seção "Resultado real
  da implementação") qualquer achado adicional descoberto durante a
  implementação ou verificação.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive jfr-generated-headers`) depois
  que o usuário revisar a implementação.
