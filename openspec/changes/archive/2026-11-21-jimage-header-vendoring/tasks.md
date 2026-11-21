# Tasks

## 1. Vendorizar `jimage.hpp` real

- [x] 1.1 Buscar o conteúdo real de `jimage.hpp` do repositório oficial
  `openjdk/jdk` na tag `jdk-27-ga`
  (`src/java.base/share/native/libjimage/jimage.hpp`) e salvá-lo em
  `vendor/jdk-27-ga/src/java.base/share/native/libjimage/jimage.hpp`,
  preservando o conteúdo exatamente como está no repositório real (ver
  design.md, "Context"/"Decisions").

## 2. Verificação por compilação real

- [x] 2.1 Verificar com `g++ -fsyntax-only -std=gnu++17` (mesma
  invocação de `os-javaos-skeleton`/`jvmti-generated-headers`, com a
  nova entrada `-I ../java.base/share/native/libjimage` adicionada) que
  `share/classfile/classLoader.cpp` e `share/memory/universe.cpp`
  compilam sem erro. Confirmar que `share/runtime/threads.cpp` continua
  bloqueado só pelo achado de JFR já documentado
  (`jfrfiles/jfrEventClasses.hpp`), sem nenhum erro novo relacionado a
  `jimage`.

## 3. Documentação

- [x] 3.1 Atualizar `ROADMAP.md`: registrar `jimage.hpp` vendorizado e a
  confirmação de que `classLoader.cpp`/`universe.cpp` compilam limpo —
  esclarecendo que isso não abre automaticamente todo o roadmap item 4
  (`classfile`/`oops`), só resolve o bloqueio específico já identificado.
- [x] 3.2 Registrar em `design.md` desta change (seção "Resultado real
  da implementação") qualquer achado adicional descoberto durante a
  implementação ou verificação.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive jimage-header-vendoring`) depois
  que o usuário revisar a implementação.
