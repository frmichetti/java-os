# Design

## Context

Ver `proposal.md` para a motivação. Estado atual relevante:

- `scripts/cross/x86_64-elf-gcc`/`x86_64-elf-g++` (wrappers finos sobre o GCC/G++ do
  host — host e alvo são ambos x86_64, mesmo racional desde `libc-minimal-port`)
  forçam `-mno-sse -mno-sse2 -mno-sse3 -mno-mmx -mno-80387` em toda compilação C/C++
  de modo usuário do projeto. O comentário de `x86_64-elf-g++` já documenta
  explicitamente que isso ficou pendente para esta change ("rastreado separadamente
  em ROADMAP.md").
- `scripts/cross/build-newlib.sh` builda o newlib vendorizado com `make -k`
  (continua apesar de erros) e arquiva manualmente só os `.o` que realmente
  compilaram — hoje isso exclui `libm/` inteira e um punhado de arquivos de
  `libc/stdlib` (`atof.c`/`atoff.c`/`dtoa.c`/`strtod.c`/`strtodg.c`/`gdtoa-*.c`/
  `ldtoa.c`) e `libc/time/difftime.c`, todos descartados do `libc.a` resultante por
  falharem a compilação sob `-mno-sse`.
- `kernel-fpu-context-switch` (arquivada) já resolveu a causa raiz: `FXSAVE`/
  `FXRSTOR` por thread na troca de contexto, `CR0`/`CR4` configurados no boot,
  `x86_64-java_os_user.json` já habilita SSE/SSE2/SSE3/SSSE3/SSE4.x (só MMX/AVX/AVX2
  permanecem desligados — MMX por ser legado sem uso, AVX/AVX2 por exigirem `XSAVE`,
  mais largo que a área `FXSAVE` que o kernel salva). O próprio `design.md` dessa
  change já listou "reconstruir ponto flutuante na build do newlib" como Non-Goal
  explícito, deixado para esta change.
- **Validado empiricamente nesta pesquisa** (compilação direta via `gcc` do host,
  sem os wrappers, contra os headers do newlib vendorizado): removendo todas as
  flags `-mno-sse*`, sem adicionar nenhuma flag `-msse*` explícita,
  `libc/stdlib/atof.c`, `strtod.c`, `dtoa.c`, `libc/time/difftime.c` e
  `libm/math/s_sin.c` (arquivo representativo de `libm`) compilam todos com sucesso.
  O GCC do host em x86_64 já habilita SSE/SSE2 por padrão — são a ABI básica dessa
  arquitetura, só `-mno-sse` as desliga explicitamente.

## Goals / Non-Goals

**Goals:**
- `libm` inteira e as conversões `atof`/`strtod`/`dtoa`/`strtodg`/`gdtoa-*`/`ldtoa`
  compiladas e incluídas no `libc.a` vendorizado.
- Os dois wrappers de cross-compilação (`x86_64-elf-gcc`/`x86_64-elf-g++`) sem
  nenhuma flag `-mno-sse*`/`-mno-mmx`/`-mno-80387`, com os comentários que hoje
  justificam essas flags atualizados para refletir a nova realidade.
- Um programa C real verificado em boot real exercitando aritmética
  `float`/`double`, uma função de `libm`, e `strtod`/`atof`.

**Non-Goals:**
- Suporte a `%f`/`%e`/`%g` em `printf`/`scanf` (`--disable-newlib-io-float`/
  `--disable-newlib-io-long-double` no `configure` do newlib) — ver proposal.md,
  Impact, para o racional de deixar isso de fora.
- `long double` (80 bits, x87 estendido) — `--disable-newlib-io-long-double`
  permanece como está; o `Non-Goal` de `kernel-fpu-context-switch` sobre não cobrir
  nada além da área `FXSAVE` legada (x87/MMX/SSE até XMM) já implicitamente cobre
  `long double` via x87, mas esta change não testa nem garante esse caminho
  especificamente — só `double`/`float` (precisão simples/dupla via SSE).
- `x86_64-java_os_user_pic.json` (target das bibliotecas de teste do linker
  dinâmico) e o target do próprio kernel (`x86_64-java_os.json`) — nenhum dos dois
  muda.
- Reduzir ou eliminar o uso de `make -k` em `build-newlib.sh` por antecipação — a
  tarefa de implementação verifica empiricamente se ainda é necessário (ver
  Decisions) e documenta o resultado real, em vez de presumir.

## Decisions

### Remover as flags em vez de adicionar `-msse`/`-msse2` explícitas

Confirmado pela pesquisa empírica (ver Context): o GCC do host em x86_64 já habilita
SSE/SSE2 por padrão — `-mno-sse*` é que precisa sumir, não uma flag positiva
equivalente que precise ser adicionada. Menor diff, e evita a impressão de que SSE é
uma escolha explícita desta change em vez do comportamento padrão da arquitetura.
`-mno-mmx`/`-mno-80387` também removidas: nenhuma removida isoladamente teve efeito
diferente na pesquisa empírica (testado removendo todas de uma vez), e `kernel::fpu`
já documenta que a área `FXSAVE` legada cobre x87/MMX/SSE até XMM — não há mais
nenhuma razão para desabilitá-los deliberadamente agora que o kernel preserva esse
estado inteiro por thread.

### Re-rodar `build-newlib.sh` sem modificá-lo

O script já descobre os `.o` compilados dinamicamente (`find . -name '*.o'`) antes de
arquivar o `libc.a` — não há nenhuma lista hardcoded de arquivos esperados para
atualizar. Como o `Makefile` do newlib já existe de uma build anterior, rodar o
script de novo não reconfigura nada, só executa `make -k` (que agora vai conseguir
compilar os arquivos antes impossíveis) e re-arquiva tudo que existir. Nenhuma
mudança no próprio script é necessária — só o comentário de topo, que hoje descreve
exatamente a limitação que esta change remove, precisa ser atualizado para não
confundir leitores futuros.

### `make -k` mantido por enquanto; remoção avaliada empiricamente durante a implementação

Não há garantia, só pela pesquisa desta proposta (que testou 5 arquivos
representativos, não as ~1046 unidades de tradução de `libm`+`libc/stdlib`), de que
**toda** a árvore compila sem nenhuma falha residual não relacionada a SSE (ex.:
algo específico de uma plataforma que o newlib genérico `x86_64-elf` não cobre,
análogo aos achados já documentados por `libc-minimal-port` para outras partes da
libc). A tarefa de implementação roda a build completa e relata o resultado real —
se zero arquivos falharem, `-k` deixa de ser necessário e pode sair do script (ou
ficar, como rede de segurança documentada); se algum falhar por razão não
relacionada a SSE, essa falha é documentada explicitamente (mesma prática de
`libc-minimal-port`/`os-javaos-memory-pd`), não escondida.

### Verificação em boot real sem depender de `printf("%f", ...)`

Como a formatação de ponto flutuante em texto continua fora de escopo (ver
Non-Goals), o programa de verificação (`userland_c/libm_test_program.c`) compara
resultados numericamente (ex.: `fabs(resultado - esperado) < epsilon`) e reporta só
`P`/`F` por cenário via `write()` — mesmo padrão já estabelecido por
`libc_test_program.c`/`javaos_vm_test_program.c`/`javaos_pthread_test_program.c`
(nenhum deles jamais dependeu de `printf` com formatação numérica, pela mesma razão
histórica). Nenhuma necessidade de reconsiderar esse Non-Goal só para relatar o
resultado do teste.

## Risks / Trade-offs

- **[Risco] Algum arquivo de `libm`/`libc/stdlib` ainda falha por razão não
  relacionada a SSE, descoberto só ao rodar a build completa** → Mitigação: aceito
  como resultado possível e honesto (ver Decisions, "`make -k` mantido"); documentado
  explicitamente se acontecer, mesma prática já estabelecida neste projeto.
- **[Risco] Habilitar SSE no toolchain C/C++ compartilhado poderia, em teoria, mudar
  o código gerado para os programas de teste já existentes** (`libc_test_program.c`,
  `cxx_test_program.cpp`, etc., nenhum deles usa `float`/`double` hoje) →
  Mitigação: nenhum deles usa ponto flutuante, então a mudança de ABI disponível não
  tem efeito observável sobre eles; a tarefa de verificação em boot real desta
  change também confirma que os programas já existentes continuam passando (mesmos
  cenários `P`/`F` de antes), não só o programa novo.
- **[Risco] `long double`/x87 estendido permanece não testado** — aceito
  deliberadamente (ver Non-Goals); se algum código do HotSpot precisar de `long
  double` de verdade, fica para uma change futura investigar.

## Migration Plan

Puramente aditivo — nenhum programa de usuário existente muda de comportamento
(nenhum usa ponto flutuante hoje); a mudança de toolchain é transparente para quem
não usa `float`/`double`. Nenhum passo de migração de dados ou estado persistente.
Qualquer programa futuro que queira usar ponto flutuante só precisa ser
(re)compilado depois desta change — nenhuma ação adicional.

## Resultado real da implementação (relato honesto — task 4.1)

Tudo planejado foi implementado e verificado, mas um achado real não previsto no
plano original (`task 3.3`) quase impediu a verificação em boot real — resumo
completo abaixo; o relato tarefa-a-tarefa fica em `tasks.md`.

### Achados reais (4, por ordem de descoberta)

1. **`libc.a` nunca incluiu `libm`** — mesmo antes desta change (achado durante a
   task 2.1, não causado por ela): `scripts/cross/build-newlib.sh` só arquivava
   `.o` de `newlib/libc`, nunca de `newlib/libm` (diretórios irmãos na árvore de
   build do newlib). Corrigido arquivando as duas árvores juntas.
2. **Detecção de `NEWLIB_SRC` frágil** (`build-newlib.sh` e
   `xtask::vendored_newlib_src_dir`) — casava com `vendor/newlib-build`/
   `vendor/newlib-install` (criados pelo próprio script, nunca populados),
   escondido até uma build do zero (`Makefile` inexistente) ser tentada. Corrigido
   exigindo um dígito logo após `newlib-` nos dois lugares.
3. **`libm`/conversão float compilam limpas** sob SSE habilitado — nenhum arquivo de
   `newlib/libc`/`newlib/libm` falhou numa build completa do zero; a única falha
   remanescente (`libgloss/doc/porting.info`, por `makeinfo` ausente) é alheia a
   ponto flutuante e a `newlib/libc`/`newlib/libm` (nunca usada por este projeto).
   `-k` mantido na invocação de `make`, agora só por essa razão.
4. **Bug crítico de alinhamento de stack em `userland_c/crt0.S`, não previsto no
   plano original** (ver `tasks.md`, task 3.3, para o relato técnico completo) —
   `_start` herdava um `RSP` ajustado para a convenção de "função chamada" da ABI
   (`% 16 == 8`), correta para os pontos de entrada em assembly deste projeto, mas
   errada para `_start` em si (ponto de entrada do processo, que a ABI exige
   começar alinhado a 16 bytes). Isso deixava `main` com `RSP` desalinhado em 8
   bytes — invisível em todo programa C/C++ anterior (nenhum usava SSE), e causava
   um `#GP` real assim que o primeiro `movaps` do projeto apareceu
   (`libm_test_program`, a primeira função com aritmética de ponto flutuante de
   verdade). Corrigido com `and $-16, %rsp` em `_start`, sem nenhuma mudança no
   kernel.

### Verificação em boot real

`userland_c/libm_test_program.c`, carregado como `/bin/libm.elf`, reportou no
console serial (QEMU, `cargo xtask build-image` + `qemu-system-x86_64 ...
-display none -serial stdio`):

```
libm: /bin/libm.elf carregado e em execucao
libm_test_program: resultado PPPPP (ordem: double_arith,sqrt,sin,strtod,atof)
```

Todos os 5 cenários passaram. **Confirmado sem regressão**: os 6 programas de
teste C/C++ pré-existentes (`libc_test_program`/`cxx_test_program`/
`javaos_vm_test_program`/`javaos_pthread_test_program`/`user_posix_demo`/
`file_syscalls_demo`, mais `fpu_demo_a`/`fpu_demo_b`) reportaram exatamente os
mesmos padrões `P`/`F` de antes, no mesmo log de boot — a mudança de flags do
toolchain compartilhado e a correção de `crt0.S` não afetaram nenhum deles. Nenhum
`EXCECAO`/pânico em todo o log. `cargo test -p kernel --lib` (261 testes) e `cargo
kernel-build` também confirmados sem regressão.
