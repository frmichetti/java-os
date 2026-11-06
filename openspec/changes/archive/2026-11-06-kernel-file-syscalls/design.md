# Design

## Context

`kernel::fat32::Fat32Filesystem::read_file` já existe e lê o conteúdo completo de um arquivo (por caminho absoluto) para um `Vec<u8>` numa única chamada — não há, hoje, nenhum conceito de posição/cursor incremental dentro de `kernel::fat32`; cada chamada relê o arquivo do zero. `kernel::fat32::mount_boot_partition()` monta a partição de boot a cada chamada (sem estado persistente entre chamadas) — mesmo padrão de custo já aceito por `sys_dlopen`/`kernel::fat32_demo`.

O padrão de tabela de handle dinâmico já existe e está estabelecido por `kernel::pthread_handles` (`Spinlock<Vec<T>>`, índice como handle) e é reaproveitado por `kernel::syscall::dispatch` para `SYSCALL_MUTEX_*`/`SYSCALL_COND_*`. Esta change segue o mesmo padrão para arquivos abertos, com uma diferença deliberada explicada abaixo (Decisão "Liberação de memória ao fechar").

Ver proposal.md para motivação; ver specs delta de `kernel-filesystem`/`kernel-syscalls` para o contrato comportamental completo.

## Goals / Non-Goals

**Goals:**
- Permitir que código em modo usuário abra, leia sequencialmente e feche um arquivo do FAT32 montado, através de três novas syscalls.
- Liberar a memória do conteúdo de um arquivo assim que ele é fechado (ao contrário de `kernel::pthread_handles`, cujos objetos nunca são destruídos).

**Non-Goals:**
- Leitura incremental por cluster (streaming): o arquivo inteiro é lido para memória na abertura, como `read_file` já faz hoje. Arquivos `.class`/`.jar` individuais são pequenos o suficiente para isso ser aceitável nesta etapa; reconsiderar se/quando um caso real exigir arquivos grandes demais para caber na memória do kernel de uma vez.
- `lseek`/posicionamento arbitrário do cursor — só avanço sequencial via leitura.
- Escrita em arquivo, diretórios, VFAT/nomes longos — ver proposal.md.
- Isolamento da tabela de handles por processo — permanece uma tabela global ao kernel, mesma limitação já aceita para `kernel::pthread_handles`/`kernel::vmm` (ver `docs/architecture/overview.md`, seção 4, "Limitação atual").

## Decisions

### Estrutura da tabela: `Vec<Option<OpenFile>>`, nunca reaproveita índice

`kernel::file_handles` mantém `static OPEN_FILES: Spinlock<Vec<Option<OpenFile>>>`, onde `OpenFile { contents: Vec<u8>, cursor: usize }`. `open` sempre faz `push` de uma nova entrada (índice = handle), nunca reaproveita um índice já fechado — mesma política de "a tabela só cresce" de `pthread_handles::create_mutex`/`create_cond`.

**Alternativa considerada e rejeitada**: reaproveitar o primeiro slot `None` livre ao abrir um novo arquivo (economiza entradas de tabela). Rejeitada porque introduz um problema ABA: um handle antigo (já fechado, guardado por engano ou por uma otimização futura do chamador) voltaria a apontar — silenciosamente — para um arquivo completamente diferente assim que o índice fosse reaproveitado, violando a expectativa de "handle desconhecido é rejeitado" da spec. Como cada entrada fechada vira `None` (ver decisão seguinte), o custo de nunca reaproveitar é apenas um ponteiro nulo por handle historicamente aberto — desprezível frente ao risco.

### Liberação de memória ao fechar (diferença deliberada de `kernel::pthread_handles`)

Ao contrário de `Mutex`/`CondVar` (que nunca são destruídos, pois vivem por `Box::leak` com referência `'static` exigida pela API de bloqueio), o conteúdo de um arquivo aberto não precisa de referência `'static` — é só um `Vec<u8>` lido uma vez. `sys_close` substitui a entrada por `None`, liberando o `Vec<u8>` (e seu buffer de heap) imediatamente.

**Por que isso importa aqui e não em `pthread_handles`**: carregamento de classes abre potencialmente centenas de arquivos `.class` ao longo da vida de uma JVM; nunca liberar o conteúdo lido exauriria o heap do kernel de forma previsível e rápida. Mutexes/condvars nomeados, por comparação, tendem a ser criados uma vez e mantidos pela vida inteira do processo (não há caso de uso análogo a "abrir milhares e descartar a maioria").

### `open` monta o sistema de arquivos e lê o arquivo inteiro de uma vez

`sys_open` chama `fat32::mount_boot_partition()` e, em caso de sucesso, `Fat32Filesystem::read_file(path)` — mesma sequência e mesmo tratamento de erro (`SYSCALL_ERROR`) já usado por `sys_dlopen`. Nenhum estado de montagem é mantido entre chamadas de `open` distintas (mesmo custo aceito em `sys_dlopen`/`kernel::fat32_demo`).

### Numeração de syscalls

Próximos números livres após `SYSCALL_DLSYM` (17):

| Número | Nome | Argumentos | Retorno |
|---|---|---|---|
| 18 | `SYSCALL_OPEN` | `path_ptr`, `path_len` | `handle`, ou `SYSCALL_ERROR` |
| 19 | `SYSCALL_READ` | `handle`, `buf_ptr`, `buf_len` | bytes lidos, ou `SYSCALL_ERROR` |
| 20 | `SYSCALL_CLOSE` | `handle` | `0`, ou `SYSCALL_ERROR` |

`path_ptr`/`path_len` seguem a mesma convenção de string já usada por `SYSCALL_WRITE`/`SYSCALL_DLOPEN` (ponteiro + tamanho de bytes UTF-8 do chamador, sem validação — mesmo precedente de `sys_write`/`sys_dlopen`). `SYSCALL_READ` copia para `buf_ptr` via `core::slice::from_raw_parts_mut` + cópia de bytes, mesmo padrão não-seguro aceito pelas demais syscalls que tocam memória de usuário diretamente.

## Risks / Trade-offs

- **[Risco] Arquivo grande demais esgota o heap do kernel na abertura** (lido inteiro de uma vez) → Mitigação: aceito como Non-Goal nesta etapa; `read_file` já tinha essa mesma limitação antes desta change (usada por `sys_dlopen`), e arquivos `.class`/`.jar` individuais do classpath da JVM são tipicamente pequenos. Reconsiderar (leitura incremental por cluster) se um caso real exigir.
- **[Risco] Tabela de handles nunca encolhe (`Vec` só cresce em comprimento, mesmo com entradas `None`)** → Mitigação: o custo por entrada fechada é um `Option::None` (poucos bytes), não o conteúdo do arquivo; aceitável para a escala desta etapa (mesma disciplina de `pthread_handles`, que tem o mesmo comportamento para o índice em si).
- **[Risco] Sem isolamento entre processos — qualquer processo pode ler/fechar o handle de outro, bastando acertar o número** → Mitigação: nenhuma; é a mesma limitação já documentada e aceita para `kernel::pthread_handles`/`kernel::vmm` nesta fase do roadmap (item 8). Isolamento por processo fica para quando/se necessário.

## Migration Plan

Puramente aditivo — nenhuma syscall ou API existente muda. Nenhum passo de migração necessário.
