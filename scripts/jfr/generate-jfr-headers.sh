#!/usr/bin/env bash
# Gera os headers JFR reais (jfrEventClasses.hpp e os outros 4 do mesmo
# modo de geração) do HotSpot vendorizado - ver change
# `jfr-generated-headers`, design.md.
#
# `jfrfiles/jfrEventClasses.hpp` (incluído incondicionalmente por
# `share/jfr/jfrEvents.hpp`, independente de `-DINCLUDE_JFR`) é um
# arquivo GERADO - produzido pelo gerador real do próprio OpenJDK
# (`build.tools.jfr.GenerateJfrFiles`, vendorizado em
# `vendor/jdk-27-ga/make/src/classes/build/tools/jfr/`, fora de
# `src/hotspot` - primeira vez que este projeto vendoriza algo de
# `make/`, ver design.md) a partir do spec real já vendorizado
# (`share/jfr/metadata/metadata.xml`/`metadata.xsd`).
#
# Exige um JDK no PATH (mesma dependência de
# scripts/jvmti/generate-jvmti-headers.sh - só javax.xml/org.xml.sax
# padrão, nenhuma API específica de versão).
#
# Saída: vendor/jdk-27-ga/src/hotspot/share/jfr/jfrfiles/{jfrEventClasses.hpp,
# jfrEventControl.hpp,jfrEventIds.hpp,jfrPeriodic.hpp,jfrTypes.hpp} -
# vendorizada, não versionada no Git (mesma convenção do resto de
# vendor/jdk-27-ga).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
JFR_TOOL_SRC="$ROOT/vendor/jdk-27-ga/make/src/classes/build/tools/jfr/GenerateJfrFiles.java"
JFR_METADATA_DIR="$ROOT/vendor/jdk-27-ga/src/hotspot/share/jfr/metadata"
OUT_DIR="$ROOT/vendor/jdk-27-ga/src/hotspot/share/jfr/jfrfiles"
BUILD_DIR="$ROOT/target/jfrgen"

mkdir -p "$OUT_DIR" "$BUILD_DIR"

javac -d "$BUILD_DIR" "$JFR_TOOL_SRC"

java -cp "$BUILD_DIR" build.tools.jfr.GenerateJfrFiles \
  --mode headers \
  --xml "$JFR_METADATA_DIR/metadata.xml" \
  --xsd "$JFR_METADATA_DIR/metadata.xsd" \
  --output "$OUT_DIR"

echo "Gerado em $OUT_DIR:"
ls -la "$OUT_DIR"
