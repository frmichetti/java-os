#!/usr/bin/env bash
# Configura e builda o newlib vendorizado (vendor/newlib-<versao>/) para o
# alvo genérico x86_64-elf — ver openspec/changes/archive/*/libc-minimal-port
# (build original, sem ponto flutuante) e .../libc-float-math (reconstrução
# de libm/conversão float, depois que `kernel-fpu-context-switch` tornou SSE
# seguro em modo usuário e os wrappers de cross-compilação pararam de
# desabilitá-lo).
#
# Achados reais durante a implementação de `libc-float-math`:
# 1. Com SSE habilitado nos wrappers (scripts/cross/x86_64-elf-gcc/g++), a
#    build completa de `libc`+`libm` não produziu nenhum erro de compilação
#    (confirmado por uma build limpa, do zero, isolada de `libgloss`) — ao
#    contrário do histórico anterior (`-mno-sse` fazia `libm` inteira e um
#    punhado de arquivos de conversão float de `libc/stdlib`
#    `atof`/`strtod`/`dtoa`/`strtodg`/`gdtoa-*`/`ldtoa`/`libc/time/
#    difftime.c` falharem). `-k` permanece nesta invocação só por causa do
#    achado 2 abaixo, não por ponto flutuante.
# 2. `make all` (sem `-k`) para com erro **em `libgloss`**, não em
#    `newlib`/`libm` — `libgloss/doc/porting.info` precisa de `makeinfo`
#    (pacote `texinfo`), ausente neste ambiente de build. `libgloss` nunca
#    foi usado por este projeto (o `libc.a` final vem só de `newlib/libc` +
#    `newlib/libm`); `-k` deixa esse alvo não relacionado falhar sem
#    interromper o resto da build.
# 3. A detecção de `NEWLIB_SRC` abaixo usava `-name 'newlib-*'`, que também
#    casava com `vendor/newlib-build`/`vendor/newlib-install` (criados por
#    este próprio script) — inofensivo na primeira execução (antes desses
#    diretórios existirem), mas `sort -V | tail -1` passava a escolher
#    `newlib-install` (que nunca é populado, já que este script nunca roda
#    `make install`) em qualquer execução seguinte que precisasse
#    reconfigurar do zero. Nunca disparou em uso normal porque o `Makefile`
#    já existente pulava o passo de `configure` — só uma build limpa (sem
#    `vendor/newlib-build` antes) expõe o bug. Corrigido exigindo que o
#    nome comece com um dígito após `newlib-` (só a árvore de versão
#    baixada bate com isso).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NEWLIB_SRC="$(find "$ROOT/vendor" -maxdepth 1 -name 'newlib-[0-9]*' -type d | sort -V | tail -1)"
BUILD_DIR="$ROOT/vendor/newlib-build"
INSTALL_DIR="$ROOT/vendor/newlib-install"

if [ -z "$NEWLIB_SRC" ]; then
  echo "Nenhum vendor/newlib-<versao> encontrado — baixe e extraia o newlib primeiro (ver README)." >&2
  exit 1
fi

export PATH="$ROOT/scripts/cross:$PATH"

mkdir -p "$BUILD_DIR" "$INSTALL_DIR"
cd "$BUILD_DIR"

if [ ! -f Makefile ]; then
  "$NEWLIB_SRC/configure" \
    --target=x86_64-elf \
    --prefix="$INSTALL_DIR" \
    --disable-newlib-io-float \
    --disable-newlib-io-long-double \
    --disable-multilib \
    --disable-nls
fi

# `-k`: deixa passar o erro esperado de `libgloss` (achado 2 acima, não
# relacionado a ponto flutuante). Sem `-k`, `make` para nesse alvo antes de
# `newlib/libc`/`newlib/libm` sequer serem tentados em alguns layouts de
# build paralela.
make -k -j"$(nproc)" || true

# `libc.a` arquiva os `.o` de `newlib/libc` E `newlib/libm` juntos — achado
# real: a versão anterior deste script só olhava `newlib/libc`, então
# `libm` (sin/cos/sqrt/etc.) nunca foi incluída no `libc.a` final, mesmo
# quando compilava com sucesso. `mathfp`/`ld128` (variantes alternativas,
# não selecionadas para este alvo) ficam vazios por design do próprio
# `configure` do newlib, não por falha de build.
# Arquivado na raiz de `newlib/` (não dentro de `newlib/libc/`) — é o
# caminho que `xtask/src/main.rs` (`vendored_newlib_src_dir`/
# `newlib_build.join("libc.a")`) espera encontrar.
NEWLIB_BUILD_ROOT="$BUILD_DIR/x86_64-elf/newlib"
OBJLIST="$(mktemp)"
trap 'rm -f "$OBJLIST"' EXIT
find "$NEWLIB_BUILD_ROOT/libc" "$NEWLIB_BUILD_ROOT/libm" -name '*.o' > "$OBJLIST"
LIBC_A="$NEWLIB_BUILD_ROOT/libc.a"
x86_64-elf-ar rc "$LIBC_A" @"$OBJLIST" 2>/dev/null \
  || xargs x86_64-elf-ar rc "$LIBC_A" < "$OBJLIST"
x86_64-elf-ranlib "$LIBC_A"

echo "libc.a construido em $LIBC_A ($(wc -l < "$OBJLIST") objetos arquivados: libc + libm)"
echo "Headers em: $NEWLIB_SRC/newlib/libc/include e $BUILD_DIR/x86_64-elf/newlib/targ-include"
