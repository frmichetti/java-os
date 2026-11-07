# cxx-runtime Specification

## Purpose

Fornece a um programa C++ de modo usuário do Java OS o mínimo de runtime de linguagem necessário para compilar e rodar: alocação/liberação de objetos via `new`/`delete` (sobre a libc C já existente) e despacho correto de métodos virtuais — sem exceções, RTTI, nem construtores/destrutores de objetos globais.

## Requirements

### Requirement: Alocação e Liberação de Objetos via `new`/`delete`
Um programa C++ de modo usuário SHALL poder alocar um objeto com `new` e liberá-lo com `delete` (incluindo as formas de array, `new[]`/`delete[]`), com cada alocação bem-sucedida resultando numa região de memória utilizável e distinta, consistente com a alocação dinâmica já disponível via a libc C.

#### Scenario: Objeto alocado com `new` é utilizável e distinto de outras alocações
- **WHEN** um programa C++ aloca um objeto com `new`
- **THEN** a região retornada SHALL ser utilizável pelo objeto (leitura/escrita de seus campos) e SHALL NOT sobrepor nenhuma outra alocação viva, seja via `new` ou via as funções de alocação da libc C

#### Scenario: Memória liberada com `delete` pode ser reaproveitada
- **WHEN** um objeto alocado com `new` é liberado com `delete`
- **THEN** uma alocação subsequente (via `new` ou via a libc C) SHALL poder reaproveitar essa memória

### Requirement: Despacho Correto de Método Virtual
Chamar um método virtual através de um ponteiro ou referência de classe-base SHALL invocar a implementação da classe mais derivada do objeto real, consistente com o modelo de herança e polimorfismo da linguagem C++.

#### Scenario: Chamada virtual através de ponteiro de classe-base invoca a implementação derivada
- **WHEN** um método virtual é chamado através de um ponteiro ou referência declarado como uma classe-base, mas apontando para uma instância de uma classe derivada que sobrescreve esse método
- **THEN** a implementação executada SHALL ser a da classe derivada, não a da classe-base
