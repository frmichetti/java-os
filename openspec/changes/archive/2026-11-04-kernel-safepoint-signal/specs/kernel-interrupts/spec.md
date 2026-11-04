# Spec Delta

## MODIFIED Requirements

### Requirement: Tratamento de Exceções de CPU
O kernel SHALL instalar handlers para as exceções de CPU essenciais (page fault, general protection fault, double fault) que reportam diagnóstico pelo canal de saída de boot e então entram no comportamento de halt já definido pela capability `kernel-boot`, em vez de permitir um triple fault — exceto uma falta de página por ausência dentro de uma faixa registrada como ponto de poll de safepoint (`kernel-safepoint`), que é recuperável em vez de fatal.

#### Scenario: Page fault é diagnosticado em vez de causar triple fault
- **WHEN** o kernel acessa um endereço de memória não mapeado ou sem permissão suficiente, gerando uma exceção de page fault, fora de qualquer faixa registrada como ponto de poll de safepoint
- **THEN** o kernel SHALL reportar o vetor da exceção, o código de erro e o endereço que causou a falta, e então entrar em halt, em vez de sofrer um triple fault

#### Scenario: General protection fault é diagnosticado em vez de causar triple fault
- **WHEN** o kernel executa uma operação que viola a proteção do processador (ex.: acesso indevido a um segmento ou instrução privilegiada), gerando uma exceção de general protection fault
- **THEN** o kernel SHALL reportar o vetor da exceção e o código de erro, e então entrar em halt, em vez de sofrer um triple fault
