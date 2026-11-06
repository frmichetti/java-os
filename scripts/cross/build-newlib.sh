#!/usr/bin/env bash
# Configura e builda o newlib vendorizado (vendor/newlib-<versao>/) para o
# alvo genérico x86_64-elf, sem ponto flutuante — ver
# openspec/changes/archive/*/libc-minimal-port/design.md (depois de
# arquivada), "Flags de ABI de ponto flutuante... e nenhum ponto flutuante
# na libc desta change".
#
# Achado real durante a implementação: `--disable-newlib-io-float` só
# desliga o suporte a %f/%e/%g em printf/scanf — não impede que `libm`
# inteira, nem um punhado de arquivos de `libc/stdlib` (atof/strtod/dtoa e
# família) e `libc/time/difftime.c`, sejam *tentados* na build, e todos eles
# falham a compilação sob `-mno-sse` (não um erro sutil — ver
# scripts/cross/x86_64-elf-gcc). Como isso impede o alvo `all` padrão do
# `make` de chegar ao passo de arquivar `libc.a` (falta nas suas
# dependências), este script builda com `make -k` (continua apesar de
# erros) e depois arquiva manualmente só os `.o` de `libc/` que realmente
# compilaram — `libm/` inteira e os poucos arquivos de conversão
# float/`difftime.c` de `libc/` simplesmente não entram no `libc.a`
# resultante, consistente com o Non-Goal "sem ponto flutuante" desta libc.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NEWLIB_SRC="$(find "$ROOT/vendor" -maxdepth 1 -name 'newlib-*' -type d | sort -V | tail -1)"
BUILD_DIR="$ROOT/vendor/newlib-build"
INSTALL_DIR="$ROOT/vendor/newlib-install"

if [ -z "$NEWLIB_SRC" ]; then
  echo "Nenhum vendor/newlib-* encontrado — baixe e extraia o newlib primeiro (ver README)." >&2
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

# `-k`: deixa passar os erros esperados (libm, atof/strtod/dtoa/difftime —
# ver cabeçalho deste script). Sem `-k`, `make` para no primeiro erro.
make -k -j"$(nproc)" || true

NEWLIB_BUILD_LIBC="$BUILD_DIR/x86_64-elf/newlib/libc"
cd "$NEWLIB_BUILD_LIBC"
find . -name '*.o' > /tmp/javaos_libc_objs.txt
x86_64-elf-ar rc libc.a @/tmp/javaos_libc_objs.txt 2>/dev/null \
  || xargs x86_64-elf-ar rc libc.a < /tmp/javaos_libc_objs.txt
x86_64-elf-ranlib libc.a

echo "libc.a construido em $NEWLIB_BUILD_LIBC/libc.a ($(find . -name '*.o' | wc -l) objetos arquivados)"
echo "Headers em: $NEWLIB_SRC/newlib/libc/include e $BUILD_DIR/x86_64-elf/newlib/targ-include"
