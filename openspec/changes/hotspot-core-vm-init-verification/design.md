# Design

## Context

Ver `proposal.md` — "Why" para a motivação. Estado atual confirmado por
leitura de código real e por compilação real (não suposição):

- `share/classfile` tem 37 arquivos `.cpp`, `share/oops` tem 43 — 80 no
  total, correspondendo ao item 4 do ROADMAP.md ("classfile/oops,
  ~83+125 arquivos", estimativa aproximada registrada antes desta
  verificação).
- **O método de verificação já estabelecido é intencionalmente `g++` do
  host, não o cross-compiler do projeto**: o `design.md` arquivado de
  `os-javaos-skeleton` (primeira change de HotSpot deste projeto) já
  documenta explicitamente: "O HotSpot não é `-ffreestanding` — é uma
  aplicação hosted; o wrapper de cross-compilação existente (feito para
  `userland_c/`) não serve para compilar contra os headers reais do
  HotSpot." Confirmado nesta exploração: tentar usar
  `scripts/cross/x86_64-elf-g++` (`-ffreestanding -nostdlib`) contra os
  headers reais do `newlib` vendorizado falha já na cadeia de include
  mais básica (`globalDefinitions_gcc.hpp` → `os.hpp` →
  `os/javaos/include/jvm_md.h`) — `newlib` tem um `#error` explícito em
  `sys/dirent.h` ("`<dirent.h>` not supported" nesta configuração de
  build do newlib) e, para headers que `newlib` não tem (`ucontext.h`,
  `dlfcn.h`), o compilador cai de volta para os headers do `glibc` do
  host, que conflitam em tipos básicos (`__dev_t`/`__uid_t`/etc.) com os
  do `newlib`. Isso **não é um achado novo desta change** — é a mesma
  decisão de escopo já aceita desde `os-javaos-skeleton`: compilar o
  HotSpot vendorizado como uma aplicação hosted contra `g++` do host é
  suficiente para esta fase (confirmar que o código compila como unidade
  de tradução C++ contra os headers reais), sem ainda integrar a uma
  libc hospedeira de verdade nem ao sistema de build real do OpenJDK —
  não-objetivo aceito desde o início, revisitado e reconfirmado aqui.
- O interpretador C++ real do caminho Zero (`cpu/zero/zeroInterpreter_zero.cpp`,
  `bytecodeInterpreter_zero.cpp`, etc., mais `share/interpreter/*.cpp`
  relevante) já compila limpo. `share/interpreter/templateInterpreterGenerator.cpp`
  aparentava falhar numa varredura inicial ingênua ("`TemplateInterpreterGenerator`
  does not name a type") — investigação revelou que
  `share/interpreter/templateInterpreterGenerator.hpp` envolve a classe
  inteira em `#ifndef ZERO`/`#endif // !ZERO` (confirmado por leitura
  direta do header), o mesmo valendo para `templateInterpreter.hpp`
  (`#ifndef ZERO` / `#endif // !ZERO`) e `templateTable.hpp` (`#ifndef ZERO`
  / `#endif /* !ZERO */`). Ou seja, esses três arquivos **nunca deveriam
  ser compilados nem contados como bloqueio** no port Zero — é o
  interpretador de templates (gera código de máquina nativo por
  bytecode), substituído inteiramente pelo `cppInterpreter`/
  `zeroInterpreter` no caminho Zero.
- `share/runtime/threads.cpp::Threads::create_vm()` chama
  `init_globals()`/`init_globals2()` e, logo em seguida, cria a
  `VMThread` (`VMThread::create()` + `os::create_thread(vmthread,
  os::vm_thread)` — confirmado por leitura direta do código, linhas
  595-654). `os::create_thread` já foi implementado por
  `pthread-create-thread`/`os-javaos-attach-thread` — nenhuma
  infraestrutura nova necessária. `share/runtime/vmThread.cpp` compila
  limpo tanto via `-fsyntax-only` quanto via `-c` (objeto real, `.o` de
  156 KB gerado) — confirmado por compilação real, primeira vez que um
  arquivo do HotSpot vendorizado é compilado para objeto real (não só
  syntax-checado) neste projeto.
- Uma varredura exaustiva de toda `share/` (1116 arquivos `.cpp`) foi
  tentada mas abandonada por custo (WSL2 via `/mnt/c` ~25-28s por
  arquivo, ~7-8h estimadas para o total) — não é necessária para o
  escopo desta change. Os ~135 arquivos que ela chegou a cobrir antes de
  ser interrompida confirmaram só achados já esperados (GCs não
  escolhidos G1/Parallel/Shenandoah/Z, compilador JIT C1/C2, CDS — todos
  fora do escopo Epsilon+interpretador) e os dois achados reais tratados
  nesta change (`templateInterpreterGenerator.cpp` — falso positivo — e
  arquivos de runtime JFR, que dependem de extensões não vendorizadas em
  `Method`/`InstanceKlass` — já aceito como não-objetivo por
  `jfr-generated-headers`, nenhuma integração real de JFR prevista).

## Goals / Non-Goals

**Goals:**
- `share/classfile`/`share/oops` (80 arquivos) verificados por
  compilação real — fecha o item 4 do ROADMAP.md por completo.
- Interpretador real do caminho Zero verificado, com a exclusão correta
  do interpretador de templates documentada.
- `share/runtime/vmThread.cpp` verificado, incluindo como objeto real
  (não só sintaxe).
- Reconfirmar que `g++` do host continua sendo o método de verificação
  correto para esta fase (não o cross-compiler do projeto).

**Non-Goals:**
- Qualquer integração real a `GCConfig`/`universe_init()`.
- Mapear o restante de `Threads::create_vm()` após a criação da
  `VMThread` (fases de inicialização da JVM, bootstrap do
  `SystemDictionary`, etc.) — próximo passo de uma change futura.
- Resolver o gap de superfície POSIX entre o HotSpot e o `newlib`
  (`<dirent.h>`, conflitos de tipo com `glibc`) — não é um bloqueio
  desta fase, é um não-objetivo já aceito desde `os-javaos-skeleton`
  para quando o projeto de fato integrar o HotSpot a uma libc
  hospedeira real.
- Varredura exaustiva de toda `share/` — fora de escopo por custo,
  substituída por verificação direcionada das três fatias reais listadas
  acima.

## Decisions

### Reutilizar a mesma invocação de `g++` do host já estabelecida
Mesma invocação usada por `jvmti-generated-headers`/
`jimage-header-vendoring`/`jfr-generated-headers` (`g++ -fsyntax-only
-std=gnu++17` com as mesmas macros `-D`), rodada em
`vendor/jdk-27-ga/src/hotspot`. Nenhuma flag nova necessária — as 80 +
~10 + 1 arquivos desta change não introduzem nenhuma dependência de
build diferente das já resolvidas.

### Excluir explicitamente o interpretador de templates da verificação
Em vez de tratar `templateInterpreter*.cpp`/`templateTable.cpp` como
arquivos "bloqueados" (como uma varredura ingênua indicaria), a spec
delta documenta formalmente que eles são guardados por `#ifndef ZERO` e
portanto nunca fazem parte do port Zero — evita que uma verificação
futura os liste incorretamente como pendência.

### Verificar `vmThread.cpp` também como objeto real (`-c`), não só sintaxe
Primeira vez que este projeto vai além de `-fsyntax-only` para um
arquivo do HotSpot vendorizado. Decisão: usar `-c` (sem `-o` apontando
para dentro do repositório — objeto descartável, não versionado) como
uma verificação adicional, mais rigorosa, mas sem introduzir nenhuma
infraestrutura de build nova (nenhum script novo precisa persistir; a
verificação é feita e documentada, o objeto é descartado).

## Risks / Trade-offs

- **[Risco]** O gap de superfície POSIX entre HotSpot e `newlib`
  (achado durante a exploração que motivou esta change) pode voltar a
  ser relevante quando o projeto precisar de fato linkar o HotSpot
  contra uma libc hospedeira. → **Mitigação**: documentado aqui e será
  revisitado nessa ocasião futura — não bloqueia esta change nem
  nenhuma das três anteriores, todas escopadas como "compila como
  unidade de tradução", nunca "linka contra uma libc real".
- **[Trade-off]** Nenhuma verificação de link real (além do teste pontual
  de `vmThread.cpp` como objeto) — erros que só aparecem em tempo de
  link (símbolos indefinidos, duplicados) continuam não cobertos por
  esta linha de changes. Aceitável — mesmo escopo já aceito desde
  `os-javaos-skeleton`.

## Migration Plan

Não aplicável — change de verificação pura, nenhum código de produção
muda.

## Open Questions

Nenhuma — todas as decisões técnicas necessárias para esta change foram
resolvidas acima.
