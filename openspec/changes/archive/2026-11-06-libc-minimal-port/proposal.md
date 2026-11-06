# Proposal

## Why

O HotSpot é C++ e assume uma libc/libstdc++ hospedeira completa por baixo (`malloc`, `string.h`, `pthread_*`, `dlopen`, `libm`) — não consome syscalls diretamente. Hoje nenhuma libc existe no Java OS: todo código de modo usuário do projeto (`elf_test_program`, as bibliotecas de teste do linker dinâmico) é Rust `#![no_std]` falando direto com as syscalls. A change `jvm-posix-compat-strategy` decidiu e inventariou as primitivas de *kernel* necessárias (`mmap`, `pthread_mutex`, `clock_gettime`, `dlopen`) mas nunca endereçou como código C/C++ padrão chegaria a elas — esse gap só foi descoberto agora, ao tentar escopar o próximo passo concreto do porte do HotSpot (um esqueleto `os_javaos`), e bloqueia esse esqueleto antes mesmo dele poder compilar. Esta change entrega a libc C mínima que faltava, tornando-se a nova primeira sub-change concreta do porte do HotSpot.

## What Changes

- Vendorização local do código-fonte do `newlib` (não versionado no Git, mesmo padrão de `boot/limine` — ver README), numa versão pinada.
- Implementação dos stubs de retarget do newlib (`_read`, `_write`, `_open`, `_close`, `_lseek`, `_sbrk`, `_fstat`, `_isatty`, `_exit`, `_kill`, `_getpid`) sobre as syscalls já existentes do kernel (`SYSCALL_READ`/`WRITE`/`OPEN`/`CLOSE` para E/S de arquivo; `SYSCALL_VM_RESERVE`+`SYSCALL_VM_COMMIT` para crescimento de heap via `_sbrk`).
- **Nova chamada de sistema `SYSCALL_EXIT`**: gap descoberto durante a pesquisa desta change — não existe hoje nenhuma forma de um processo/thread de modo usuário terminar (todo programa de teste do projeto até agora só faz loop infinito). Sem isso, o stub `_exit` do newlib (chamado por `exit()`/`abort()`/retorno de `main`) não tem o que invocar.
- Suporte no escalonador (`kernel::scheduler`) para parar de re-agendar permanentemente uma thread que chamou `SYSCALL_EXIT` — sem liberar seus recursos (mesma disciplina de "leak aceitável por enquanto" já usada para `kernel::pthread_handles`/`kernel::file_handles`, que também nunca destroem objetos completamente).
- Um wrapper fino sobre o GCC do host (mesma arquitetura x86_64 do alvo — nenhum cross-compilador dedicado precisa ser compilado do zero) para compilar o newlib retargetado e um programa C de verificação.
- Verificação em boot real: um programa C real (não Rust), carregado via o carregador ELF já existente (`kernel::elf`/`spawn_from_elf`), que aloca memória (`malloc`), escreve no console, lê um arquivo do disco, e termina via `exit()` — provando a cadeia completa.
- **Achado durante a implementação, mudando o escopo**: a ABI x86_64 não tem um modo "soft-float" utilizável sem SSE (o GCC recusa compilar qualquer função com `float`/`double` por valor sob `-mno-sse`, exigido pela limitação de FPU/XMM do escalonador — ver design.md). Esta libc, portanto, **não inclui nenhum suporte a ponto flutuante** (`libm`, `%f` em `printf`/`scanf`, `strtod`/`atof` reais) — bloqueador conhecido para quando o HotSpot precisar de `double`/`float`, tipos primitivos Java.

## Capabilities

### New Capabilities

- `libc-runtime`: a libc C mínima (newlib retargetado) disponível para programas de modo usuário do Java OS — alocação dinâmica de memória, E/S de arquivo via as funções padrão de `stdio.h`/`unistd.h`, e término de processo via `exit()`, todas sobre as syscalls do kernel já existentes.

### Modified Capabilities

- `kernel-syscalls`: adiciona `SYSCALL_EXIT`, a chamada de sistema que termina a thread/processo chamador.
- `kernel-scheduler`: adiciona o comportamento de nunca mais re-agendar uma thread que terminou via `SYSCALL_EXIT`.

## Impact

- **Código afetado**: novo diretório vendorizado para o código-fonte do newlib (não versionado no Git); novo módulo/crate de stubs de retarget (glue entre newlib e as syscalls); `kernel::syscall` (nova `SYSCALL_EXIT` e `sys_exit`); `kernel::scheduler` (`schedule_next` passa a poder omitir uma thread terminada da `READY_QUEUE` em vez de sempre devolvê-la via `push_back`); `xtask` (nova etapa de build para compilar o newlib retargetado e o programa C de verificação, e copiá-lo para a imagem de disco).
- **Dependências novas**: um wrapper fino sobre o GCC já instalado no ambiente de desenvolvimento (host e alvo são ambos x86_64 — nenhum cross-compilador dedicado precisa ser compilado do zero, ver design.md) e o código-fonte do newlib, vendorizado localmente fora do controle de versão, mesmo tratamento já dado ao Limine.
- **Compatibilidade**: puramente aditivo no lado das syscalls — nenhuma chamada existente muda de número, argumentos ou retorno.
- **Documentação**: `docs/architecture/overview.md` precisa de uma nova entrada (seção 4 para `SYSCALL_EXIT`; seção 6, que hoje descreve só as primitivas de kernel do HotSpot, ganha uma subseção sobre a camada de libc C) e o `README.md` precisa documentar o novo pré-requisito de toolchain (cross-compilador C), mesmo padrão das seções já existentes sobre Rust/Limine/QEMU.
