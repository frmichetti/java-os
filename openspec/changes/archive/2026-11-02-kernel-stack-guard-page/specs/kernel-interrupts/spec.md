# Spec Delta

## ADDED Requirements

### Requirement: Page Fault com Stack Dedicada
O handler de page fault SHALL executar em uma stack própria (independente da stack corrente do kernel), para continuar sendo entregue de forma confiável mesmo quando a causa do page fault for um estouro da stack do kernel.

#### Scenario: Page fault causado por estouro de stack ainda é entregue ao handler
- **WHEN** um estouro da stack do kernel provoca uma exceção de page fault
- **THEN** a CPU SHALL conseguir entregar a exceção ao handler de page fault (usando sua própria stack dedicada), que reporta o diagnóstico normalmente, em vez de a falha escalar para double fault por a CPU não conseguir empilhar o frame de exceção na stack corrente (já invadida pela guard page)
