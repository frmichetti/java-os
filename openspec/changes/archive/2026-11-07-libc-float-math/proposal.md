# Proposal

## Why

`libc-minimal-port` excluiu `libm` inteira e as conversões `float`↔string
(`strtod`/`atof`/`dtoa`) da libc do Java OS porque a ABI x86_64 não compila nenhuma
função com `float`/`double` por valor sob SSE desabilitado (`-mno-sse`, flag então
necessária porque o kernel não salvava/restaurava o estado de FPU/SSE na troca de
contexto). `kernel-fpu-context-switch` já resolveu essa causa raiz — `FXSAVE`/
`FXRSTOR` por thread, `CR0`/`CR4` configurados, e `x86_64-java_os_user.json` (o
target Rust de modo usuário) já habilita SSE desde então — mas, como o próprio
`design.md` dessa change registrou explicitamente em seus Non-Goals, "reconstruir
ponto flutuante na build do newlib... fica para uma sub-change futura". O `ROADMAP.md`
aponta exatamente isso como o próximo passo imediato: sem `libm`/conversão float,
nenhum código do HotSpot que use `double`/`float` (tipos primitivos Java, usados
extensivamente) pode funcionar, mesmo depois dos `pd_*` de memória e do
`PlatformMutex`/`PlatformMonitor` reais já entregues.

## What Changes

- Remover as flags `-mno-sse -mno-sse2 -mno-sse3 -mno-mmx -mno-80387` dos dois
  wrappers de cross-compilação do projeto (`scripts/cross/x86_64-elf-gcc` e
  `x86_64-elf-g++`), atualizando os comentários que hoje justificam essas flags —
  incluindo a nota desatualizada em `x86_64-elf-gcc` que diz "mesmas features do
  target Rust `x86_64-java_os_user.json`": esse target já foi atualizado para
  habilitar SSE por `kernel-fpu-context-switch`, o comentário do wrapper C ficou
  para trás. **Validado empiricamente nesta pesquisa**: o GCC do host em x86_64 já
  habilita SSE/SSE2 por padrão assim que essas flags somem — não é preciso
  adicionar `-msse`/`-msse2` explicitamente.
- Re-rodar `scripts/cross/build-newlib.sh` (idempotente — o `Makefile` já existe, só
  roda `make -k` de novo e re-arquiva os `.o` encontrados) para produzir um `libc.a`
  que agora inclui `libm` inteira e as conversões `atof`/`strtof`/`strtod`/`dtoa` —
  sem precisar reconfigurar nem mudar o script em si, já que ele descobre os `.o`
  dinamicamente.
- Um novo programa de teste C real (`userland_c/libm_test_program.c`, mesmo
  mecanismo ELF das changes anteriores) exercitando aritmética `float`/`double` real,
  uma função de `libm` (`sqrt`/`sin`) e as conversões `strtod`/`atof`, comparando
  resultados com uma tolerância (sem `printf("%f", ...)`, que continua sem suporte —
  ver abaixo) e reportando `P`/`F` por `write()`, verificado em boot real via QEMU.
- **Deliberadamente fora de escopo**: suporte a `%f`/`%e`/`%g` em `printf`/`scanf`
  (controlado por `--disable-newlib-io-float`/`--disable-newlib-io-long-double` no
  `configure` do newlib — flags de `configure`, separadas das de compilação, não
  tocadas aqui). O HotSpot não depende de `printf("%f", ...)` para formatação
  numérica Java (usa sua própria `FloatingDecimal`/`sun.misc`), e expandir para
  `stdio` aumentaria o escopo além do que o `ROADMAP.md` pede ("`libm`/conversão
  float"). Documentado como possível trabalho futuro, não implementado agora.
- **Deliberadamente fora de escopo**: `x86_64-java_os_user_pic.json` (target das
  bibliotecas de teste do linker dinâmico — `libbase.so`/`libuser.so` de
  `kernel-dynamic-linker` — continuam soft-float/sem SSE; não têm nenhuma ligação com
  libc/newlib hoje, são fixtures Rust puras). Nenhuma mudança no target do próprio
  kernel (`x86_64-java_os.json` permanece soft-float — o kernel não usa `libm`).

## Capabilities

### New Capabilities
- `libc-float-math`: aritmética `float`/`double` real, funções matemáticas de `libm`
  (`sin`/`cos`/`sqrt`/`pow`/etc.) e conversão string↔ponto-flutuante
  (`strtod`/`atof`) disponíveis a um programa C de modo usuário do Java OS.

### Modified Capabilities
(nenhuma — `libc-runtime` não muda: seus requisitos cobrem alocação/E-S de
arquivo/console/término de processo, não ponto flutuante; `kernel-fpu-context-switch`
já arquivada não é revista; nenhuma syscall nova, nenhum comportamento de kernel
muda.)

## Impact

- **Código afetado**: `scripts/cross/x86_64-elf-gcc`, `scripts/cross/x86_64-elf-g++`
  (remoção de flags + atualização de comentários), `vendor/newlib-build/` (artefato
  de build re-gerado, não código-fonte), um novo
  `userland_c/libm_test_program.c` e sua integração em `xtask`/`kernel/src/main.rs`
  (mesmo padrão das duas changes anteriores).
- **Afeta todos os builds C/C++ de modo usuário existentes** (`libc_test_program.c`,
  `cxx_test_program.cpp`, `javaos_vm_test_program.c`,
  `javaos_pthread_test_program.c`, `javaos_pthread_glue.o`/`glue.c` em geral):
  todos passam a ser compilados com SSE habilitado a partir desta change, já que o
  wrapper é compartilhado. **Não esperado nenhuma mudança de comportamento** — nenhum
  deles usa `float`/`double` hoje, SSE habilitado só muda a ABI de chamada
  disponível, não o código gerado para esses programas.
- **Sem mudança em**: ABI de syscall do kernel (nenhuma nova, nenhuma alterada),
  `kernel-fpu-context-switch`, qualquer target do kernel ou das bibliotecas de teste
  do linker dinâmico.
- **Dependências**: nenhuma nova — usa apenas a capacidade de FPU/SSE por thread que
  `kernel-fpu-context-switch` já entrega.
