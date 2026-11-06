# Proposal

## Why

O HotSpot precisa carregar classes do class path (arquivos `.class`/`.jar`) em tempo de execução, a partir de código rodando em modo usuário — não de dentro do próprio kernel. Hoje `kernel::fat32::Fat32Filesystem::read_file` já lê o conteúdo completo de um arquivo por caminho absoluto, mas essa capacidade só existe como chamada de função Rust interna ao kernel (usada por `sys_dlopen`/`spawn_from_elf`); não há nenhuma forma de um processo em modo usuário abrir e ler um arquivo por conta própria. Esse gap foi deixado explicitamente em aberto pela change `kernel-fat32-filesystem` ("Ainda sem exposição via *syscall* (`open`/`read`) a modo usuário — fica para uma sub-change futura, mesma disciplina do item 8 do roadmap") e é a próxima sub-change pendente do item 8 ("Porte inicial da JVM") antes do próprio porte do HotSpot poder começar.

## What Changes

- Nova tabela de handles de arquivo aberto (`kernel::file_handles`, mesmo padrão de `kernel::pthread_handles`): cada `open` bem-sucedido monta o sistema de arquivos, lê o conteúdo completo do arquivo (reaproveitando `Fat32Filesystem::read_file`, sem leitura incremental por cluster) e registra um handle opaco com um cursor de posição.
- Três novas chamadas de sistema, seguindo a convenção de ABI já estabelecida (número em `RAX`, argumentos em `RDI`/`RSI`/`RDX`/`R10`, retorno em `RAX`, `SYSCALL_ERROR` para qualquer falha):
  - `SYSCALL_OPEN`: abre um arquivo por caminho absoluto, devolve um `handle` opaco.
  - `SYSCALL_READ`: copia bytes do cursor atual de um handle aberto para um buffer do chamador, avança o cursor, devolve a quantidade de bytes lidos (`0` ao alcançar o fim do arquivo).
  - `SYSCALL_CLOSE`: invalida um handle aberto, liberando seus recursos.
- Nenhuma mudança em `kernel::fat32`/`kernel::gpt` em si — a capability `kernel-filesystem` ganha a semântica de handle/cursor como uma camada nova sobre a leitura de arquivo completo já existente, não uma reescrita dela.

Fora de escopo (mesma disciplina minimalista das changes anteriores do item 8):
- Escrita em arquivo (`write`/`SYSCALL_WRITE` já existe, mas só para o console — não se aplica a arquivos nesta change).
- `lseek`/busca por posição arbitrária (cursor só avança por leitura sequencial).
- Diretórios, VFAT/nomes longos, qualquer sistema de arquivo além do FAT32 já implementado.
- Isolamento da tabela de handles por processo (permanece global ao kernel, mesma limitação já aceita e documentada para `kernel::pthread_handles`/`kernel::vmm`).

## Capabilities

### New Capabilities

(nenhuma — esta change estende duas capabilities já existentes)

### Modified Capabilities

- `kernel-filesystem`: adiciona a semântica de abrir um arquivo por caminho (obtendo um handle com cursor de leitura independente), ler sequencialmente a partir do cursor, e fechar o handle — construída sobre a leitura de arquivo completo por caminho já especificada, sem alterá-la.
- `kernel-syscalls`: adiciona três novas chamadas de sistema (`SYSCALL_OPEN`, `SYSCALL_READ`, `SYSCALL_CLOSE`) à tabela de despacho existente, expondo a capability `kernel-filesystem` a código em modo usuário.

## Impact

- **Código afetado**: novo módulo `kernel::file_handles` (tabela de handles, análoga a `kernel::pthread_handles`); `kernel::syscall` (três novos números de chamada e funções `sys_open`/`sys_read`/`sys_close`, mais uma nova entrada no `match` de `dispatch`).
- **Dependências**: reaproveita `kernel::fat32::Fat32Filesystem`/`mount_boot_partition` e `kernel::paging`/`kernel::phys` já existentes — nenhuma dependência nova.
- **Compatibilidade**: puramente aditivo — nenhuma syscall existente muda de número, argumentos ou retorno.
- **Documentação**: `docs/architecture/overview.md` (seções 4 e 15) precisa registrar as três novas syscalls e fechar o gap "falta apenas expor isso como *syscall*" descrito na seção 5/6 para `kernel-fat32-filesystem`.
