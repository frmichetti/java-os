# Spec Delta

## Purpose

Fornece um alocador de heap para o kernel, permitindo o uso de tipos de `alloc` (`Vec`, `String`, `Box`, etc.) em qualquer parte do código do kernel, com a região de heap crescendo sob demanda a partir de memória física disponível.

## ADDED Requirements

### Requirement: Alocação Dinâmica Geral
Após sua inicialização, o heap do kernel SHALL estar disponível como alocador padrão para qualquer tipo de `alloc` usado pelo kernel.

#### Scenario: Alocação de um tipo de `alloc` funciona após a inicialização do heap
- **WHEN** o heap do kernel foi inicializado com sucesso
- **THEN** o kernel consegue criar e usar normalmente valores de tipos como `Vec`, `String` ou `Box`

### Requirement: Crescimento sob Demanda
O heap do kernel SHALL crescer sua região de memória disponível conforme necessário, em vez de depender de um tamanho de heap fixo definido antecipadamente, respeitando o limite de memória física disponível.

#### Scenario: Alocação maior que a região inicial do heap é satisfeita
- **WHEN** uma alocação solicitada excede a quantidade de memória já reservada para o heap, mas há memória física disponível suficiente no sistema
- **THEN** a alocação é satisfeita com sucesso

### Requirement: Falha de Alocação Tratável
Quando não há memória física suficiente para satisfazer uma alocação, o heap do kernel SHALL reportar uma falha de alocação tratável, em vez de causar corrupção de memória ou comportamento indefinido.

#### Scenario: Alocação além da memória física disponível falha de forma controlada
- **WHEN** uma alocação é solicitada e não há memória física livre suficiente para satisfazê-la
- **THEN** o kernel SHALL tratar essa condição como uma falha de alocação explícita, sem acessar memória não reservada nem continuar como se a alocação tivesse sucedido
