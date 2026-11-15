# Proposal

## Why

Toda *syscall* que recebe um ponteiro de modo usuário (`SYSCALL_WRITE`,
`SYSCALL_DLOPEN`, `SYSCALL_DLSYM`, `SYSCALL_OPEN`, `SYSCALL_READ`) o
desreferencia sem nenhuma validação — documentado desde sempre como
precedente deliberado de `sys_write` ("`ptr`/`len` vêm diretamente do
chamador em modo usuário, sem validação"). Isso não é só uma lacuna de
robustez: desde `kernel-fault-isolation`, uma exceção de CPU causada por
código de modo usuário encerra só a thread causadora — mas um ponteiro ruim
passado a uma dessas *syscalls* causa a falta de página **depois** da
transição para modo kernel (a *syscall* já está rodando com `CS` de anel 0
quando desreferencia o ponteiro do chamador), então `kernel-fault-isolation`
classifica essa falta como originada em modo kernel e derruba o kernel
inteiro — exatamente a categoria de falha que aquela change já isolou para
todo código que nunca cruza para dentro do kernel. Um processo de modo
usuário mal-comportado (ou só com um bug) pode hoje travar o sistema inteiro
passando qualquer ponteiro inválido a uma dessas cinco chamadas.

## What Changes

- Nova função de validação em `kernel::paging` que verifica, para um
  intervalo `[ptr, ptr+len)` de modo usuário, que toda página de 4 KiB
  tocada está mapeada, acessível a partir do modo usuário e (quando a
  *syscall* escreve nela, não só lê) gravável — sem desreferenciar nada,
  só consultando as tabelas de página do espaço de endereçamento corrente.
- `sys_write`/`sys_dlopen`/`sys_dlsym`/`sys_open` (leitura do chamador) e
  `sys_read` (escrita no buffer do chamador) passam a validar seu(s)
  ponteiro(s) com essa função antes de qualquer `core::slice::from_raw_parts`
  — um ponteiro inválido é rejeitado, nunca desreferenciado.
- Novo sentinela de erro `SYSCALL_EFAULT`, distinto do já existente
  `SYSCALL_ERROR` (e de `SYSCALL_TIMEOUT`) — devolvido especificamente
  quando a rejeição é por ponteiro inválido, nunca nos demais casos de erro
  já existentes de cada chamada (que continuam devolvendo `SYSCALL_ERROR`,
  sem nenhuma mudança). Este é o primeiro passo de um esquema de erro mais
  granular (ver design.md, "Alternativas consideradas" para o porquê de não
  ir além disso nesta change).
- **Não é BREAKING** para nenhum chamador hoje existente: todo programa de
  teste já existente só passa ponteiros válidos (sua própria stack/heap/
  seções mapeadas), então nenhum resultado observável muda — só o novo
  caminho de rejeição (hoje inatingível por qualquer chamador legítimo)
  passa a existir.

## Capabilities

### New Capabilities

(nenhuma)

### Modified Capabilities

- `kernel-syscalls`: ganha um novo requisito cobrindo a validação de
  ponteiro de usuário, aplicável às cinco chamadas que hoje recebem um
  ponteiro de modo usuário.

## Impact

- `kernel/src/paging.rs`: nova função de validação de intervalo de memória
  de usuário (`Mapper`).
- `kernel/src/syscall.rs`: `sys_write`/`sys_dlopen`/`sys_dlsym`/`sys_open`/
  `sys_read` passam a validar seus ponteiros; novo `SYSCALL_EFAULT`.
- Novo programa de teste de modo usuário (`userland_c/`) que invoca uma
  dessas *syscalls* com um ponteiro deliberadamente inválido (ex.: `0x0`,
  ou um endereço nunca mapeado) e confirma `SYSCALL_EFAULT`, sem derrubar o
  kernel — prova de que o buraco de `kernel-fault-isolation` descrito acima
  está de fato fechado.
- Nenhuma mudança em `userland_c/glue.c`/libc existente necessária (o
  sentinela `SYSCALL_EFAULT` é só mais um valor de retorno de `RAX`,
  tratável pelo mesmo chamador que já trata `SYSCALL_ERROR`).
