# Spec Delta

## MODIFIED Requirements

### Requirement: Chamada de Espera em Variável de Condição Nomeada
O kernel SHALL fornecer uma chamada de sistema que libera o mutex nomeado informado e bloqueia a thread chamadora na variável de condição nomeada informada, como uma única operação atômica, reaquirindo o mesmo mutex antes de retornar quando a thread for notificada ou quando um prazo opcional informado pelo chamador expirar.

#### Scenario: Espera bem-sucedida libera o mutex e bloqueia a thread
- **WHEN** um processo em modo usuário invoca a chamada com os `handles` de uma variável de condição e de um mutex que a thread chamadora detém
- **THEN** o kernel libera o mutex, bloqueia a thread chamadora na variável de condição e, ao ser notificada, reaquire o mesmo mutex antes da chamada retornar

#### Scenario: Handle de variável de condição ou de mutex desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com um `handle` de variável de condição ou de mutex que não corresponde a nenhum objeto criado
- **THEN** o kernel devolve o sentinela de erro, sem bloquear a thread chamadora nem alterar o estado do mutex

#### Scenario: Prazo zero preserva a espera indefinida já existente
- **WHEN** um processo em modo usuário invoca a chamada com um prazo de zero (mesmo valor que todo chamador já existente desta chamada sempre passou)
- **THEN** o comportamento SHALL ser exatamente o de uma espera indefinida, idêntico ao de antes desta capacidade existir

#### Scenario: Prazo maior que zero expirando sem notificação retorna indicando expiração
- **WHEN** um processo em modo usuário invoca a chamada com um prazo maior que zero, e nenhuma notificação alcança a thread chamadora antes desse prazo expirar
- **THEN** o kernel SHALL retomar a thread chamadora automaticamente quando o prazo expirar, com o mutex associado reaquirido, e a chamada SHALL retornar um valor que distingue essa expiração de uma notificação real

#### Scenario: Prazo maior que zero notificado antes de expirar retorna indicando notificação
- **WHEN** um processo em modo usuário invoca a chamada com um prazo maior que zero, e é notificado antes desse prazo expirar
- **THEN** a chamada SHALL retornar um valor que indica notificação, distinto do valor usado para expiração de prazo
