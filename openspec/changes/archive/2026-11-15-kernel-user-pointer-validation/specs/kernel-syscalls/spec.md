# Spec Delta

## ADDED Requirements

### Requirement: Ponteiro de Usuário Inválido em Argumento de Syscall é Rejeitado
Toda chamada de sistema que recebe, como argumento, um ponteiro de modo
usuário a ser lido ou escrito pelo kernel SHALL validar essa memória antes
de acessá-la, rejeitando com um sentinela de erro distinto se a validação
falhar, sem nunca acessar a memória apontada.

#### Scenario: Ponteiro válido continua funcionando sem nenhuma mudança observável
- **WHEN** um processo em modo usuário invoca qualquer uma dessas chamadas
  com um ponteiro e tamanho que apontam inteiramente para memória sua,
  mapeada e com a permissão necessária (leitura ou escrita, conforme a
  chamada)
- **THEN** a chamada SHALL ter sucesso, exatamente como se nenhuma validação
  de ponteiro existisse

#### Scenario: Ponteiro apontando para memória não mapeada é rejeitado sem acessá-la
- **WHEN** um processo em modo usuário invoca uma dessas chamadas com um
  ponteiro cujo intervalo pedido toca pelo menos uma página não mapeada no
  seu próprio espaço de endereçamento
- **THEN** a chamada SHALL ser rejeitada com o sentinela de erro distinto,
  sem acessar nenhum byte dessa memória e sem causar nenhuma falta de
  página

#### Scenario: Ponteiro apontando para memória de kernel é rejeitado
- **WHEN** um processo em modo usuário invoca uma dessas chamadas com um
  ponteiro cujo intervalo pedido toca pelo menos uma página mapeada mas não
  acessível a partir do modo usuário
- **THEN** a chamada SHALL ser rejeitada com o sentinela de erro distinto,
  sem acessar nenhum byte dessa memória

#### Scenario: Buffer de escrita apontando para memória só de leitura é rejeitado
- **WHEN** um processo em modo usuário invoca uma chamada que escreve no
  buffer do chamador (a chamada de leitura de arquivo) com um ponteiro cujo
  intervalo pedido toca pelo menos uma página mapeada, acessível a partir do
  modo usuário, mas não gravável
- **THEN** a chamada SHALL ser rejeitada com o sentinela de erro distinto,
  sem escrever nenhum byte nessa memória

#### Scenario: Tamanho zero é sempre válido, independentemente do ponteiro
- **WHEN** um processo em modo usuário invoca uma dessas chamadas com
  tamanho zero, qualquer que seja o valor do ponteiro
- **THEN** a validação SHALL sempre aceitar, já que nenhum byte dessa
  memória chega a ser tocado
