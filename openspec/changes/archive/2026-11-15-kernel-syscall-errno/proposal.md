# Proposal

## Why

Hoje, 23 das 24 *syscalls* do kernel colapsam todo erro num único sentinela
genérico (`SYSCALL_ERROR = u64::MAX`), distinguindo só três casos especiais
ad-hoc adicionados incrementalmente (`SYSCALL_TIMEOUT`, `SYSCALL_EFAULT`).
Isso tem dois custos reais, não só estéticos:

1. **`userland_c/glue.c` já precisa adivinhar `errno` real por chamada, às
   vezes errado**: `pthread_mutex_trylock` devolve `EBUSY` tanto para "já
   estava tomado" quanto para "`handle` desconhecido" (deveria ser `EINVAL`
   neste segundo caso — exatamente a distinção que o próprio `pthread_mutex_
   trylock(3)` POSIX real faz); `read()` sempre assume `EBADF`, mesmo quando
   a causa real é outra. A libc C não tem como fazer melhor hoje, porque o
   kernel não entrega informação suficiente para isso.
2. Internamente, várias funções de kernel já calculam um erro bem mais
   específico (`VmmError`, `Fat32Error`, `DylibError` — cada um com vários
   variantes distintos) e o jogam fora na fronteira da *syscall*,
   convertendo tudo para o mesmo sentinela.

Este é o próximo passo imediato do `ROADMAP.md` ("Esquema de erro granular
(`errno`-like) mais amplo nas syscalls"), deliberadamente adiado pela change
`kernel-user-pointer-validation` (que só introduziu `SYSCALL_EFAULT` como
primeiro código distinto, documentando a decisão de não ir além disso
naquele momento).

## What Changes

- **BREAKING** (na convenção interna de retorno de toda *syscall* que pode
  falhar, não na numeração/ordem de argumentos de nenhuma *syscall* já
  existente): `SYSCALL_ERROR`/`SYSCALL_TIMEOUT`/`SYSCALL_EFAULT` são
  removidos. Em seu lugar, toda falha passa a devolver um código de erro
  POSIX real (`EINVAL`/`ENOENT`/`EBADF`/`EACCES`/`EBUSY`/`ENOMEM`/`EFAULT`/
  `ETIMEDOUT`/etc. — os mesmos valores numéricos de `<errno.h>` do newlib já
  vendorizado) codificado como `-errno` (inteiro de 64 bits assinado,
  reinterpretado como `u64`) — a mesma convenção usada pelas *syscalls* reais
  do kernel Linux. Sucesso de qualquer *syscall* continua sendo um valor
  `u64` dentro da faixa de uso normal de cada uma (endereço, `handle`,
  contagem de bytes, nanossegundos) — nenhuma dessas faixas jamais chega
  perto de `u64::MAX`, então não há ambiguidade entre sucesso e erro.
- Cada chamada que hoje só devolve "o sentinela de erro" ganha uma tabela
  explícita de qual código de erro corresponde a qual causa de falha —
  catálogo completo em design.md, "Mapeamento de erro por *syscall*".
- `userland_c/glue.c` passa a definir `errno` com o valor real devolvido
  pelo kernel (`errno = (int)-ret`), em vez dos valores fixos que hoje
  adivinha por função — corrige a imprecisão de `pthread_mutex_trylock`
  descrita acima, entre outras.
- Nenhuma mudança na numeração das 23 *syscalls* existentes, nos argumentos
  de nenhuma delas, nem no valor de sucesso de nenhuma — só o valor
  devolvido em caso de falha muda de forma (um sentinela fixo → um código de
  erro específico).

## Capabilities

### New Capabilities

(nenhuma)

### Modified Capabilities

- `kernel-syscalls`: toda chamada que pode falhar ganha um requisito
  explícito do código de erro POSIX devolvido para cada causa de falha
  distinta, substituindo "o sentinela de erro" genérico usado em cada
  requisito já existente.

## Impact

- `kernel/src/syscall.rs`: reescrita da maior parte das funções `sys_*`
  (cada uma passa a devolver um código específico por causa de falha, em
  vez de um `SYSCALL_ERROR` único) — arquivo com maior superfície de
  mudança desta change.
- `userland_c/glue.c`: toda função que hoje checa `r == SYSCALL_ERROR`/
  `SYSCALL_TIMEOUT` e define um `errno` fixo passa a propagar o valor real.
- `userland_c/efault_test_program.c` (change `kernel-user-pointer-
  validation`): compara o retorno bruto da *syscall* contra `SYSCALL_EFAULT`
  (`-3L`) — precisa ser atualizado para comparar contra `-EFAULT` (`-14L`),
  já que o valor concreto do sentinela muda.
- Nenhum programa de teste existente que só verifica sucesso (o caso comum)
  precisa mudar — só os que inspecionam o valor exato de um retorno de erro.
