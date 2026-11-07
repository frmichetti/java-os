# Spec Delta

## ADDED Requirements

### Requirement: Espera Limitada por Prazo via `pthread_cond_timedwait`
Um programa C de modo usuário SHALL poder esperar em um `pthread_cond_t` com um
prazo absoluto através da função padrão `pthread_cond_timedwait`, retornando quando
notificado ou quando esse prazo for alcançado, o que ocorrer primeiro, com o mutex
associado liberado durante a espera e reaquirido antes de retornar em ambos os casos.

#### Scenario: Esperar com um prazo já no passado expira imediatamente
- **WHEN** um programa C chama `pthread_cond_timedwait` com um prazo absoluto que já
  passou no momento da chamada, sem nenhuma notificação pendente
- **THEN** a função SHALL retornar indicando expiração do prazo, sem bloquear a
  thread chamadora por um tempo perceptível

#### Scenario: Esperar com um prazo futuro sem notificação expira após o prazo decorrer
- **WHEN** um programa C chama `pthread_cond_timedwait` com um prazo absoluto no
  futuro, e nenhuma notificação alcança a thread chamadora antes desse prazo
- **THEN** a função SHALL retornar indicando expiração do prazo somente depois desse
  prazo ter decorrido, com o mutex associado novamente detido pela thread chamadora

#### Scenario: Esperar com um prazo futuro notificado antes de expirar retorna com sucesso
- **WHEN** um programa C chama `pthread_cond_timedwait` com um prazo absoluto no
  futuro, e é notificado antes desse prazo
- **THEN** a função SHALL retornar indicando sucesso (não expiração), com o mutex
  associado novamente detido pela thread chamadora
