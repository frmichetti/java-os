# Design

## Context

Hoje todo código de modo usuário do projeto é Rust `#![no_std]` compilado contra `x86_64-java_os_user.json` (target customizado): **sem SSE/AVX, ponto flutuante só via software (`+soft-float`)**, executável estático não-PIE (`relocation-model: static`, `position-independent-executables: false`). A ausência de SSE não é um detalhe de performance — é uma exigência de corretude: `kernel::scheduler`/`kernel::process::Context` não salvam nem restauram estado de FPU/XMM na troca de contexto (ver `docs/architecture/overview.md`, seção 3). Qualquer código que use ponto flutuante via SSE seria silenciosamente corrompido pela primeira preempção. `kernel::elf`/`kernel::scheduler::spawn_from_elf` só aceitam `ET_EXEC` (estático, sem relocations) — `ET_DYN` (bibliotecas `.so`) é tratado por um caminho totalmente diferente (`kernel::dylib`).

As syscalls relevantes já existem e não mudam nesta change: `SYSCALL_OPEN`/`READ`/`CLOSE` (18-20, change `kernel-file-syscalls`), `SYSCALL_VM_RESERVE`/`COMMIT` (2-3, change `kernel-jvm-posix-syscalls`), `SYSCALL_WRITE` (0). `kernel::scheduler` mantém uma `ReadyQueue` (`VecDeque<ThreadId>`) e `CONTEXTS: Vec<Context>` indexado por `ThreadId`; `schedule_next` sempre faz `pop_front` da thread corrente e, ao fim do tick, `push_back` dela de volta (nunca a descarta) — ver `kernel/src/scheduler.rs`.

Ver proposal.md para motivação; ver specs delta de `libc-runtime`/`kernel-syscalls`/`kernel-scheduler` para o contrato comportamental completo.

## Goals / Non-Goals

**Goals:**
- Um programa C real, compilado por um cross-compilador dedicado e linkado contra um newlib retargetado, consegue alocar memória, ler um arquivo, escrever no console e terminar de forma limpa — rodando sobre o carregador ELF já existente.
- Toda a cadeia de build (cross-compilador, newlib, programa de verificação) respeita a mesma ABI de ponto flutuante do target Rust existente (sem SSE, soft-float) e o mesmo modelo de executável (estático, não-PIE).

**Non-Goals:**
- Programas C multi-thread: os ganchos de reentrância do `malloc` do newlib (`__malloc_lock`/`__malloc_unlock`) viram no-ops nesta change — seguro só porque o programa de verificação é single-thread. Um C multi-thread de verdade precisa da sub-change futura de pthreads em libc (ver proposal.md, Non-Goals).
- `libstdc++`/C++ — esta change entrega só a libc C; o esqueleto `os_javaos` do HotSpot (C++) é a sub-change seguinte.
- **Ponto flutuante** (`float`/`double`) em qualquer parte desta libc: `libm`, `%f`/`%e`/`%g` em `printf`/`scanf`, `strtod`/`atof` reais — achado durante a implementação (ver Decisões): a ABI x86_64 não tem um modo "soft-float" utilizável sem SSE, então essas partes do newlib simplesmente não compilam sob as flags exigidas pela limitação de FPU/XMM do escalonador. Suporte real fica bloqueado até uma change futura que ensine o kernel a salvar/restaurar estado de FPU/XMM na troca de contexto — necessário de qualquer forma antes do HotSpot (que usa `double`/`float` como tipos primitivos Java).
- Qualquer coisa além do conjunto mínimo de stubs de retarget do newlib — sem rede, sem sinais reais, sem múltiplos descritores de diretório, sem variáveis de ambiente.

## Decisions

### newlib, não musl nem uma libc escrita do zero

Pesquisado via busca na web (sem acesso a um ambiente real de compilação do HotSpot nesta sessão, mas confirmando um padrão bem estabelecido na comunidade OSDev — ver fontes no final): **newlib** foi desenhado desde o início para ser retargetado via um conjunto pequeno e bem definido de *syscall stubs* que o próprio projeto hospedeiro implementa — historicamente por volta de 11 a 17 funções (`_read`, `_write`, `_open`, `_close`, `_lseek`, `_sbrk`, `_fstat`, `_isatty`, `_exit`, `_kill`, `_getpid`), cada uma convertida para o que o kernel alvo realmente oferece.

**Alternativa rejeitada — musl**: musl assume internamente a ABI de syscalls do Linux (números e semântica de `read`/`write`/`mmap`/`clone`/`futex` etc. do kernel Linux) — portá-lo exigiria reescrever sua camada arquitetura-específica de syscall (`arch/<arch>/syscall_arch.h` e о backend de TLS/threads) para uma ABI que não tem relação nenhuma com a nossa, um trabalho de cirurgia interna bem mais profundo do que o pequeno conjunto de stubs do newlib.

**Alternativa rejeitada — escrever uma libc do zero**: reimplementaria `malloc`/`printf`/`string.h`/`stdio.h` (centenas de funções testadas e já portadas por décadas de uso) só para evitar um punhado de arquivos de retarget — esforço muito maior pelo mesmo resultado.

### Sem `libc/sys/<alvo>` dentro da árvore do newlib — stubs ligados externamente ao `libc.a` genérico

**Achado confirmado inspecionando o código-fonte vendorizado** (`newlib-4.6.0.20260123/newlib/configure.host`): `sys_dir` (a variável que escolhe qual subdiretório de `libc/sys/<nome>` entra na build) só é preenchida para combinações específicas de triplo-alvo já conhecidas pelo newlib (`arm-*-elf`, `*-*-rtems*`, etc.); para um triplo sem correspondência (como `x86_64-elf`, usado por esta change), `sys_dir` permanece vazio e o newlib builda só a camada genérica (`newlib/libc/syscalls/` + `newlib/libc/reent/`) — cuja cadeia completa (`read()` → `_read_r()` → `_read()`, confirmado lendo `libc/syscalls/sysread.c` e `libc/reent/readr.c`) termina em funções não-reentrantes (`_read`, `_write`, `_open`, `_close`, `_lseek`, `_sbrk`, `_fstat`, `_isatty`, `_exit`, `_kill`, `_getpid`) que ficam **indefinidas dentro de `libc.a`** — exatamente o conjunto de stubs já identificado na pesquisa original, só que resolvidas por um arquivo externo nosso no link final de cada programa, não por uma integração dentro da árvore do newlib.

**Decisão**: não criar nenhum `libc/sys/javaos` dentro do newlib. Em vez disso, buildar o newlib "genérico" (`--target=x86_64-elf`, sem `sys_dir`) e escrever nosso próprio arquivo de glue (`_read`/`_write`/.../`_getpid`, fora da árvore do newlib, no próprio repositório do Java OS) mais um `crt0` mínimo (zera `bss`, chama `main`, chama `_exit` com o valor de retorno) — ambos compilados com o wrapper da tarefa 2 e linkados, junto com `libc.a`, em cada programa C final. Mais simples e com superfície de mudança menor do que integrar um alvo novo na árvore do newlib (sem precisar reconfigurar/regenerar nada dentro dela).

**Alternativa descartada — integrar `libc/sys/javaos` na árvore do newlib** (plano original desta change antes deste achado): exigiria entender e reproduzir a estrutura de `configure.in`/`Makefile.inc`/`crt0` específica que o sistema de build do newlib espera para um alvo novo "de primeira classe" — trabalho real só compensador se outros alvos/arquiteturas fossem suportados pela mesma árvore vendorizada no futuro, o que não é o caso aqui (só x86_64, só Java OS).

### Wrapper fino sobre o GCC do host, não um binutils+GCC cross-compilado do zero

Nem GCC nem binutils têm um target nomeado para o Java OS (nem faria sentido criar um rio acima, sem patrocínio upstream). O caminho clássico da comunidade OSDev é construir um cross-compilador dedicado para o triplo genérico `x86_64-elf` a partir do código-fonte do binutils/GCC — mas essa construção do zero (historicamente dezenas de minutos, exige `autoconf`/`automake`/`texinfo`/`bison`/`flex`/`libgmp-dev`/`libmpfr-dev`/`libmpc-dev`) só é estritamente necessária quando a arquitetura de destino difere da do host. Aqui **ambas são x86_64** — o ambiente de desenvolvimento (WSL/Ubuntu) já tem um GCC nativo (confirmado: GCC 15.2.0) perfeitamente capaz de gerar o código-objeto correto.

**Decisão**: em vez de compilar um binutils+GCC inteiro, usar um **script wrapper fino** nomeado `x86_64-elf-gcc` (convenção de triplo que o sistema de build do newlib espera encontrar no `PATH`) que invoca o GCC do host já instalado com as flags que suprimem as suposições "hospedadas" (`-ffreestanding -nostdlib -fno-pie -fno-stack-protector`) e as de ponto flutuante da seção abaixo — sem baixar nem compilar nenhum código-fonte de binutils/GCC. `ar`/`ranlib` do host (`x86_64-elf-ar`/`x86_64-elf-ranlib`, mesmos wrappers triviais) resolvem a mesma convenção de nome para as demais ferramentas de binário que o newlib invoca.

**Alternativa rejeitada — construir um binutils+GCC dedicado do zero**: só se justificaria se o host fosse de arquitetura diferente do alvo, ou se precisássemos de uma versão de GCC/binutils diferente da já instalada por alguma razão específica (nenhuma identificada). Manter como opção de fallback documentada caso o wrapper se prove insuficiente (ex.: se alguma suposição do GCC do host sobre o ambiente hospedeiro não puder ser desligada por flag).

### Flags de ABI de ponto flutuante replicadas em toda a cadeia de build — e nenhum ponto flutuante na libc desta change

Como `kernel::process::Context`/`kernel::scheduler` não salvam/restauram estado de FPU/XMM na troca de contexto (ver Contexto), o wrapper de cross-compilador, o build do newlib e o programa de verificação SHALL ser construídos/invocados com as mesmas flags que já desabilitam SSE/AVX no target Rust existente (`-mno-sse -mno-sse2 -mno-sse3 -mno-mmx -mno-80387`).

**Achado confirmado experimentalmente durante esta change** (compilando um `.c` trivial com o wrapper de `x86_64-elf-gcc`): ao contrário do que o nome "soft-float" sugere, `-mno-sse` em x86_64 **não** faz o GCC gerar chamadas de biblioteca de emulação por software para `float`/`double` — a ABI SysV x86_64 não define nenhuma convenção alternativa de passagem/retorno desses tipos por valor em registradores inteiros (diferente de ARM, onde "soft-float" é uma ABI completa e suportada). O GCC simplesmente **recusa compilar** qualquer função com parâmetro ou retorno `float`/`double` por valor sob essas flags (`error: SSE register return with SSE disabled`).

**Consequência direta**: esta change **não pode** incluir nenhuma parte do newlib que use ponto flutuante — `libm` inteira, o suporte a `%f`/`%e`/`%g` em `printf`/`scanf`, e `strtod`/`atof` de verdade ficam fora desta build. Isso é aceitável porque nada no escopo desta change (os stubs de retarget, o programa C de verificação — malloc, E/S de arquivo, impressão de texto simples, término) usa ponto flutuante. Suporte real a `double`/`float` (exigido pelo próprio tipo primitivo Java `double`/`float`, e portanto inevitável para o HotSpot) depende de uma mudança bem maior e fora de escopo aqui: o kernel passar a salvar/restaurar estado de FPU/XMM na troca de contexto (`FXSAVE`/`FXRSTOR` em `kernel::process::Context`) — ver Non-Goals e Riscos.

**Segundo achado, mais sério do que o esperado, confirmado tentando buildar de verdade**: `--disable-newlib-io-float` só desliga `%f`/`%e`/`%g` em `printf`/`scanf` — não impede que `libm` inteira (todas as ~230 fontes de `libm/math`, `libm/common`, `libm/complex`, `libm/ld*`) nem um punhado de arquivos de `libc/stdlib` (`atof`/`atoff`/`strtod`/`strtodg`/`strtold`/`wcstod`/`wcstold`/`dtoa`/`gdtoa-gdtoa`/`mprec`/`drand48`/`erand48`/`ecvtbuf`/`efgcvt`/`__adjust`/`__exp10`/`__ten_mu`) e `libc/time/difftime.c` sejam *tentados* na build — e todos eles falham a compilação sob `-mno-sse` (mesmo erro de ABI da seção anterior; `difftime.c` falha só por `time.h` **declarar** `double difftime(...)`, sem nem chegar a compilar o corpo da função). Como isso impede o alvo `all` padrão do `make` de alcançar o passo de arquivar `libc.a` (uma dependência dele falhou), a build desta change usa `make -k` (continua apesar de erros — ver `scripts/cross/build-newlib.sh`) e depois **arquiva manualmente** só os `.o` de `libc/` que de fato compilaram (confirmado: 586 objetos, cobrindo `stdio`/`string`/`reent`/`signal`/`errno`/`misc`/`locale`/`ssp`/`machine/x86_64` por completo) em vez de depender do passo de arquivamento do `make` — sem tocar nenhum arquivo do newlib vendorizado (a árvore-fonte permanece intacta, só o `Makefile` gerado é "desobedecido" no passo final).

### `_sbrk` sobre `SYSCALL_VM_RESERVE`/`SYSCALL_VM_COMMIT` já existentes

A rotina de inicialização do programa (`crt0`) reserva uma janela de heap de tamanho fixo (ex.: 16 MiB) uma única vez via `SYSCALL_VM_RESERVE`, guardando o endereço base e um cursor de "break" (inicialmente igual à base). `_sbrk(incremento)` comete, via `SYSCALL_VM_COMMIT`, só as páginas adicionais necessárias para cobrir o novo cursor (arredondado para página), avança o cursor, e devolve o endereço do início do incremento — ou falha (`errno = ENOMEM`) se o incremento ultrapassar a janela reservada. Nenhuma syscall nova é necessária para isso.

### `SYSCALL_EXIT` (21) e remoção permanente de `ReadyQueue`

Próximo número livre após `SYSCALL_CLOSE` (20, change `kernel-file-syscalls`): `SYSCALL_EXIT = 21`, sem argumentos, nunca retorna. Implementação: um novo `static TERMINATED: Spinlock<Vec<bool>>` (indexado por `ThreadId`, mesmo padrão de `CONTEXTS`) marca a thread corrente como terminada; `schedule_next`, ao fazer `pop_front` da thread que estava rodando, consulta `TERMINATED` antes de decidir se faz `push_back` dela — se terminada, ela simplesmente não volta à fila (ver spec delta `kernel-scheduler`). `sys_exit` marca a terminação e entra num laço `sti`/`hlt` (mesmo padrão de `kernel::idle_loop`): como a thread nunca mais é posta de volta na `ReadyQueue`, essa última fatia de execução nunca é retomada — o laço só existe para manter a CPU ociosa com segurança até o próximo tick trocar definitivamente para outra thread.

**Recursos não são liberados** (stack de kernel, entradas em `CONTEXTS`/`TERMINATED`, espaço de endereçamento do processo): mesma disciplina de "leak aceitável por enquanto" já aceita para `kernel::pthread_handles`/`kernel::file_handles` (que também nunca destroem objetos completamente) e para `CONTEXTS`/`PROCESSES` em geral (comentário existente em `kernel::scheduler`: "sem remoção — esta change não implementa encerramento de thread"). Liberação de verdade fica para uma change futura, se/quando necessário.

## Risks / Trade-offs

- **[Risco] O wrapper fino sobre o GCC do host pode não suprimir alguma suposição "hospedada" só com flags** (descoberto só na prática, ao tentar compilar o newlib de verdade) → Mitigação: se isso acontecer, o fallback documentado é a construção de um binutils+GCC dedicado para `x86_64-elf` (caminho clássico da comunidade OSDev, mais pesado mas garantidamente livre de qualquer suposição do host) — não bloqueia a decisão inicial, só adia o custo para se/quando necessário.
- **[Risco] A versão/convenções do GCC do host (15.2.0) podem mudar entre atualizações do WSL/Ubuntu, alterando sutilmente o código gerado** → Mitigação: aceito por ora (mesmo risco que qualquer outra ferramenta de host já usado pelo projeto, como `qemu-system-x86_64`/`mtools`, nenhum dos quais é vendorizado); documentar a versão usada no momento da implementação.
- **[Risco] Esquecer as flags de sem-SSE em qualquer ponto da cadeia corrompe estado de ponto flutuante silenciosamente na primeira preempção** (sem crash imediato, só corrupção silenciosa — o tipo de bug mais caro de depurar) → Mitigação: flags centralizadas no wrapper único (`scripts/cross/x86_64-elf-gcc`) compartilhado por todas as builds; tarefa de verificação inclui inspecionar o binário final (`objdump`) confirmando ausência de instruções SSE/x87.
- **[Risco] Qualquer trecho do newlib (ou código futuro) que use `float`/`double` por valor simplesmente não compila sob `-mno-sse`** (erro de compilação, não um comportamento sutil — ver Decisões) → Mitigação: newlib configurado para omitir as partes que usam ponto flutuante (`--disable-newlib-io-float` e equivalentes, excluindo `libm`); aceito como Non-Goal explícito desta change. Bloqueador conhecido e documentado para quando o HotSpot precisar de `double`/`float` de verdade.
- **[Risco] `_sbrk` sobre uma janela de `kernel::vmm` de tamanho fixo pode não ser suficiente para um programa que aloque muito mais que o esperado** → Mitigação: aceito por ora (janela generosa o bastante para o programa de verificação); revisitar só se um caso real exigir mais.
- **[Risco] Reentrância do `malloc` do newlib desabilitada (no-op)** → Mitigação: aceito explicitamente como Non-Goal; só é seguro enquanto nenhum programa C multi-thread existir (nenhum nesta change).

## Migration Plan

Puramente aditivo no lado do kernel (`SYSCALL_EXIT` é uma chamada nova, nenhuma existente muda). O cross-compilador e o newlib são novas dependências de build vendorizadas localmente, sem efeito em nada que já compila. Nenhum passo de migração necessário.

## Fontes consultadas

Pesquisa feita via busca na web (sem acesso a um ambiente de build real do HotSpot ou do newlib nesta sessão):
- Páginas da wiki do OSDev sobre porte do newlib para kernels próprios (padrão de ~17 stubs de syscall, triplo genérico `*-elf`, estrutura mínima de `libc/sys/<alvo>`).
- Discussões arquivadas de listas de e-mail do próprio newlib sobre os stubs `_read`/`_write`/`_sbrk`/`_exit` e suas variantes reentrantes (`_r`).
- Discussões sobre a dificuldade relativa de portar musl (acoplado à ABI de syscalls do Linux) vs. newlib para um kernel não-Linux, em fóruns de desenvolvimento de SO hobby.

Os nomes exatos de arquivo/função dentro do newlib devem ser reconfirmados contra o código-fonte real da versão escolhida no momento da implementação.
