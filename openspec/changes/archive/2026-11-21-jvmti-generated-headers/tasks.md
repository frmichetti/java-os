# Tasks

## 1. Ferramental e geração real dos headers JVMTI

- [x] 1.1 Confirmar um JDK disponível (`java`/`javac`) no ambiente de
  build (WSL) usado para verificação de compilação real deste projeto —
  instalar se ausente.
- [x] 1.2 Criar `scripts/jvmti/generate-jvmti-headers.sh` (ver
  design.md, "Decisions"): compila `share/prims/jvmtiGen.java` com
  `javac`, roda o gerador duas vezes contra `share/prims/jvmti.xml`
  (`-PARAM majorversion 27`) — uma com `jvmtiH.xsl` → `jvmti.h`, outra
  com `jvmtiHpp.xsl` → `jvmtiEnv.hpp` — escrevendo em
  `vendor/jdk-27-ga/src/hotspot/share/jvmtifiles/`. Rodar o script e
  confirmar que os dois arquivos existem ao final.

## 2. Verificação por compilação real

- [x] 2.1 Verificar com `g++ -fsyntax-only -std=gnu++17` (mesma
  invocação de `os-javaos-skeleton` e sucessoras) os 21 arquivos
  `share/prims/jvmti*.cpp`. Confirmar que 18 compilam sem erro, e que os
  3 restantes (`jvmtiEnv.cpp`, `jvmtiEnvBase.cpp`,
  `jvmtiRedefineClasses.cpp`) falham exatamente pelas duas causas já
  documentadas em design.md (`jimage.hpp`/
  `jfrfiles/jfrEventClasses.hpp`), sem nenhum erro novo ou inesperado.
- [x] 2.2 Verificar com a mesma invocação (`-DINCLUDE_EPSILONGC=1
  -DINCLUDE_G1GC=0 -DINCLUDE_PARALLELGC=0 -DINCLUDE_SERIALGC=0
  -DINCLUDE_SHENANDOAHGC=0`) que os 6 arquivos `.cpp` de
  `share/gc/epsilon` continuam compilando sem erro (confirmação formal
  da decisão, não uma mudança de código).

## 3. Documentação

- [x] 3.1 Atualizar `ROADMAP.md`: registrar a geração real de
  `jvmti.h`/`jvmtiEnv.hpp`, `share/gc/epsilon` como o coletor de lixo
  escolhido, e os dois achados novos (`jimage.hpp` — roadmap item 4;
  `jfrfiles/jfrEventClasses.hpp` — subsistema JFR, nunca antes mapeado)
  como bloqueios conhecidos e documentados para `universe_init()`
  compilar fim a fim.
- [x] 3.2 Registrar em `design.md` desta change (seção "Resultado real
  da implementação") qualquer achado adicional descoberto durante a
  implementação ou verificação.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive jvmti-generated-headers`) depois
  que o usuário revisar a implementação.
