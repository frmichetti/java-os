# Tasks

## 1. Wrappers de cross-compilação sem flags de SSE desabilitado

- [x] 1.1 Remover `-mno-sse -mno-sse2 -mno-sse3 -mno-mmx -mno-80387` de
      `scripts/cross/x86_64-elf-gcc` e atualizar o comentário do arquivo (a nota
      "mesmas features do target Rust `x86_64-java_os_user.json`" fica desatualizada
      — esse target já habilita SSE desde `kernel-fpu-context-switch`; o comentário
      sobre `-msoft-float` não ter efeito permanece válido como contexto histórico,
      mas deixar claro que SSE agora está habilitado de propósito) — verificar
      compilando um arquivo trivial com `float`/`double` por valor sem erro.
- [x] 1.2 Remover as mesmas flags de `scripts/cross/x86_64-elf-g++` e atualizar seu
      comentário (hoje referencia explicitamente esta change pelo texto do
      `ROADMAP.md` — atualizar para "feito", não mais pendente) — verificar
      compilando um `.cpp` trivial com `float`/`double` por valor sem erro.

## 2. Rebuild do newlib com libm/conversão float incluídos

- [x] 2.1 Rodar `scripts/cross/build-newlib.sh` de novo (idempotente — não
      reconfigura, já que o `Makefile` existe) e confirmar que `libc.a` resultante
      agora contém objetos de `libm/` e de `libc/stdlib/atof.o`/`strtod.o`/`dtoa.o`
      (`ar t libc.a | grep -E 'atof|strtod|dtoa'`, e uma contagem de objetos de
      `libm/` antes/depois). **Achados reais (2)**: (a) a detecção de `NEWLIB_SRC`
      (`find ... -name 'newlib-*' | sort -V | tail -1`) também casava com
      `vendor/newlib-build`/`vendor/newlib-install` (criados por este próprio
      script, nunca populados) — numa build limpa do zero, `sort -V | tail -1`
      escolhia `newlib-install` em vez do código-fonte real; nunca disparava em uso
      normal porque o `Makefile` já existente pulava o `configure`. Mesmo bug
      existia em `xtask::vendored_newlib_src_dir` (ordem de `fs::read_dir` não
      garantida). Corrigidos os dois exigindo um dígito logo após `newlib-`. (b) A
      etapa de arquivamento só olhava `.o` de `newlib/libc`, nunca de `newlib/libm`
      — `libm` (sin/cos/sqrt/etc.) nunca entrava no `libc.a` final, mesmo quando
      compilava com sucesso; corrigido arquivando os `.o` das duas árvores juntos,
      no caminho que `xtask` espera (`newlib/libc.a`, não `newlib/libc/libc.a`).
      Confirmado via build limpa do zero: 1070 objetos arquivados (`libc`+`libm`),
      `nm libc.a` confirma `atof`/`strtod`/`sin`/`cos`/`sqrt`/`pow` presentes.
- [x] 2.2 Verificar se algum arquivo de `libm`/`libc/stdlib`/`libc/time` ainda falha
      a compilação durante essa build, por razão não relacionada a SSE — se
      nenhum falhar, documentar isso e avaliar remover `-k` da invocação de `make`
      em `build-newlib.sh` (ou mantê-la como rede de segurança, documentando a
      decisão); se algum falhar, documentar explicitamente qual e por quê (mesma
      prática de `libc-minimal-port`), sem tentar corrigi-lo nesta change a menos
      que seja trivial e dentro do escopo de ponto flutuante. **Resultado real**:
      nenhum arquivo de `newlib/libc`/`newlib/libm` falhou — a única falha
      remanescente é em `libgloss` (`libgloss/doc/porting.info`, exige `makeinfo`/
      pacote `texinfo`, ausente no ambiente de build), um alvo nunca usado por este
      projeto (`libc.a` vem só de `newlib/libc`+`newlib/libm`). `-k` mantido na
      invocação de `make`, mas agora por essa razão não relacionada a ponto
      flutuante — documentado no comentário do script.
- [x] 2.3 Atualizar o comentário de topo de `scripts/cross/build-newlib.sh` — hoje
      descreve exatamente a limitação de SSE que esta change remove; atualizar para
      refletir o estado real pós-implementação (o que ainda fica de fora, se algo
      ficar, conforme a tarefa 2.2).

## 3. Verificação em boot real

- [x] 3.1 Criar `userland_c/libm_test_program.c` (mesmo padrão de
      `javaos_pthread_test_program.c`): exercita uma expressão aritmética com
      `double` (ex.: `2.5 + 1.5 == 4.0`), `sqrt(16.0)` (deve ser `4.0`, dentro de uma
      tolerância pequena), `sin(0.0)` (deve ser `0.0`, dentro de tolerância), e
      `strtod("3.5", NULL)`/`atof("3.5")` (devem corresponder a `3.5`, dentro de
      tolerância) — cada cenário reporta `P`/`F` numa única mensagem via `write()`,
      sem usar `printf("%f", ...)` (continua sem suporte — ver design.md).
- [x] 3.2 Adicionar `build_libm_test_program` a `xtask/src/main.rs` (mesmo corpo de
      `build_javaos_pthread_test_program`, linkando também contra o `libc.a`
      reconstruído — sem bibliotecas adicionais, `libm` está arquivada dentro do
      mesmo `libc.a` por `build-newlib.sh`), chamar a partir de `build_image`, e
      copiar o ELF resultante para um nome curto 8.3 válido (ex. `::/bin/libm.elf`,
      ver achado de nome de arquivo de `os-javaos-platform-sync`) — verificar que
      `cargo xtask build-image` produz o arquivo sem erro.
- [x] 3.3 Adicionar `LIBM_TEST_PROGRAM_STACK_VIRT`/`_STACK_SIZE` a
      `kernel/src/scheduler.rs` (endereço distinto dos já existentes, próximo livre
      após `0x0000_0a00_0000_0000`) e carregar `/bin/libm.elf` via
      `kernel::scheduler::spawn_from_elf` em `kernel/src/main.rs`, mesma disciplina
      de erro explícito (nunca pânico) dos carregamentos ELF já existentes ali —
      verificar via `cargo xtask run` em QEMU que a mensagem de resultado aparece no
      console serial com todos os cenários `P`, e que nenhum `EXCECAO`/pânico ocorre.
      **Achado real crítico, fora do plano original**: a primeira tentativa travou o
      kernel inteiro com `EXCECAO #GP (general protection fault)` dentro de
      `libm_test_program` — causa raiz em `userland_c/crt0.S`, não no kernel nem na
      libc: `_start` fazia `call main` direto, herdando de
      `kernel::process::create_user_thread` um `RSP` ajustado para a convenção de
      "função chamada" da ABI (`RSP % 16 == 8`, correta para os pontos de entrada em
      assembly deste projeto, que são efetivamente funções). `_start`, porém, é o
      ponto de entrada do processo (`e_entry` do ELF), que a ABI exige começar com
      `RSP` alinhado a 16 bytes — sem esse ajuste, `main` recebia `RSP` desalinhado
      em 8 bytes, invisível até o compilador gerar a primeira instrução SSE alinhada
      (`movaps`) de todo o projeto. Corrigido com `and $-16, %rsp` logo no início de
      `_start` (mesma convenção de `crt0`/`crt1.o` reais como glibc/musl) — não exige
      nenhuma mudança no kernel. Resultado após a correção:
      `libm_test_program: resultado PPPPP`, sem nenhuma exceção.
- [x] 3.4 Confirmar, no mesmo log de boot, que os programas de teste já existentes
      (`libctest.elf`/`cxxtest.elf`/`javaosvm.elf`/`jpthread.elf`) continuam
      reportando exatamente os mesmos resultados de antes (nenhuma regressão
      introduzida pela mudança de flags do toolchain compartilhado). Confirmado:
      todos os 6 programas de teste C/C++ pré-existentes (incluindo
      `user_posix_demo`/`file_syscalls_demo`/`fpu_demo_a`/`fpu_demo_b`) reportaram
      exatamente os mesmos padrões `P`/`F` de antes desta change.

## 4. Documentação

- [x] 4.1 Adicionar a este change (`design.md`, "Resultado real da implementação")
      a confirmação do que foi verificado em boot real e o relato de qualquer achado
      real durante a implementação (incluindo o resultado da tarefa 2.2).
- [x] 4.2 Atualizar `ROADMAP.md` — mover o item concluído ("Reconstruir
      `libm`/conversão float...") da seção "Próximo passo imediato" para a seção de
      implementado, e apontar o próximo item real (primeiro item do roadmap médio
      prazo restante) como a continuação — verificar que o documento permanece
      consistente (sem referência pendente a um "próximo passo" já concluído).

## Workflow follow-up

- Arquivar esta change (`openspec archive libc-float-math`) depois que a verificação
  em boot real (tarefas 3.3/3.4) e a confirmação de que `libc.a` inclui `libm`/
  conversão float (tarefa 2.1) estiverem confirmadas.
