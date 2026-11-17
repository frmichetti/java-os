# Spec Delta

## ADDED Requirements

### Requirement: Thread Adicional de Usuário Compartilhando Processo Existente
O kernel SHALL permitir criar uma nova thread de usuário dentro de um
processo de usuário já existente, compartilhando o mesmo espaço de
endereçamento virtual (a mesma memória mapeada) desse processo, com sua
própria stack de usuário, sua própria stack de kernel, e seu próprio
contexto de registradores, independentes dos de qualquer outra thread do
mesmo processo.

#### Scenario: Thread adicional enxerga a mesma memória do processo
- **WHEN** uma nova thread é criada dentro de um processo de usuário já
  existente
- **THEN** essa thread SHALL ter acesso de leitura e escrita a qualquer
  endereço virtual já mapeado nesse processo, exatamente como a thread que a
  criou

#### Scenario: Thread adicional começa a executar a partir do ponto de entrada fornecido
- **WHEN** uma nova thread é criada dentro de um processo de usuário já
  existente, com um ponto de entrada e uma stack de usuário fornecidos
- **THEN** essa thread, quando escalonada pela primeira vez, SHALL começar a
  executar em modo usuário a partir desse ponto de entrada, usando essa
  stack de usuário

#### Scenario: Thread adicional tem stack e registradores independentes da que a criou
- **WHEN** uma nova thread é criada dentro de um processo de usuário já
  existente
- **THEN** alterar a stack de usuário, a stack de kernel, ou o contexto de
  registradores salvo de uma dessas duas threads SHALL NOT afetar a da
  outra

## MODIFIED Requirements

### Requirement: Liberação da Memória Física do Espaço de Endereçamento ao Encerrar
Quando a última thread viva de um processo termina (voluntariamente ou por
exceção de CPU em modo usuário) e é permanentemente excluída da rotação do
escalonador, o kernel SHALL devolver ao alocador de frames físicos todo
frame que o espaço de endereçamento desse processo possuía — cada página de
usuário mapeada e cada tabela de página intermediária — tornando-os
disponíveis para alocação futura. Enquanto o processo ainda tiver outra
thread viva, o término de uma thread SHALL NOT liberar nenhum frame do
espaço de endereçamento compartilhado.

#### Scenario: Frames de uma thread terminada voltam a ficar disponíveis
- **WHEN** a última thread viva de um processo com espaço de endereçamento
  próprio termina e é excluída da rotação do escalonador
- **THEN** todo frame físico que só esse espaço de endereçamento usava
  SHALL voltar a estar disponível para uma alocação futura, em vez de
  permanecer reservado para sempre

#### Scenario: Liberação não afeta a metade superior compartilhada
- **WHEN** o espaço de endereçamento de um processo cuja última thread
  terminou é liberado
- **THEN** nenhum frame da metade superior do endereçamento (compartilhada
  com o próprio kernel e com qualquer outro processo) SHALL ser afetado

#### Scenario: Liberação não afeta o espaço de endereçamento de outra thread ainda viva
- **WHEN** a última thread viva de um processo termina e seu espaço de
  endereçamento é liberado, enquanto outra thread com seu próprio espaço de
  endereçamento (de outro processo) continua em execução
- **THEN** nenhum frame pertencente ao espaço de endereçamento da thread que
  continua viva SHALL ser afetado

#### Scenario: Thread que termina não é a última do seu processo não libera nada
- **WHEN** uma thread termina, mas seu processo ainda tem pelo menos outra
  thread viva
- **THEN** nenhum frame do espaço de endereçamento desse processo SHALL ser
  devolvido ao alocador de frames físicos, e a thread que continua viva
  SHALL continuar acessando normalmente toda a memória que já tinha mapeada
