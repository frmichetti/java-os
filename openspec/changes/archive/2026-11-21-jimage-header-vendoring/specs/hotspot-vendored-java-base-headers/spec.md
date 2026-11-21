# Spec Delta

## Purpose

Garante que headers de `src/java.base` exigidos transitivamente por código do HotSpot vendorizado são vendorizados pontualmente a partir do repositório real do OpenJDK, preservando seu conteúdo original, nunca reescritos ou simplificados por este projeto.

## ADDED Requirements

### Requirement: Headers de `java.base` São Vendorizados do Repositório Real
Todo header de `src/java.base` que código do HotSpot vendorizado exigir
transitivamente SHALL ser vendorizado a partir do repositório real do
OpenJDK, na mesma tag já usada para o restante da árvore vendorizada,
preservando seu conteúdo original — nunca escrito ou simplificado à mão
por este projeto.

#### Scenario: `jimage.hpp` é vendorizado do repositório real, não escrito à mão
- **WHEN** `share/classfile/classLoader.hpp` (HotSpot vendorizado) precisa
  de `jimage.hpp`
- **THEN** o arquivo presente em `src/java.base/share/native/libjimage/jimage.hpp`
  SHALL ser o conteúdo real do repositório oficial do OpenJDK na mesma tag
  já vendorizada, nunca um arquivo escrito ou adaptado manualmente por
  este projeto

### Requirement: Vendorização Pontual Não Exige Implementação Nativa Associada
Quando o único uso real de uma API de `java.base` pelo HotSpot vendorizado
for através de carregamento em tempo de execução (nunca vinculação em
tempo de compilação), a vendorização SHALL se limitar ao header que
declara essa API, sem exigir a implementação nativa correspondente.

#### Scenario: Vendorizar só `jimage.hpp` basta para a verificação de compilação
- **WHEN** `share/classfile/classLoader.cpp`/`share/memory/universe.cpp`
  são verificados por compilação real depois de `jimage.hpp` ser
  vendorizado
- **THEN** ambos SHALL compilar sem erro, sem exigir nenhuma
  implementação nativa de `libjimage` vendorizada
