# libc-float-math Specification

## Purpose

Fornece a um programa C de modo usuário do Java OS aritmética `float`/`double` real,
as funções matemáticas padrão de `libm`, e conversão string↔ponto-flutuante
(`strtod`/`atof`), todas compiladas e executadas de verdade sobre o estado de
FPU/SSE que o kernel já preserva por thread.

## Requirements

### Requirement: Aritmética de Ponto Flutuante Real
Um programa C de modo usuário SHALL poder compilar e executar expressões aritméticas
com `float`/`double` passados e retornados por valor, com resultado numericamente
correto, em vez de falhar a compilação ou produzir um valor corrompido.

#### Scenario: Uma expressão aritmética com double produz o resultado correto
- **WHEN** um programa C executa uma expressão aritmética (soma, multiplicação) com
  operandos `double`
- **THEN** o resultado SHALL corresponder ao valor matematicamente correto da
  expressão, dentro da precisão de ponto flutuante de 64 bits

### Requirement: Funções Matemáticas de `libm`
Um programa C de modo usuário SHALL poder chamar funções matemáticas padrão de
`libm` (incluindo, no mínimo, `sqrt` e uma função trigonométrica) sobre argumentos
`double`, recebendo um resultado numericamente correto dentro de uma tolerância
razoável de arredondamento.

#### Scenario: `sqrt` de um quadrado perfeito devolve a raiz exata
- **WHEN** um programa C chama `sqrt` com o quadrado de um inteiro pequeno (ex.: 16.0)
- **THEN** o resultado SHALL corresponder à raiz quadrada exata (ex.: 4.0), dentro de
  uma tolerância de arredondamento desprezível

#### Scenario: Função trigonométrica em um ponto conhecido devolve o valor esperado
- **WHEN** um programa C chama uma função trigonométrica (ex.: `sin`) num ponto cujo
  resultado é conhecido (ex.: `sin(0.0)`)
- **THEN** o resultado SHALL corresponder ao valor matematicamente esperado, dentro
  de uma tolerância de arredondamento desprezível

### Requirement: Conversão de String para Ponto Flutuante
Um programa C de modo usuário SHALL poder converter uma representação textual de um
número de ponto flutuante para seu valor `double`/`float` através das funções padrão
`strtod`/`atof`, obtendo o valor numericamente correspondente.

#### Scenario: Converter uma string numérica simples devolve o valor correspondente
- **WHEN** um programa C chama `strtod` ou `atof` sobre uma string representando um
  número decimal simples (ex.: `"3.5"`)
- **THEN** o valor `double` devolvido SHALL corresponder numericamente à string
  (dentro da precisão de ponto flutuante), sem exigir nenhuma conversão manual
  adicional pelo chamador

### Requirement: Formatação de Ponto Flutuante em Texto Permanece Fora de Escopo
As funções padrão de formatação de texto da linguagem C (`printf`/`scanf` e
variantes) SHALL continuar sem suporte aos especificadores de ponto flutuante
(`%f`/`%e`/`%g`) — comportamento inalterado por esta capacidade, não uma regressão.

#### Scenario: Especificador de ponto flutuante em printf continua sem efeito garantido
- **WHEN** um programa C usa `%f`/`%e`/`%g` numa chamada de formatação de texto padrão
  da linguagem C
- **THEN** o comportamento SHALL permanecer exatamente o mesmo de antes desta
  capacidade (sem suporte), já que nenhuma mudança nesta capacidade altera a
  formatação de texto
