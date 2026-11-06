# Spec Delta

## MODIFIED Requirements

### Requirement: Troca de Contexto Preserva Estado da Thread
Ao trocar de uma thread para outra, o escalonador SHALL salvar o estado de registradores da CPU da thread interrompida — incluindo registradores de uso geral e estado de FPU/SSE/XMM — e restaurar exatamente o estado salvo da thread retomada, de forma que cada thread continue sua execução exatamente de onde parou, inclusive quanto a cálculos de ponto flutuante em andamento.

#### Scenario: Thread retomada continua com seus valores de registradores/stack anteriores
- **WHEN** uma thread é interrompida e, mais tarde, retomada pelo escalonador
- **THEN** seus registradores de uso geral e seu ponteiro de stack SHALL refletir exatamente o estado que tinham no momento em que foi interrompida

#### Scenario: Thread retomada continua com seu estado de ponto flutuante anterior
- **WHEN** uma thread com um cálculo de ponto flutuante em andamento (registradores de FPU/SSE/XMM com valores não-triviais) é interrompida e, mais tarde, retomada pelo escalonador
- **THEN** seus registradores de FPU/SSE/XMM SHALL refletir exatamente o estado que tinham no momento em que foi interrompida, sem nenhuma mistura com o estado de FPU/SSE/XMM de qualquer outra thread que tenha executado nesse intervalo
