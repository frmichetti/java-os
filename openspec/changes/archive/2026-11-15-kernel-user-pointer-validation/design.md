# Design

## Context

Ver proposal.md para a motivação completa (um ponteiro ruim passado a
`SYSCALL_WRITE`/`SYSCALL_DLOPEN`/`SYSCALL_DLSYM`/`SYSCALL_OPEN`/
`SYSCALL_READ` causa uma falta de página em modo **kernel**, fora do alcance
de `kernel-fault-isolation`, derrubando o kernel inteiro).

`kernel::paging::Mapper` já expõe `translate(virt) -> Option<u64>`, uma
consulta somente-leitura que caminha PML4→PDPT→PD→PT e devolve o endereço
físico mapeado (ou `None` se qualquer nível não estiver presente) — mas
exige `virt` alinhado a página e só devolve o endereço físico, não as flags
de permissão (`PTE_USER`/`PTE_WRITABLE`) de que esta change precisa. Como os
demais métodos de `Mapper` que tocam hardware de verdade (`map`/`unmap`/
`translate`), ela só é verificável em boot real — teste de host cobre só a
lógica pura em torno dela (ver `translate_rejects_misaligned_address_
without_touching_cr3`, mesmo precedente).

`kernel::syscall` já reserva `SYSCALL_ERROR = u64::MAX` e
`SYSCALL_TIMEOUT = u64::MAX - 1` como sentinelas distintos devolvidos em
`RAX` — o padrão já estabelecido para introduzir um sentinela novo sem
mudar a convenção de ABI (número da chamada em `RAX`, retorno em `RAX`,
nenhuma struct de erro, nenhum registrador adicional).

## Goals / Non-Goals

**Goals:**
- Nenhum ponteiro de modo usuário é desreferenciado pelo kernel antes de
  confirmar que toda a memória que ele cobre está mapeada, acessível a
  partir do modo usuário e (se for escrita) gravável.
- Um ponteiro inválido é rejeitado com um sentinela distinto
  (`SYSCALL_EFAULT`), nunca com uma falta de página.

**Non-Goals:**
- Qualquer esquema de erro mais amplo que distinga outras categorias de
  falha (handle desconhecido, tamanho inválido, etc.) — ver "Alternativas
  consideradas" abaixo. Essas continuam devolvendo `SYSCALL_ERROR`, sem
  nenhuma mudança nesta change.
- Proteção contra um ponteiro que aponta para memória válida e com a
  permissão certa, mas para dados que o chamador não deveria poder ler
  (ex.: um ponteiro de outro processo que por acaso aponta para um endereço
  também mapeado no espaço do chamador) — fora de escopo: cada processo tem
  seu próprio espaço de endereçamento isolado desde `kernel-memory-
  management`; esta change só garante que o endereço *pertence* ao chamador,
  não audita a semântica do que ele decidiu apontar.
- Suporte a páginas grandes (`PTE_HUGE_PAGE`) — nenhum mapeamento de
  usuário hoje as usa (mesma limitação já herdada por `translate`), então a
  caminhada sempre desce até a PT.
- Validar ponteiros de qualquer `arg` que hoje **não** é um par
  ponteiro/tamanho de memória do chamador (ex.: `handle`s opacos de
  mutex/condvar/arquivo/VMM, que já são validados por posse desde
  `kernel-handle-ownership`, ou endereços de reserva de VMM, já validados
  pela própria tabela de reservas) — só as cinco chamadas listadas na
  proposta recebem um ponteiro de memória bruto.

## Decisions

### Validação por página, reaproveitando a caminhada de `Mapper`

Nova função `pub fn validate_user_range(&self, ptr: u64, len: u64,
require_writable: bool) -> bool` em `kernel::paging::Mapper`: calcula a
primeira página e a quantidade de páginas tocadas pelo intervalo
`[ptr, ptr+len)` (lógica pura, extraída numa função própria — ver abaixo) e,
para cada uma, caminha as 4 tabelas (mesmo código de `translate`, mas
devolvendo as *flags* da entrada folha, não só o endereço físico) checando
`PTE_PRESENT` + `PTE_USER` (+ `PTE_WRITABLE` se `require_writable`). Primeira
página sem essas flags (ou não presente) já retorna `false` imediatamente,
sem checar o restante. **Alternativa rejeitada**: validar só a primeira e a
última página do intervalo (mais barato) — rejeitada porque um intervalo
pode atravessar uma página mapeada seguida de uma não mapeada no meio (ex.:
um buffer que começa dentro de uma região válida mas se estende além dela),
e essa página do meio nunca seria checada.

### Lógica de span extraída como função pura, testável em host

`fn spanned_pages(ptr: u64, len: u64) -> Option<(u64, u64)>` (primeira
página alinhada, quantidade de páginas) é uma função livre, sem `self`, sem
tocar hardware — só aritmética sobre `ptr`/`len`. Devolve `None` se
`ptr + len` estourar `u64` (em vez de um resultado truncado/errado que
poderia validar páginas de menos). `validate_user_range` chama essa função
primeiro e só então entra na parte que toca hardware (caminhada real de
tabelas), mesma separação "lógica pura vs. hardware" já usada em todo o
resto do kernel (ver `kernel::clock::cycles_to_nanos` vs. `calibrate_tsc`,
por exemplo). `len == 0` é tratado como caso especial em
`validate_user_range` (sempre válido, sem nem chamar `spanned_pages`) — ver
spec, "Tamanho zero é sempre válido".

### `SYSCALL_EFAULT = u64::MAX - 2`

Terceiro sentinela da mesma família de `SYSCALL_ERROR`/`SYSCALL_TIMEOUT` —
nenhuma mudança na convenção de ABI. Cada uma das cinco chamadas valida seu
ponteiro **antes** de qualquer outra validação (handle, tamanho, etc.) e
devolve `SYSCALL_EFAULT` imediatamente se a validação falhar — nenhum efeito
colateral (ex.: avançar o cursor de um arquivo) acontece antes dessa
checagem.

### Alternativas consideradas (esquema de erro mais amplo)

Como registrado em `ROADMAP.md`, o item original cogitava um "esquema de
erro granular (`errno`-like)" mais amplo — rejeitado para esta change (a
pedido explícito do usuário, consultado diretamente): um esquema completo
ao estilo POSIX (`ENOENT`/`EINVAL`/`EACCES`/etc. por chamada) reescreveria a
maioria dos requisitos já existentes de `kernel-syscalls` (hoje todos usam
só "o sentinela de erro" genérico), um escopo bem maior e mais arriscado do
que o buraco de segurança real identificado aqui. Fica como um próximo passo
possível, não decidido nesta change.

## Risks / Trade-offs

- **[Risco] Custo de caminhar as tabelas de página por página para um
  buffer grande (ex.: uma leitura de arquivo grande em `SYSCALL_READ`)** →
  Aceito: este kernel não é otimizado para throughput de E/S hoje (lê o
  arquivo inteiro de uma vez na abertura, ver `kernel::file_handles`), e o
  custo de validação é da mesma ordem de grandeza do resto do trabalho já
  feito por chamada.
- **[Risco] TOCTOU — a validação e o acesso real não são atômicos; um
  mapeamento poderia mudar entre as duas** → Mitigação: não aplicável hoje
  (nenhum processo de modo usuário tem mais de uma thread, e cada *syscall*
  roda com interrupções desabilitadas até a validação concluir, sem ceder a
  CPU no meio do caminho) — revisitar quando `kernel-user-mode` ganhar
  criação de thread.
- **[Risco] Esquecer de validar um ponteiro novo numa *syscall* futura** →
  Mitigação parcial, mesma categoria já aceita em `kernel-handle-ownership`
  para posse de handle: não há verificação automática do compilador aqui
  (ao contrário de um parâmetro obrigatório), então fica documentado como
  convenção a seguir por qualquer *syscall* nova que receba um ponteiro de
  memória do chamador.

## Migration Plan

Puramente aditivo para todo chamador que já passa ponteiros válidos (nenhuma
mudança observável) — o único caminho novo é a rejeição com
`SYSCALL_EFAULT`, hoje inatingível por qualquer programa de teste já
existente. Nenhum passo de migração necessário.

## Resultado real da implementação

**Testes de host**: `cargo test -p kernel --lib` passou com 287 testes (281
de antes desta change + 6 novos: 5 de `spanned_pages` cobrindo intervalo
dentro de uma página/atravessando duas/terminando exatamente no início da
seguinte/estouro de `u64`/tamanho zero, e 1 de `validate_user_range` com
`len == 0`, o único caso do método inteiro testável em host sem tocar `CR3`
— mesma limitação de testabilidade já documentada para `translate`/`map`/
`unmap`).

**Boot real** (`cargo xtask test`, harness padrão de 15s): `smoke test de
boot: PASSOU`. O novo `/bin/efault.elf` reportou `efault_test_program:
resultado PPPP (ordem: write_valido,write_nulo,open_invalido,
read_invalido_sem_consumir_cursor)` — os 4 cenários da spec confirmados:
ponteiro válido continua funcionando sem nenhuma mudança observável,
ponteiro nulo em `SYSCALL_WRITE` rejeitado com `SYSCALL_EFAULT`, ponteiro de
caminho inválido em `SYSCALL_OPEN` rejeitado, e buffer de leitura inválido
em `SYSCALL_READ` rejeitado sem consumir o cursor do arquivo (confirmado
lendo o primeiro byte real do arquivo logo depois da tentativa inválida).
Todo programa de teste já existente reportou exatamente o mesmo padrão de
resultado de antes desta change — nenhuma regressão para ponteiros válidos.
O único `EXCECAO #PF` do log é o já esperado, deliberado, de
`fault_test_program.c` (`kernel-fault-isolation`), sem nenhuma relação com
esta change — nenhum novo panic ou exceção de CPU em modo kernel apareceu.

**Achado real, simplificação em relação ao design.md**: uma pequena função
privada `kernel::syscall::user_range_is_valid(ptr, len, require_writable)`
(lock do `FRAME_ALLOCATOR` + `Mapper::new` + `validate_user_range`) não
estava prevista explicitamente no design, mas evitou repetir esse bloco de
3 linhas em cada uma das 5 chamadas — cada `sys_*` chama só essa função e
devolve `SYSCALL_EFAULT` se `false`.
