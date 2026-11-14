# Proposal

## Why

Hoje, `kernel::pthread_handles` (mutex/condvar nomeados), `kernel::file_handles`
(arquivos abertos) e `kernel::vmm` (reservas de memória virtual anônima) são
tabelas **globais**, sem nenhum rastreamento de qual processo criou qual entrada.
Isso traz dois problemas reais, não só um:

1. **Vazamento entre processos**: `kernel-process-cleanup` (arquivada) já recupera
   a memória física do espaço de endereçamento de uma thread terminada, mas deixou
   essas três tabelas de fora de propósito — nenhuma sabe de quem é cada entrada,
   então não há como saber o que liberar quando um processo termina.
2. **Acesso entre processos, hoje já possível** (achado desta pesquisa, não
   motivação original do `ROADMAP.md`): como o `handle` é só um índice sequencial
   validado apenas por existir na tabela, **qualquer processo pode operar sobre o
   handle de outro** — passar o número de um mutex, arquivo ou endereço de reserva
   que pertence a outro processo funciona hoje sem nenhuma rejeição. Nenhuma spec
   existente documenta isolamento de handle como requisito — então corrigir isso
   não contradiz nada já especificado, só preenche uma lacuna nunca antes coberta.

Este é o "Próximo passo imediato" do `ROADMAP.md` depois de `kernel-process-cleanup`.

**Achado de pesquisa que define a abordagem**: não existe hoje nenhuma forma de
descobrir, a partir de uma syscall em andamento, qual `ProcessId` é dono da thread
chamadora — `kernel::scheduler::register_thread` recebe um `Thread` (que já carrega
`thread.process: ProcessId`) mas **descarta esse campo**, guardando só
`thread.context` em `CONTEXTS`. O próprio `Context` já é o lugar certo para isso: já
ganhou campos por-thread incrementalmente em changes anteriores (`fpu_state`,
`syscall_rsp_slot`, `timed_out`), populados em `register_thread` — o mesmo padrão
serve para um novo campo `process: ProcessId`.

## What Changes

- `kernel::process::Context` ganha um campo `process: ProcessId`, populado em
  `kernel::scheduler::register_thread` a partir do `thread.process` que já chega
  ali (hoje descartado). Nova função `kernel::scheduler::current_process_id() ->
  ProcessId`, mesmo papel de `current_thread_id()` já existente.
- `kernel::pthread_handles`/`kernel::file_handles`/`kernel::vmm` passam a registrar
  o `ProcessId` dono de cada entrada na criação (`create_mutex`/`create_cond`/
  `register`/`reserve`, cada um ganhando um parâmetro `owner: ProcessId`), e a
  rejeitar qualquer operação (`mutex`/`cond`/`read`/`close`/`commit`/`decommit`/
  `protect`/`release`) cujo chamador não seja o dono — reaproveitando o mesmo
  sentinela de erro já usado hoje para "handle desconhecido" (nenhuma mudança de
  assinatura de syscall, nenhum código de erro novo).
- Quando uma thread termina (voluntária ou involuntariamente — mesmo ponto de
  `kernel::scheduler::schedule_next` que já chama `AddressSpace::
  free_user_mappings`), todo `file_handles`/região de `kernel::vmm` que seu
  processo possuía é fechado/liberado de verdade (reaproveitando `close()`/
  `release()` já existentes).
- **`kernel::pthread_handles` continua sem destruição real** (mutex/condvar
  `Box::leak`ados permanecem alocados para sempre) — só ganha o rastreamento de
  dono e a rejeição de acesso entre processos. Ver "Impact"/design.md para o
  porquê de não revisitar essa decisão agora.

## Capabilities

### New Capabilities
- `kernel-handle-ownership`: cada handle (mutex/condvar nomeado, arquivo aberto,
  reserva de memória virtual) pertence ao processo que o criou; nenhum outro
  processo pode operar sobre ele; os handles/reservas de um processo são
  fechados/liberados quando ele termina (onde já existe uma operação de
  fechamento/liberação real — arquivo e memória virtual).

### Modified Capabilities
(nenhuma — a regra de posse é transversal às operações já especificadas em
`kernel-thread-sync`/`kernel-filesystem`/`kernel-virtual-memory`/`kernel-syscalls`,
mas nenhuma delas documenta hoje nada sobre escopo entre processos para
contradizer; tratada como uma capability nova e coesa em vez de dezenas de edições
mecânicas repetindo a mesma regra em cada requisito existente)

## Impact

- **Código afetado**: `kernel/src/process.rs` (`Context.process`),
  `kernel/src/scheduler.rs` (população do campo, `current_process_id`, chamada de
  limpeza), `kernel/src/pthread_handles.rs`, `kernel/src/file_handles.rs`,
  `kernel/src/vmm.rs`, `kernel/src/syscall.rs` (toda `sys_*` que cria ou consulta um
  handle passa a repassar `current_process_id()`).
- **Não afetado**: nenhuma assinatura de syscall nem código de retorno novo — um
  processo que só usa seus próprios handles (todo programa de teste já existente)
  não observa nenhuma mudança de comportamento; só um acesso entre processos
  (hoje sempre bem-sucedido, nunca exercido por nenhum teste existente) passa a
  ser rejeitado.
- **Por que `pthread_handles` não ganha destruição real agora**: seus
  `Mutex`/`CondVar` são `Box::leak`ados porque toda a API de
  `kernel::pthread::Mutex`/`CondVar` exige referência `'static` — reclamar essa
  memória exigiria `unsafe { Box::from_raw(...) }`, que só é seguro se nenhuma
  outra referência `&'static` a esse objeto ainda circula em algum lugar do
  kernel. Essa é uma decisão de segurança de memória própria, maior que "mais um
  campo de dono" — já era um Non-Goal explícito desde `kernel-pthread-sync`, e
  continua sendo um Non-Goal aqui.
- **Verificação**: totalmente provável em boot real — um segundo processo
  tentando operar sobre o handle de um primeiro (rejeitado), e a liberação de
  arquivo/memória virtual observável da mesma forma que `kernel-process-cleanup`
  já observou a liberação de frames.
