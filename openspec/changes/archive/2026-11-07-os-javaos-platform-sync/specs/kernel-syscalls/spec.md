# Spec Delta

## ADDED Requirements

### Requirement: Chamada de Tentativa de Aquisição Não-Bloqueante de Mutex Nomeado
O kernel SHALL fornecer uma chamada de sistema que tenta adquirir o mutex
identificado pelo `handle` informado sem bloquear a thread chamadora, devolvendo
sucesso imediato se o mutex estava livre, ou o sentinela de erro imediatamente se já
estava tomado — sem nunca bloquear a thread chamadora, ao contrário da chamada de
aquisição bloqueante já existente.

#### Scenario: Tentativa sobre mutex livre adquire e retorna imediatamente
- **WHEN** um processo em modo usuário invoca a chamada com o `handle` de um mutex
  livre
- **THEN** o kernel marca o mutex como tomado pela thread chamadora e a chamada
  retorna com sucesso imediatamente

#### Scenario: Tentativa sobre mutex já tomado falha sem bloquear
- **WHEN** um processo em modo usuário invoca a chamada com o `handle` de um mutex já
  tomado (pela própria thread chamadora ou por outra)
- **THEN** o kernel devolve o sentinela de erro imediatamente, sem bloquear a thread
  chamadora e sem alterar o estado do mutex

#### Scenario: Handle desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com um `handle` que não
  corresponde a nenhum mutex criado
- **THEN** o kernel devolve o sentinela de erro, sem bloquear a thread chamadora
