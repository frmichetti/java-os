# Spec Delta

## ADDED Requirements

### Requirement: Criação de Novo Espaço de Endereçamento
O kernel SHALL permitir criar um novo espaço de endereçamento virtual, independente do espaço de endereçamento atualmente ativo, com o mapeamento higher-half do próprio código e dados do kernel já presente nele.

#### Scenario: Novo espaço de endereçamento preserva o acesso ao kernel após a troca
- **WHEN** a CPU troca para um espaço de endereçamento recém-criado
- **THEN** o código e os dados do kernel SHALL continuar acessíveis e executáveis exatamente como antes da troca

#### Scenario: Espaços de endereçamento distintos têm mapeamentos de aplicação independentes
- **WHEN** uma página virtual fora da região do kernel é mapeada em um espaço de endereçamento
- **THEN** essa página SHALL NOT estar automaticamente mapeada em nenhum outro espaço de endereçamento criado separadamente

### Requirement: Operações de Mapeamento sobre um Espaço de Endereçamento Explícito
O kernel SHALL permitir mapear e desmapear páginas virtuais em um espaço de endereçamento especificado explicitamente, não apenas no espaço de endereçamento ativo no momento da chamada.

#### Scenario: Mapear um espaço de endereçamento inativo não afeta o espaço ativo
- **WHEN** uma página virtual é mapeada em um espaço de endereçamento que não é o atualmente ativo
- **THEN** o espaço de endereçamento atualmente ativo SHALL permanecer inalterado por essa operação
