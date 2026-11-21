# Spec Delta

## MODIFIED Requirements

### Requirement: Chamada de Criação de Thread
O kernel SHALL fornecer uma chamada de sistema que cria uma nova thread de
usuário dentro do processo chamador, a partir de um ponto de entrada, uma
stack de usuário, e um argumento opcional informados pelo chamador,
devolvendo um identificador utilizável para essa thread. O argumento
informado SHALL ser entregue à nova thread, acessível a partir do seu
ponto de entrada, sem exigir nenhum mecanismo adicional de comunicação
entre a thread chamadora e a nova thread.

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

#### Scenario: Argumento informado é entregue à nova thread
- **WHEN** um processo em modo usuário invoca a chamada de criação de
  thread com um argumento diferente de zero
- **THEN** a nova thread, ao começar a executar a partir do ponto de
  entrada fornecido, SHALL ter esse mesmo argumento acessível a partir do
  seu próprio ponto de entrada, sem precisar consultar nenhuma variável
  compartilhada para obtê-lo

#### Scenario: Ausência de argumento preserva o comportamento já existente
- **WHEN** um processo em modo usuário invoca a chamada de criação de
  thread sem informar nenhum argumento (equivalente a informar zero)
- **THEN** o comportamento SHALL ser idêntico ao de antes desta capacidade
  aceitar um argumento — a nova thread começa a executar normalmente a
  partir do ponto de entrada fornecido
