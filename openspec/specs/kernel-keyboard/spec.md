# kernel-keyboard Specification

## Purpose

Fornece ao kernel o primeiro dispositivo de entrada: um driver de teclado PS/2 que traduz a sinalização elétrica de teclas pressionadas e soltas em eventos de tecla com caractere ASCII, para uso por código que precise de entrada interativa.

## Requirements

### Requirement: Tecla Pressionada Gera um Evento com o Caractere Correspondente
Quando uma tecla correspondente a um caractere ASCII imprimível do layout suportado é pressionada, o kernel SHALL gerar um evento de tecla identificando essa tecla como pressionada e contendo o caractere correspondente.

#### Scenario: Pressionar uma tecla de letra gera o caractere correto
- **WHEN** uma tecla de letra do layout suportado é pressionada
- **THEN** o kernel SHALL gerar um evento de tecla pressionada contendo o caractere ASCII correspondente a essa tecla

### Requirement: Tecla Solta é Distinguível de Tecla Pressionada
O kernel SHALL distinguir, em cada evento de tecla gerado, entre uma tecla sendo pressionada e uma tecla sendo solta.

#### Scenario: Soltar uma tecla gera um evento distinto do de pressionar
- **WHEN** uma tecla que havia sido pressionada é solta
- **THEN** o kernel SHALL gerar um evento identificando essa tecla como solta, distinguível do evento gerado quando ela foi pressionada

### Requirement: Eventos de Tecla Ficam Disponíveis para Consulta Posterior
O kernel SHALL manter os eventos de tecla gerados disponíveis para consulta por outro código do kernel, mesmo que a consulta ocorra depois do momento em que o evento foi gerado.

#### Scenario: Evento gerado antes da consulta ainda é obtido
- **WHEN** um evento de tecla é gerado e, em um momento posterior, algum código do kernel consulta por eventos pendentes
- **THEN** esse evento SHALL ser obtido nessa consulta, em vez de ter sido perdido por não ter sido consultado no instante em que foi gerado
