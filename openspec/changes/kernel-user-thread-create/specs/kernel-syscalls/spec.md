# Spec Delta

## ADDED Requirements

### Requirement: Chamada de Criação de Thread
O kernel SHALL fornecer uma chamada de sistema que cria uma nova thread de
usuário dentro do processo chamador, a partir de um ponto de entrada e uma
stack de usuário informados pelo chamador, devolvendo um identificador
utilizável para essa thread.

#### Scenario: Criação bem-sucedida devolve um identificador de thread utilizável
- **WHEN** um processo em modo usuário invoca a chamada de criação de thread
  com um ponto de entrada e um tamanho de stack válidos
- **THEN** o kernel cria uma nova thread pronta para ser escalonada dentro
  do mesmo processo, e devolve um identificador que a distingue de qualquer
  outra thread já criada

#### Scenario: Thread criada compartilha o espaço de endereçamento do processo chamador
- **WHEN** uma nova thread é criada pela chamada de criação de thread
- **THEN** essa thread SHALL enxergar exatamente a mesma memória mapeada do
  processo chamador, incluindo qualquer memória mapeada antes ou depois da
  criação

#### Scenario: Ponto de entrada inválido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada de criação de
  thread com um ponto de entrada que não corresponde a memória executável
  mapeada e acessível a partir do modo usuário no processo chamador
- **THEN** o kernel devolve o código de erro `EFAULT`, sem criar nenhuma
  thread

#### Scenario: Tamanho de stack inválido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada de criação de
  thread com um tamanho de stack igual a zero, ou maior que o espaço
  virtual livre disponível para stacks de thread
- **THEN** o kernel devolve o código de erro `EINVAL`, sem criar nenhuma
  thread
