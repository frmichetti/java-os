# Tasks

## 1. Verificação de `classfile`/`oops`

- [ ] 1.1 Verificar com `g++ -fsyntax-only -std=gnu++17` (mesma invocação
  já usada por `jvmti-generated-headers`/`jimage-header-vendoring`/
  `jfr-generated-headers`) que todos os 37 arquivos `share/classfile/*.cpp`
  e todos os 43 arquivos `share/oops/*.cpp` (80 no total) compilam sem
  erro.

## 2. Verificação do interpretador real do caminho Zero

- [ ] 2.1 Verificar com a mesma invocação que os arquivos do interpretador
  C++ real do caminho Zero compilam sem erro: `cpu/zero/zeroInterpreter_zero.cpp`,
  `cpu/zero/bytecodeInterpreter_zero.cpp`, `cpu/zero/abstractInterpreter_zero.cpp`,
  `cpu/zero/interpreterRT_zero.cpp`, `cpu/zero/stack_zero.cpp`,
  `cpu/zero/frame_zero.cpp`, `cpu/zero/stubGenerator_zero.cpp`,
  `cpu/zero/vtableStubs_zero.cpp`, `cpu/zero/sharedRuntime_zero.cpp`, e
  todo `share/interpreter/*.cpp` exceto `templateInterpreter.cpp`/
  `templateInterpreterGenerator.cpp`/`templateTable.cpp`.
- [ ] 2.2 Confirmar por leitura direta dos headers correspondentes
  (`share/interpreter/templateInterpreter.hpp`,
  `templateInterpreterGenerator.hpp`, `templateTable.hpp`) que as três
  classes citadas acima são de fato guardadas por `#ifndef ZERO`/
  `#endif // !ZERO` (ou variante equivalente), confirmando que nunca
  fazem parte do port Zero.

## 3. Verificação da criação da `VMThread`

- [ ] 3.1 Verificar com a mesma invocação que `share/runtime/vmThread.cpp`
  compila sem erro via `-fsyntax-only`.
- [ ] 3.2 Verificar adicionalmente que `share/runtime/vmThread.cpp`
  compila com sucesso como objeto real (`g++ -c`, gerando um `.o`
  descartável) — primeira verificação deste tipo nesta linha de changes.

## 4. Documentação

- [ ] 4.1 Atualizar `ROADMAP.md`: fechar o item 4 por completo
  (`classfile`/`oops` confirmados), registrar o interpretador real do
  Zero como verificado, documentar a exclusão do interpretador de
  templates, e registrar o avanço de `Threads::create_vm()` até a
  criação da `VMThread`.
- [ ] 4.2 Registrar em `design.md` desta change (seção "Resultado real
  da implementação") qualquer achado adicional descoberto durante a
  implementação ou verificação.

## Workflow follow-up

- Arquivar esta change (`/opsx:archive hotspot-core-vm-init-verification`)
  depois que o usuário revisar a implementação.
