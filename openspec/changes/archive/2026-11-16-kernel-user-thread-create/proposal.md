# Proposal

## Why

O HotSpot (porte Zero, já escopado em `jvm-posix-compat-strategy`) não consegue
sequer iniciar sem criação real de thread: o próprio bring-up da VM
(`share/runtime/init.cpp`) cria `VMThread`, `ServiceThread`,
`NotificationThread` e `MonitorDeflationThread` antes de qualquer bytecode
rodar. Hoje o kernel não tem nenhuma forma de adicionar uma segunda thread a
um processo de usuário já existente — todo processo nasce e morre com
exatamente uma thread. `pthread_create` é, por isso, o maior bloqueio
estrutural identificado na exploração do código-fonte do HotSpot, à frente de
qualquer decisão de GC, interpretador ou classfile (ver
`ROADMAP.md`, item 6.1).

## What Changes

- Nova chamada de sistema `SYSCALL_THREAD_CREATE` (número 23): cria uma nova
  thread de usuário dentro do processo chamador, compartilhando o mesmo
  espaço de endereçamento (mesmo `CR3`), executando a partir de um ponto de
  entrada e com uma stack de usuário fornecidos pelo chamador.
- Novo alocador de faixa de stack de usuário para threads adicionais: uma
  janela fixa de endereço virtual, com slots monotônicos por processo (nunca
  reciclados), análogo ao mecanismo já existente em `kernel::stack` para
  stacks de kernel — necessário porque hoje toda stack de usuário usa uma
  constante de compilação fixa por programa de teste, o que não serve para
  uma thread criada dinamicamente em tempo de execução.
- Nova forma de localizar um `Process` por `ProcessId` em
  `PROCESSES` (hoje a lista só é percorrida por posição de criação).
- **Correção de comportamento**: a limpeza de encerramento de thread
  (liberação de frames físicos do espaço de endereçamento, fechamento de
  arquivos, liberação de reservas de VMM) passa a só rodar quando a thread
  que está terminando é a **última** thread viva do seu processo — hoje ela
  roda incondicionalmente a cada término de thread, o que é seguro apenas
  porque, até hoje, todo processo tem exatamente uma thread. Sem essa
  correção, a própria chamada de criação de thread introduzida aqui tornaria
  esse comportamento existente destrutivo: encerrar uma thread destruiria a
  memória e os handles de uma thread-irmã ainda viva no mesmo processo.
- Novo programa de teste em `userland_c/` que cria uma thread-irmã via a nova
  chamada, prova que as duas threads executam concorrentemente, compartilham
  handles do processo (mutex/arquivo/VMM), e que encerrar uma delas não afeta
  a outra.

**Fora do escopo desta change** (fica para uma change seguinte, no mesmo
padrão já usado por `os-javaos-platform-sync` sobre `kernel-thread-sync`):
wirear `os_javaos::pd_start_thread` ou `pthread_create` da libc sobre esta
nova chamada de sistema. Esta change entrega só o primitivo de kernel.

## Capabilities

### New Capabilities

Nenhuma — toda mudança se encaixa em capabilities já existentes.

### Modified Capabilities

- `kernel-processes`: adiciona o requisito de criação de uma thread adicional
  de usuário dentro de um processo existente (compartilhando o mesmo espaço
  de endereçamento); corrige o requisito de liberação de memória física ao
  encerrar para ser avaliado por processo (última thread viva), não por
  thread individual.
- `kernel-syscalls`: adiciona o requisito da nova chamada de sistema de
  criação de thread.
- `kernel-handle-ownership`: esclarece, com um novo cenário, que a liberação
  de recursos fecháveis de um processo com múltiplas threads só ocorre
  quando a última thread desse processo termina, não a cada término de
  thread individual.

## Impact

- `kernel/src/process.rs`: ativa `Process::spawn_user_thread` (hoje código
  morto, nunca chamado); adiciona busca de `Process` por `ProcessId` em
  `PROCESSES`.
- `kernel/src/scheduler.rs`: novo caminho de criação de thread de usuário
  compartilhando processo existente; gate de "última thread do processo" na
  limpeza de término (`schedule_next`, linhas ~785-830).
- `kernel/src/stack.rs` (ou novo módulo): novo alocador de slots de stack de
  usuário para threads adicionais.
- `kernel/src/syscall.rs`: novo número de chamada `SYSCALL_THREAD_CREATE = 23`
  e seu handler.
- `userland_c/`: novo programa de teste de thread-irmã.
- Nenhuma mudança de API observável para processos de uma única thread
  (comportamento hoje existente permanece idêntico).
