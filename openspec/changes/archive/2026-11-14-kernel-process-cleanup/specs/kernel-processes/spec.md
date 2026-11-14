# Spec Delta

## ADDED Requirements

### Requirement: Liberação da Memória Física do Espaço de Endereçamento ao Encerrar
Quando uma thread com espaço de endereçamento próprio é permanentemente excluída da
rotação do escalonador por ter terminado (voluntariamente ou por exceção de CPU em
modo usuário), o kernel SHALL devolver ao alocador de frames físicos todo frame que
esse espaço de endereçamento possuía — cada página de usuário mapeada e cada tabela
de página intermediária — tornando-os disponíveis para alocação futura.

#### Scenario: Frames de uma thread terminada voltam a ficar disponíveis
- **WHEN** uma thread com espaço de endereçamento próprio termina e é excluída da
  rotação do escalonador
- **THEN** todo frame físico que só esse espaço de endereçamento usava SHALL
  voltar a estar disponível para uma alocação futura, em vez de permanecer
  reservado para sempre

#### Scenario: Liberação não afeta a metade superior compartilhada
- **WHEN** o espaço de endereçamento de uma thread terminada é liberado
- **THEN** nenhum frame da metade superior do endereçamento (compartilhada com o
  próprio kernel e com qualquer outro processo) SHALL ser afetado

#### Scenario: Liberação não afeta o espaço de endereçamento de outra thread ainda viva
- **WHEN** uma thread termina e seu espaço de endereçamento é liberado, enquanto
  outra thread com seu próprio espaço de endereçamento continua em execução
- **THEN** nenhum frame pertencente ao espaço de endereçamento da thread que
  continua viva SHALL ser afetado
