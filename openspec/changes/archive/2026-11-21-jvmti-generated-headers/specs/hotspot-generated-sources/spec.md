# Spec Delta

## Purpose

Garante que os arquivos gerados do HotSpot vendorizado (código produzido por uma ferramenta a partir de um arquivo de especificação, nunca escrito à mão) são produzidos pelo gerador real do próprio OpenJDK, contra o spec real, nunca por um stand-in simplificado deste projeto.

## ADDED Requirements

### Requirement: Arquivos Gerados Vêm do Gerador Real do OpenJDK
Todo arquivo de código gerado do HotSpot vendorizado que este projeto
precisar SHALL ser produzido rodando a ferramenta geradora real do
OpenJDK (vendorizada) contra o arquivo de especificação real
correspondente (também vendorizado), nunca escrito ou simplificado à
mão por este projeto.

#### Scenario: `jvmti.h` é gerado pelo gerador real, não escrito à mão
- **WHEN** o processo de geração de `jvmtifiles/jvmti.h` é executado
- **THEN** o arquivo resultante SHALL ser o produto de rodar `jvmtiGen`
  (a ferramenta geradora vendorizada) com `jvmtiH.xsl` contra
  `jvmti.xml` (ambos vendorizados), nunca um arquivo escrito ou
  adaptado manualmente por este projeto

#### Scenario: `jvmtiEnv.hpp` é gerado pelo gerador real, não escrito à mão
- **WHEN** o processo de geração de `jvmtifiles/jvmtiEnv.hpp` é
  executado
- **THEN** o arquivo resultante SHALL ser o produto de rodar `jvmtiGen`
  com `jvmtiHpp.xsl` contra `jvmti.xml`, nunca um arquivo escrito ou
  adaptado manualmente por este projeto

### Requirement: Geração é Reproduzível
O processo de geração dos arquivos gerados do HotSpot SHALL ser
reproduzível através de um único comando, sem exigir nenhum passo manual
além de ter o ferramental necessário (um JDK) disponível.

#### Scenario: Rodar o script de geração produz os arquivos esperados
- **WHEN** o script de geração de headers JVMTI é executado num
  ambiente com um JDK disponível
- **THEN** `jvmtifiles/jvmti.h` e `jvmtifiles/jvmtiEnv.hpp` SHALL existir
  ao final, prontos para serem incluídos por código C++ que os
  referencia
