#!/usr/bin/env bash
# Gera os headers JVMTI reais (jvmti.h/jvmtiEnv.hpp) do HotSpot
# vendorizado - ver change `jvmti-generated-headers`, design.md.
#
# `jvmtifiles/jvmti.h`/`jvmtifiles/jvmtiEnv.hpp` são arquivos GERADOS do
# HotSpot real (nunca código-fonte de `src/hotspot`) - produzidos pelo
# gerador real do próprio OpenJDK (`share/prims/jvmtiGen.java`, uma
# classe Java standalone que só usa `javax.xml.transform`/
# `javax.xml.parsers` padrão) a partir do spec real
# (`share/prims/jvmti.xml`) e das folhas de estilo XSLT reais
# (`jvmtiH.xsl`/`jvmtiHpp.xsl`) - todos já vendorizados, só nunca
# tinham sido rodados neste projeto.
#
# Exige um JDK no PATH (qualquer versão razoavelmente recente - nenhuma
# API específica de versão é usada). `jvmti.xml` tem DTD interna (sem
# referência externa via rede), então a geração funciona offline.
#
# Saída: vendor/jdk-27-ga/src/hotspot/share/jvmtifiles/{jvmti.h,jvmtiEnv.hpp}
# - vendorizada, não versionada no Git (mesma convenção de
# scripts/cross/build-newlib.sh e do restante de vendor/jdk-27-ga).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PRIMS="$ROOT/vendor/jdk-27-ga/src/hotspot/share/prims"
OUT_DIR="$ROOT/vendor/jdk-27-ga/src/hotspot/share/jvmtifiles"
BUILD_DIR="$ROOT/target/jvmtigen"

# Mesma versão da tag vendorizada (jdk-27-ga) - usada só para compor uma
# string de versão no header gerado (nunca para processamento
# condicional do spec, ver design.md, "Context").
MAJOR_VERSION=27

mkdir -p "$OUT_DIR" "$BUILD_DIR"

javac -d "$BUILD_DIR" "$PRIMS/jvmtiGen.java"

java -cp "$BUILD_DIR" jvmtiGen \
  -IN "$PRIMS/jvmti.xml" \
  -XSL "$PRIMS/jvmtiH.xsl" \
  -OUT "$OUT_DIR/jvmti.h" \
  -PARAM majorversion "$MAJOR_VERSION"

java -cp "$BUILD_DIR" jvmtiGen \
  -IN "$PRIMS/jvmti.xml" \
  -XSL "$PRIMS/jvmtiHpp.xsl" \
  -OUT "$OUT_DIR/jvmtiEnv.hpp" \
  -PARAM majorversion "$MAJOR_VERSION"

echo "Gerado em $OUT_DIR:"
ls -la "$OUT_DIR"
