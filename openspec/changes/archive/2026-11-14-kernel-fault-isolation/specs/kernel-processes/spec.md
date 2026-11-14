# Spec Delta

## ADDED Requirements

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
