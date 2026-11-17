# kernel-processes Specification

## Purpose

Define as abstrações de processo (dono de um espaço de endereçamento isolado) e thread (unidade de escalonamento dentro de um processo, com stack própria protegida por guard page e contexto de registradores), além da criação de novos processos/threads executando código do próprio kernel.

## Requirements

### Requirement: Processo com Espaço de Endereçamento Próprio
O kernel SHALL permitir criar um processo com seu próprio espaço de endereçamento virtual, isolado do espaço de endereçamento de qualquer outro processo.

#### Scenario: Memória mapeada em um processo não é acessível a partir de outro
- **WHEN** um endereço virtual é mapeado dentro do espaço de endereçamento de um processo
- **THEN** esse mesmo endereço virtual, consultado no espaço de endereçamento de outro processo, SHALL NOT estar mapeado para o mesmo frame físico, a menos que explicitamente compartilhado

#### Scenario: Processo criado tem pelo menos uma thread
- **WHEN** um novo processo é criado
- **THEN** ele SHALL ter pelo menos uma thread pronta para ser escalonada

### Requirement: Thread com Stack Própria Protegida por Guard Page
Cada thread SHALL ter sua própria stack de kernel, mapeada com uma guard page imediatamente abaixo, e seu próprio contexto de registradores da CPU, independente de qualquer outra thread.

#### Scenario: Estouro da stack de uma thread não corrompe a stack de outra
- **WHEN** uma thread estoura sua própria stack
- **THEN** o resultado SHALL ser uma falha de página referenciando a guard page dessa thread, em vez de corromper a stack de qualquer outra thread

#### Scenario: Duas threads têm contextos de registradores independentes
- **WHEN** duas threads distintas são criadas
- **THEN** alterar o contexto de registradores salvo de uma SHALL NOT afetar o contexto salvo da outra

### Requirement: Criação de Processos e Threads em Modo Kernel
O kernel SHALL fornecer uma forma de criar um novo processo ou uma nova thread que comece a executar uma função do próprio kernel, sem exigir carregamento de um executável externo nesta capability.

#### Scenario: Nova thread criada executa a função fornecida
- **WHEN** uma nova thread é criada a partir de uma função do kernel
- **THEN** essa thread, quando escalonada pela primeira vez, SHALL começar a executar a partir dessa função

### Requirement: Thread com Stack de Usuário Opcional
Além de sua stack de kernel, uma thread SHALL poder ter uma stack de usuário própria, mapeada como acessível por usuário, para ser usada quando essa thread executa em modo usuário.

#### Scenario: Thread de modo usuário tem stacks de kernel e de usuário distintas
- **WHEN** uma thread é criada para rodar em modo usuário
- **THEN** ela SHALL ter uma stack de kernel (não acessível por usuário) e uma stack de usuário (acessível por usuário) distintas uma da outra

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

### Requirement: Encerramento Involuntário de Thread por Exceção de CPU em Modo Usuário
Uma thread de modo usuário que causa uma exceção de CPU não recuperável (page fault
fora de uma faixa de safepoint, ou general protection fault) SHALL ser encerrada
permanentemente pelo kernel, com a mesma garantia de "nunca mais executa" do
encerramento voluntário — sem exigir nenhuma ação do programa que a causou.

#### Scenario: Thread encerrada por exceção nunca mais executa
- **WHEN** uma thread de modo usuário causa uma exceção de CPU que resulta no seu
  encerramento pelo kernel
- **THEN** nenhuma instrução dessa thread SHALL ser executada novamente a partir
  desse ponto

#### Scenario: Encerramento por exceção não exige cooperação do programa causador
- **WHEN** uma thread de modo usuário causa uma exceção de CPU que resulta no seu
  encerramento
- **THEN** o kernel SHALL encerrá-la sem depender de nenhuma instrução ou chamada de
  sistema adicional executada por essa mesma thread depois da exceção

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
