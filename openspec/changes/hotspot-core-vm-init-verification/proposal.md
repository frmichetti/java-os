# Proposal

## Why

O roadmap item 4 ("classfile/oops, ~83+125 arquivos") estava em aberto desde
`jvmti-generated-headers`, listado como uma possível cascata grande de
bloqueios de compilação ainda não investigada. Com a cadeia de headers
gerados/vendorizados fechada (`jvmti-generated-headers`/
`jimage-header-vendoring`/`jfr-generated-headers`), é hora de confirmar — por
compilação real, não suposição — se essa cascata temida de fato existe.
Investigação real revelou que não: `share/classfile`/`share/oops` (80
arquivos) já compilam limpo, assim como o interpretador C++ real do caminho
Zero e `share/runtime/vmThread.cpp` — o próximo passo real da sequência
`Threads::create_vm()` logo após `init_globals2()` (criação da `VMThread`).
Isso fecha por completo o item 4 do roadmap e estende a mesma verificação de
`Threads::create_vm()` um passo além de onde `jvmti-generated-headers`/
`jfr-generated-headers` haviam chegado.

## What Changes

- Verificar por compilação real (`g++ -fsyntax-only -std=gnu++17`, mesma
  invocação de host já estabelecida desde `os-javaos-skeleton`) que todos os
  37 arquivos `share/classfile/*.cpp` e todos os 43 arquivos
  `share/oops/*.cpp` (80 no total) compilam limpo contra a árvore vendorizada
  — fecha o item 4 do ROADMAP.md por completo.
- Verificar que o interpretador C++ real do caminho Zero compila limpo:
  `cpu/zero/zeroInterpreter_zero.cpp`, `bytecodeInterpreter_zero.cpp`,
  `abstractInterpreter_zero.cpp`, `interpreterRT_zero.cpp`, `stack_zero.cpp`,
  `frame_zero.cpp`, `stubGenerator_zero.cpp`, `vtableStubs_zero.cpp`,
  `sharedRuntime_zero.cpp`, e todo `share/interpreter/*.cpp` relevante ao
  caminho Zero.
- Documentar o achado real de que `share/interpreter/templateInterpreter.cpp`,
  `templateInterpreterGenerator.cpp` e `templateTable.cpp` **não** fazem
  parte do port Zero — a classe inteira de cada um é envolvida por
  `#ifndef ZERO`/`#endif // !ZERO` no próprio header, confirmado por leitura
  direta do código vendorizado. Nunca devem ser compilados nem contados como
  bloqueio no port Zero.
- Verificar que `share/runtime/vmThread.cpp` — o passo que vem logo após
  `init_globals2()` em `Threads::create_vm()` (criação da `VMThread` via
  `VMThread::create()`/`os::create_thread`, infraestrutura já implementada
  por `pthread-create-thread`/`os-javaos-attach-thread`) — compila limpo não
  só via `-fsyntax-only`, mas como objeto real (`-c`, gerando um `.o` de
  verdade) — primeira vez que um arquivo do HotSpot vendorizado é compilado
  para objeto real neste projeto, não só syntax-checado.
- Atualizar `ROADMAP.md`: fechar o item 4 por completo e registrar o ponto
  exato onde `Threads::create_vm()` foi mapeado até agora (criação da
  `VMThread`).

**Fora do escopo desta change**: qualquer integração real a
`GCConfig`/`universe_init()`, execução de bytecode de verdade, ou o restante
da sequência de `Threads::create_vm()` após a criação da `VMThread` (fases de
inicialização da JVM, bootstrap do `SystemDictionary` etc. — não mapeado
ainda). Nenhuma mudança de código em `kernel/`, `libc/`, `userland_c/`, ou
`os_javaos` — escopo é só verificação por compilação real da árvore
vendorizada, igual às três changes anteriores da mesma linha.

## Capabilities

### New Capabilities
- `hotspot-core-vm-init`: confirma por compilação real que as três próximas
  fatias do HotSpot vendorizado necessárias para avançar
  `Threads::create_vm()` — modelo de objetos (`classfile`/`oops`),
  interpretador real do caminho Zero, e a criação da `VMThread` — compilam
  contra a árvore vendorizada sem erro.

### Modified Capabilities
(nenhuma)

## Impact

- Nenhum arquivo novo de código — só verificação por compilação real (scripts
  de verificação temporários, removidos após uso, mesma disciplina das três
  changes anteriores).
- `ROADMAP.md`: fecha o item 4 por completo, registra o avanço de
  `Threads::create_vm()` até a criação da `VMThread`.
- Nenhuma mudança em `kernel/`, `userland_c/`, `libc/`, ou em qualquer arquivo
  `.cpp`/`.hpp` do HotSpot em si.
