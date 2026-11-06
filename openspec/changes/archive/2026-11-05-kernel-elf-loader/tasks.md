# Tasks

## 1. Parsing puro do cabeçalho e dos program headers ELF64

- [x] 1.1 Criar `kernel/src/elf.rs` com `enum ElfError` (formato inválido, classe/endianness/arquitetura/tipo não suportados, program header fora dos limites do buffer) e structs `ElfHeader`/`ProgramHeader` com os campos listados em design.md ("Decisões — Parsing manual")
- [x] 1.2 Implementar `ElfHeader::parse(&[u8]) -> Result<Self, ElfError>` validando magic (`0x7F 'E' 'L' 'F'`), `EI_CLASS=ELFCLASS64`, `EI_DATA=ELFDATA2LSB`, `e_type=ET_EXEC`, `e_machine=EM_X86_64`, usando só indexação com verificação de limites (sem indexação direta que entra em pânico) — verificar com `cargo test -p kernel --lib` cobrindo cada rejeição (magic errado, classe/endianness/arquitetura/tipo errados, buffer curto demais para o cabeçalho)
- [x] 1.3 Implementar iteração sobre os program headers (`e_phoff`/`e_phnum`/`e_phentsize`) filtrando os do tipo `PT_LOAD`, rejeitando com `ElfError` qualquer entrada cujo offset/tamanho declarado exceda o tamanho real do buffer — verificar com teste unitário usando um ELF sintético (bytes montados à mão no teste) com um program header apontando fora dos limites

## 2. Carregamento de segmentos e criação do processo

**Nota de verificação (achado ao planejar a implementação):** diferente do
parsing do grupo 1 (lógica pura sobre `&[u8]`), o mapeamento real de um
segmento usa `Mapper::for_root`, que caminha tabelas de página via o alias
HHDM — exatamente como `kernel::paging` já documenta nos seus próprios
testes (`translate_rejects_misaligned_address_without_touching_cr3`): "a
caminhada real de tabelas só é verificável em boot real". `HHDM_OFFSET` não
está inicializado em `cargo test -p kernel --lib` (roda no host, sem
paginação real), então nenhuma tarefa deste grupo que chama `Mapper::map_new`
de fato pode ter sua verificação primária como teste unitário de host — só a
matemática pura (quais páginas um segmento cobre, que `PageFlags` resulta de
quais `p_flags`, se dois intervalos de página se sobrepõem) é testável sem
hardware. A verificação do mapeamento real fica para a tarefa 4.2 (boot real
via QEMU), mesmo padrão já estabelecido pelo restante do kernel.

- [x] 2.1 Implementar funções puras auxiliares em `kernel::elf`: `segment_page_range(p_vaddr, p_memsz) -> (u64, u64)` (primeira página/página-limite, truncando para `PAGE_SIZE`), `segment_page_flags(p_flags: u32) -> PageFlags` (`PF_W`→`writable`, `PF_X`→`executable`), e `page_ranges_overlap((u64, u64), (u64, u64)) -> bool` — verificar com `cargo test -p kernel --lib` cobrindo limites exatos de página, cada combinação de `p_flags` (R, RW, RX) e pares de intervalos que se sobrepõem/não se sobrepõem
- [x] 2.2 Implementar o mapeamento real de um segmento `PT_LOAD` para um `AddressSpace` (via `Mapper::for_root`, usando `segment_page_range`/`segment_page_flags` de 2.1): para cada página nova, **zerar a página inteira** via o alias HHDM do frame recém-alocado (`Mapper::map_new` não zera — achado em design.md, "Decisões — Zeragem explícita do BSS") antes de copiar por cima os bytes do arquivo que caem nela (até `p_filesz`, nunca além) — sem teste de host possível (ver nota acima); verificação real na tarefa 4.2
- [x] 2.3 Antes de mapear, checar sobreposição entre os intervalos de página (`page_ranges_overlap`, tarefa 2.1) de todos os segmentos `PT_LOAD` do executável e propagar como `ElfError` (sem mapear nada) quando houver — verificar com o teste unitário de `page_ranges_overlap` (tarefa 2.1) mais um teste que monta um `ElfHeader`/`ProgramHeader` sintéticos com dois `PT_LOAD` sobrepostos e confirma que a função de carregamento rejeita antes de chamar qualquer `Mapper`
- [x] 2.4 Implementar `kernel::scheduler::spawn_from_elf<S: FrameStore>(frame_allocator, fs: &Fat32Filesystem, path: &str, user_stack_virt_base: u64, user_stack_size: u64) -> Result<ThreadId, ElfError>` (nome análogo a `spawn_user_demo`, mas a partir de um arquivo real — ver design.md, "`load_and_spawn` vive em `kernel::scheduler`"): lê o arquivo via `crate::storage::with_disk(|disk| fs.read_file(disk, path))` (mesmo padrão de `kernel::fat32_demo::run`, sem exigir que o chamador passe o disco), valida o cabeçalho (tarefa 1.2), checa sobreposição (tarefa 2.3), mapeia todos os `PT_LOAD` (tarefa 2.2) sobre um `AddressSpace` de `Process::begin_new_user`, e finaliza com `Process::finish_new_user` usando `e_entry` como ponto de entrada — registrar o processo e a thread exatamente como `spawn_user_demo` já faz (`PROCESSES`, `register_thread`, ambos já acessíveis dentro do próprio módulo `scheduler`) — sem teste de host possível para o caminho de sucesso; verificação real na tarefa 4.2
- [x] 2.5 Garantir, por construção, que nenhum caminho de erro (arquivo ilegível, formato inválido, sobreposição, falha de mapeamento/memória) chama `register_thread`/`PROCESSES.lock().push` — toda falha deve retornar `Err` antes desses dois pontos, que só podem ser alcançados depois que **todos** os segmentos foram mapeados com sucesso — verificar lendo o fluxo de controle de `load_and_spawn` (o `?` de cada etapa de mapeamento precede qualquer chamada a `finish_new_user`/`register_thread`/`PROCESSES`) e confirmando que o smoke test de boot (tarefa 4.3) continua passando sem nenhum processo "fantasma" visível

## 3. Programa de teste ELF real

- [x] 3.1 Criar um segundo binário Rust `#![no_std]`/`#![no_main]` (ex. `kernel/src/bin/elf_test_program.rs` ou crate irmã, conforme a estrutura de build existente) que faz a mesma sequência de teste de `kernel::user_demo` (escrever uma mensagem fixa via `syscall` de escrita, consultar ticks, repetir) — compilar como ELF64 estático não-PIE para o target `x86_64-java_os.json`, com endereço de carga fixo via linker script, e verificar com `cargo kernel-build`-equivalente que o binário resultante é `ET_EXEC` (ex. `readelf -h`)
- [x] 3.2 Estender `cargo xtask build-image` para copiar o binário compilado de 3.1 para a partição FAT32 da imagem de disco (`boot/dist/java-os.img`) num caminho fixo (ex. `/bin/test.elf`) — verificar lendo o arquivo de volta da imagem montada (ex. via `mtools`/`mdir`) e comparando bytes com o binário compilado
- [x] 3.3 Atualizar `README.md` com o novo caminho do binário de teste na imagem e o comando usado para verificá-lo, documentando a manutenção feita (conforme exigido pelas regras gerais do projeto)

## 4. Verificação em boot real

- [x] 4.1 Adicionar um ponto de disparo em `kernel_main` (análogo a `scheduler::spawn_user_demo`) que chama `kernel::scheduler::spawn_from_elf` para `/bin/test.elf` após o sistema de arquivos estar montado — verificar que o kernel builda sem erros (`cargo kernel-build`)
- [x] 4.2 Rodar `cargo xtask run -- -serial stdio -display none` em boot real e confirmar, pela saída serial, que a mensagem do programa de teste aparece repetidamente (prova de que o carregador leu o ELF do disco, mapeou os segmentos e transferiu controle para `e_entry` com sucesso) — documentar o resultado observado no `design.md` da change, seção "Achados durante a implementação" (criar a seção se ainda não existir), como já é costume nas changes anteriores
- [x] 4.3 Rodar `cargo xtask test` (smoke test automatizado) e confirmar que ainda passa com o novo ponto de disparo adicionado, garantindo que o carregamento do executável de teste não introduziu regressão no boot nem trava o kernel

## Workflow follow-up

- Gerar o relatório de migrações/modificações de banco de dados exigido antes de concluir a change — não se aplica aqui (nenhuma modificação de banco de dados nesta change), mas registrar essa constatação explicitamente ao fechar a change, conforme as regras gerais do projeto.
- Arquivar a change (`openspec-archive-change`) depois que as tarefas acima estiverem concluídas e verificadas em boot real.
