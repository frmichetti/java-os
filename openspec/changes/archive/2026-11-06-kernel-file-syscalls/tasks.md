# Tasks

## 1. Tabela de handles de arquivo (`kernel::file_handles`)

- [x] 1.1 Criar `kernel::file_handles` com `OpenFile { contents: Vec<u8>, cursor: usize }` e `static OPEN_FILES: Spinlock<Vec<Option<OpenFile>>>`, seguindo a estrutura de `kernel::pthread_handles` (índice do `Vec` = handle) — ver design.md, "Estrutura da tabela"
- [x] 1.2 Implementar `register(contents: Vec<u8>) -> u64` (faz `push(Some(OpenFile { contents, cursor: 0 }))`, devolve o índice) e verificar com teste de host que duas chamadas sucessivas devolvem handles distintos
- [x] 1.3 Implementar `read(handle: u64, len: usize) -> Option<Vec<u8>>` (copia até `len` bytes a partir do cursor da entrada, avança o cursor, `None` se o handle não existir ou já foi fechado) e verificar com testes de host: leitura dentro do arquivo, leitura que ultrapassa o fim (devolve só o restante), leitura com cursor já no fim (devolve vetor vazio), leitura com handle desconhecido (`None`)
- [x] 1.4 Implementar `close(handle: u64) -> bool` (substitui a entrada por `None`, liberando o `Vec<u8>`; `false` se o handle não existir ou já foi fechado) e verificar com testes de host: fechar um handle válido libera-o (leitura seguinte com o mesmo handle devolve `None`), fechar um handle desconhecido ou já fechado devolve `false` sem afetar outras entradas abertas
- [x] 1.5 Verificar com teste de host que dois handles abertos simultaneamente têm cursores independentes (avançar um via `read` não afeta a posição do outro)

## 2. Syscalls `SYSCALL_OPEN`/`SYSCALL_READ`/`SYSCALL_CLOSE`

- [x] 2.1 Adicionar as constantes `SYSCALL_OPEN = 18`, `SYSCALL_READ = 19`, `SYSCALL_CLOSE = 20` em `kernel::syscall`, documentando a tabela de argumentos/retorno conforme design.md
- [x] 2.2 Implementar `sys_open(path_ptr, path_len) -> u64`: decodifica o caminho UTF-8 (mesmo padrão de `sys_dlopen`), monta o sistema de arquivos via `fat32::mount_boot_partition()`, lê o arquivo completo via `Fat32Filesystem::read_file`, registra via `file_handles::register` e devolve o handle; `SYSCALL_ERROR` para UTF-8 inválido, falha de montagem, ou arquivo inexistente/diretório
- [x] 2.3 Implementar `sys_read(handle, buf_ptr, buf_len) -> u64`: chama `file_handles::read`, copia os bytes devolvidos para `buf_ptr` (mesmo padrão de acesso não validado a ponteiro de usuário de `sys_write`), devolve a quantidade copiada; `SYSCALL_ERROR` se o handle não existir
- [x] 2.4 Implementar `sys_close(handle) -> u64`: chama `file_handles::close`, devolve `0` em sucesso ou `SYSCALL_ERROR` se o handle não existir
- [x] 2.5 Adicionar as três novas chamadas ao `match` de `dispatch` em `kernel::syscall`
- [x] 2.6 Testes de host para os caminhos de erro dispatchados por número de syscall (mesmo padrão dos testes existentes de `SYSCALL_MUTEX_LOCK`/`SYSCALL_COND_WAIT` com handle desconhecido): `SYSCALL_READ`/`SYSCALL_CLOSE` com handle fora dos limites devolvem `SYSCALL_ERROR`

## 3. Verificação em boot real

- [x] 3.1 Adicionar um arquivo de teste pequeno e com conteúdo conhecido à imagem de disco (via `xtask build-image`, mesmo mecanismo que já copia `::/bin/test.elf`/`::/lib/libbase.so`), para servir de fixture às novas syscalls
- [x] 3.2 Criar `kernel::file_syscalls_demo` (mesmo padrão de `kernel::fat32_demo`/`kernel::vmm_demo`): a partir de uma thread de usuário, invoca `SYSCALL_OPEN` no arquivo fixture, faz uma ou mais chamadas de `SYSCALL_READ` cobrindo leitura parcial e leitura até o fim do arquivo (incluindo uma leitura extra que deve devolver zero bytes), compara o conteúdo lido byte a byte com o esperado, e por fim `SYSCALL_CLOSE`, reportando cada etapa pelo console serial
- [x] 3.3 Verificar em boot real via `cargo xtask run -- -serial stdio -display none` que `kernel::file_syscalls_demo` reporta sucesso em todas as etapas (conteúdo lido bate exatamente com o arquivo fixture, leitura após o fim devolve zero, fechamento bem-sucedido)
- [x] 3.4 Verificar em boot real um cenário de erro: `SYSCALL_READ`/`SYSCALL_CLOSE` com um handle após ele já ter sido fechado devolvem `SYSCALL_ERROR`, reportado pelo console serial

## 4. Documentação

- [x] 4.1 Atualizar `docs/architecture/overview.md` seção 4 (System Calls): adicionar `SYSCALL_OPEN`/`SYSCALL_READ`/`SYSCALL_CLOSE` à tabela de chamadas implementadas, descrever `kernel::file_handles` (tabela também ganhou as linhas pré-existentes de `SYSCALL_DLOPEN`/`SYSCALL_DLSYM`, que faltavam nela desde a change `kernel-dynamic-linker`)
- [x] 4.2 Atualizar `docs/architecture/overview.md` seção 5 (HAL e Drivers) e seção 6 (Estratégia de Portabilidade do OpenJDK): remover/atualizar a frase "falta apenas expor isso como *syscall* (`open`/`read`) a modo usuário" do item sobre `kernel-fat32-filesystem`, registrando que essa sub-change foi entregue
- [x] 4.3 Atualizar `docs/architecture/overview.md` seção 15 (Roadmap Completo): registrar `kernel-file-syscalls` como mais uma sub-change concluída do item 8 (sem lhe atribuir um ordinal numérico: a numeração explícita "primeira".."sétima" já tinha parado de ser usada a partir de `kernel-elf-loader`/`kernel-dynamic-linker` no texto existente, então inventar "oitava"/"nona" destoaria do padrão real do documento); "Único passo restante do item 8: o próprio porte do HotSpot" continua válido

## Workflow follow-up

- Arquivar esta change (`opsx:archive`) depois que as tasks acima estiverem concluídas e revisadas.
