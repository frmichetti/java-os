# Spec Delta

## MODIFIED Requirements

### Requirement: Tratamento de Exceções de CPU
O kernel SHALL instalar handlers para as exceções de CPU essenciais (page fault,
general protection fault, double fault) que reportam diagnóstico pelo canal de saída
de boot — exceto uma falta de página por ausência dentro de uma faixa registrada
como ponto de poll de safepoint (`kernel-safepoint`), que é recuperável em vez de
fatal. O que acontece depois do diagnóstico depende de onde a exceção (`#PF`/`#GP`)
ocorreu: se em modo kernel, o kernel SHALL entrar no comportamento de halt já
definido pela capability `kernel-boot`, em vez de permitir um triple fault; se em
modo usuário, o kernel SHALL, em vez disso, encerrar permanentemente só a thread
causadora, sem afetar nenhuma outra thread nem o restante do sistema.

#### Scenario: Page fault é diagnosticado em vez de causar triple fault
- **WHEN** o kernel acessa um endereço de memória não mapeado ou sem permissão
  suficiente, gerando uma exceção de page fault, fora de qualquer faixa registrada
  como ponto de poll de safepoint
- **THEN** o kernel SHALL reportar o vetor da exceção, o código de erro e o endereço
  que causou a falta, e então entrar em halt, em vez de sofrer um triple fault

#### Scenario: General protection fault é diagnosticado em vez de causar triple fault
- **WHEN** o kernel executa uma operação que viola a proteção do processador (ex.:
  acesso indevido a um segmento ou instrução privilegiada), gerando uma exceção de
  general protection fault
- **THEN** o kernel SHALL reportar o vetor da exceção e o código de erro, e então
  entrar em halt, em vez de sofrer um triple fault

#### Scenario: Page fault em modo usuário encerra só a thread causadora
- **WHEN** uma thread em modo usuário acessa um endereço de memória não mapeado ou
  sem permissão suficiente, gerando uma exceção de page fault, fora de qualquer
  faixa registrada como ponto de poll de safepoint
- **THEN** o kernel SHALL reportar o vetor da exceção, o código de erro e o endereço
  que causou a falta, encerrar permanentemente a thread causadora, e continuar
  funcionando normalmente — em vez de entrar em halt

#### Scenario: General protection fault em modo usuário encerra só a thread causadora
- **WHEN** uma thread em modo usuário executa uma operação que viola a proteção do
  processador, gerando uma exceção de general protection fault
- **THEN** o kernel SHALL reportar o vetor da exceção e o código de erro, encerrar
  permanentemente a thread causadora, e continuar funcionando normalmente — em vez
  de entrar em halt

#### Scenario: Outras threads continuam normalmente depois de uma falta em modo usuário
- **WHEN** uma thread em modo usuário causa uma exceção de page fault ou de general
  protection fault que resulta no encerramento dessa thread
- **THEN** qualquer outra thread que já estivesse pronta ou em execução SHALL
  continuar sendo escalonada normalmente, sem nenhuma interrupção perceptível
