# Spec Delta

## ADDED Requirements

### Requirement: Stack de Kernel Confiável Sempre Atualizada para a Thread Corrente
A cada troca de contexto, o escalonador SHALL garantir que a stack de kernel usada pela CPU em qualquer transição de privilégio (interrupção ou chamada de sistema) seja a stack de kernel da thread que passou a ser a corrente.

#### Scenario: Interrupção durante execução em modo usuário usa a stack de kernel correta
- **WHEN** uma thread em modo usuário é interrompida (ex.: pelo timer) ou faz uma chamada de sistema
- **THEN** a CPU SHALL passar a executar sobre a stack de kernel dessa mesma thread, nunca sobre a stack de kernel de outra thread
