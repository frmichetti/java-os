# libc-pthread-sync Specification

## Purpose

Fornece a um programa C de modo usuário do Java OS exclusão mútua (`pthread_mutex_*`)
e variável de condição (`pthread_cond_*`) POSIX reais, implementadas sobre as
syscalls de mutex/condvar que o kernel já expõe.

## Requirements

### Requirement: Exclusão Mútua Real via `pthread_mutex_*`
Um programa C de modo usuário SHALL poder inicializar, adquirir, liberar e destruir
um `pthread_mutex_t` através das funções padrão `pthread_mutex_init`/`lock`/
`unlock`/`destroy`, com exclusão mútua real garantida pelo kernel.

#### Scenario: Inicializar, adquirir e liberar um mutex tem sucesso
- **WHEN** um programa C chama `pthread_mutex_init` seguido de `pthread_mutex_lock` e
  depois `pthread_mutex_unlock` sobre o mesmo `pthread_mutex_t`
- **THEN** todas as três chamadas SHALL devolver sucesso (`0`)

#### Scenario: Mutex usado com inicialização estática é criado sob demanda
- **WHEN** um programa C chama `pthread_mutex_lock` sobre um `pthread_mutex_t`
  inicializado com `PTHREAD_MUTEX_INITIALIZER`, sem nunca ter chamado
  `pthread_mutex_init` nele
- **THEN** a função SHALL criar um mutex real na primeira chamada, em vez de tratar o
  valor de inicialização estática como um handle inválido

### Requirement: Tentativa de Aquisição Não-Bloqueante via `pthread_mutex_trylock`
Um programa C de modo usuário SHALL poder tentar adquirir um `pthread_mutex_t` sem
bloquear através de `pthread_mutex_trylock`, recebendo sucesso se o mutex estava
livre ou um erro imediato (sem bloquear) se já estava tomado.

#### Scenario: Tentar adquirir um mutex livre tem sucesso imediato
- **WHEN** um programa C chama `pthread_mutex_trylock` sobre um mutex atualmente
  livre
- **THEN** a função SHALL devolver sucesso (`0`) imediatamente, e o mutex passa a
  estar tomado pela thread chamadora

#### Scenario: Tentar adquirir um mutex já tomado falha sem bloquear
- **WHEN** um programa C chama `pthread_mutex_trylock` sobre um mutex já tomado pela
  própria thread chamadora
- **THEN** a função SHALL devolver um erro imediatamente, sem bloquear a thread
  chamadora

### Requirement: Variável de Condição Real via `pthread_cond_*`
Um programa C de modo usuário SHALL poder inicializar, destruir, notificar (uma
thread ou todas) e esperar em um `pthread_cond_t` através das funções padrão
`pthread_cond_init`/`destroy`/`signal`/`broadcast`/`wait`, com bloqueio real (sem
busy-wait) e despertar coordenados pelo kernel.

#### Scenario: Inicializar e destruir uma variável de condição tem sucesso
- **WHEN** um programa C chama `pthread_cond_init` seguido de `pthread_cond_destroy`
  sobre o mesmo `pthread_cond_t`
- **THEN** ambas as chamadas SHALL devolver sucesso (`0`)

#### Scenario: Notificar uma variável de condição sem nenhum esperador tem sucesso
- **WHEN** um programa C chama `pthread_cond_signal` ou `pthread_cond_broadcast`
  sobre um `pthread_cond_t` sem nenhuma thread atualmente esperando nele
- **THEN** a função SHALL devolver sucesso (`0`), sem efeito colateral observável

#### Scenario: Variável de condição usada com inicialização estática é criada sob demanda
- **WHEN** um programa C chama `pthread_cond_signal`, `pthread_cond_broadcast` ou
  `pthread_cond_wait` sobre um `pthread_cond_t` inicializado com
  `PTHREAD_COND_INITIALIZER`, sem nunca ter chamado `pthread_cond_init` nele
- **THEN** a função SHALL criar uma variável de condição real na primeira chamada, em
  vez de tratar o valor de inicialização estática como um handle inválido

#### Scenario: Esperar libera o mutex associado e o readquire antes de retornar
- **WHEN** um programa C chama `pthread_cond_wait` com um `pthread_cond_t` e um
  `pthread_mutex_t` que a thread chamadora detém, e outra execução eventualmente
  notifica essa variável de condição
- **THEN** `pthread_cond_wait` SHALL ter liberado o mutex enquanto a thread estava
  bloqueada, e SHALL readquirir o mesmo mutex antes de retornar com sucesso
