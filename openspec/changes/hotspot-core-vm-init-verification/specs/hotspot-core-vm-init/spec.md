# Spec Delta

## Purpose

Confirma, por compilação real contra a árvore vendorizada do HotSpot, que o modelo de objetos (`classfile`/`oops`), o interpretador real do caminho Zero e a criação da `VMThread` — as três próximas fatias exigidas por `Threads::create_vm()` após a cadeia de headers gerados já fechada — compilam sem erro.

## ADDED Requirements

### Requirement: Modelo de Objetos (`classfile`/`oops`) Compila Contra o HotSpot Vendorizado
Cada arquivo `.cpp` de `share/classfile` e de `share/oops` SHALL compilar
sem erro via o mesmo processo de verificação (`g++ -fsyntax-only`) já
usado para verificar outras partes do HotSpot vendorizado neste projeto.

#### Scenario: Todos os arquivos `.cpp` de `classfile`/`oops` compilam sem erro
- **WHEN** cada um dos arquivos `.cpp` de `share/classfile` e de
  `share/oops` é verificado por compilação real contra a árvore
  vendorizada
- **THEN** nenhum SHALL produzir erro de compilação

### Requirement: Interpretador Real do Caminho Zero Compila Contra o HotSpot Vendorizado
Os arquivos do interpretador C++ real do caminho Zero (`cpu/zero/*Interpreter*`
e demais arquivos de suporte do interpretador em `cpu/zero`, mais
`share/interpreter/*.cpp` relevante ao caminho Zero) SHALL compilar sem
erro via o mesmo processo de verificação já usado para outras partes do
HotSpot vendorizado. Arquivos do interpretador de templates SHALL ser
excluídos dessa verificação (ver cenário abaixo).

#### Scenario: Arquivos do interpretador real do caminho Zero compilam sem erro
- **WHEN** cada arquivo do interpretador C++ real do caminho Zero é
  verificado por compilação real contra a árvore vendorizada
- **THEN** nenhum SHALL produzir erro de compilação

#### Scenario: Arquivos do interpretador de templates são reconhecidos como fora do port Zero
- **WHEN** alguém verifica se `share/interpreter/templateInterpreter.cpp`,
  `templateInterpreterGenerator.cpp` ou `templateTable.cpp` fazem parte
  do port Zero
- **THEN** a resposta SHALL apontar para o guard `#ifndef ZERO` no header
  correspondente como confirmação de que esses arquivos nunca devem ser
  compilados nem contados como bloqueio no port Zero

### Requirement: Criação da `VMThread` Compila Contra o HotSpot Vendorizado
`share/runtime/vmThread.cpp` — o passo que vem logo após `init_globals2()`
em `Threads::create_vm()` — SHALL compilar sem erro via o mesmo processo
de verificação já usado para outras partes do HotSpot vendorizado, e
SHALL também compilar com sucesso como objeto real (`-c`), não só como
verificação sintática.

#### Scenario: `vmThread.cpp` compila sem erro como verificação sintática e como objeto real
- **WHEN** `share/runtime/vmThread.cpp` é verificado por compilação real
  contra a árvore vendorizada, tanto via `-fsyntax-only` quanto via `-c`
  (gerando um objeto real)
- **THEN** nenhuma das duas formas SHALL produzir erro de compilação
