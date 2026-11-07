# Design

## Context

Hoje `userland_c/glue.c` implementa os *syscall stubs* não-reentrantes que o newlib precisa (`read`/`write`/`open`/`close`/`lseek`/`sbrk`/`fstat`/`isatty`/`_exit`/`kill`/`getpid` — ver change `libc-minimal-port` arquivada), e `scripts/cross/x86_64-elf-gcc` é um wrapper fino sobre o GCC do host (host e alvo são ambos x86_64 — nenhum cross-compilador dedicado foi construído) com as flags que suprimem as suposições "hospedadas" do GCC e desabilitam SSE/AVX2 (mantendo SSE/SSE2/SSE3/SSSE3/SSE4.x habilitados desde a change `kernel-fpu-context-switch`).

**Achado confirmado empiricamente nesta sessão**: compilando um `.cpp` de teste (classe com herança, método virtual sobrescrito, `new`/`delete`) com o GCC do host e as flags `-ffreestanding -fno-exceptions -fno-rtti -fno-use-cxa-atexit -nostdlib`, a compilação terminou sem nenhum erro. O link falhou só por dois símbolos indefinidos: `operator new(unsigned long)` e `operator delete(void*, unsigned long)` — nenhum `__cxa_pure_virtual`, `__dso_handle` ou símbolo de exceção/RTTI apareceu como indefinido nesse teste específico (sem herdar de uma classe com método virtual puro não sobrescrito, nem usar `try`/`catch`/`dynamic_cast`, nem objetos globais com construtor não-trivial).

Ver proposal.md para motivação; ver spec delta de `cxx-runtime` para o contrato comportamental completo.

## Goals / Non-Goals

**Goals:**
- Um programa C++ com herança/polimorfismo, alocando e liberando objetos via `new`/`delete`, compila, linka e roda corretamente sobre o Java OS.

**Non-Goals:**
- Exceções C++ (`try`/`catch`/`throw`) e RTTI (`dynamic_cast`, `typeid`) — desligadas deliberadamente (`-fno-exceptions -fno-rtti`). Reabilitá-las, se o HotSpot precisar, é uma sub-change própria (exigiria a tabela de *unwind*/`libgcc_eh` e o runtime de RTTI do Itanium ABI, infraestrutura bem maior que o escopo aqui).
- Construtores/destrutores de objetos globais/estáticos — `-fno-use-cxa-atexit` suprime a necessidade de `__cxa_atexit`/`__dso_handle` de verdade; nenhum programa desta change usa objetos globais com construtor não-trivial. Caso um programa futuro precise, isso exige implementar `__cxa_atexit`/`__dso_handle` e invocar os construtores globais a partir do `crt0`.
- `libstdc++`/STL (`std::vector`, `std::string`, `std::unique_ptr`, etc.) — o HotSpot usa boa parte disso, mas decidir o que portar (ou reimplementar) fica para quando o código-fonte real do HotSpot for vendorizado e suas dependências concretas forem conhecidas, não antes.
- O esqueleto `os_javaos` ou qualquer código do HotSpot em si.

## Decisions

### Wrapper `x86_64-elf-g++`, mesmo padrão do `x86_64-elf-gcc`

Mesmo racional já estabelecido (`libc-minimal-port`, design.md arquivado, "Wrapper fino sobre o GCC do host"): host e alvo são ambos x86_64, então um `g++` cross-compilado dedicado não é necessário. O novo wrapper herda todas as flags do wrapper de C (`-ffreestanding -nostdlib -fno-pie -no-pie -fno-stack-protector`, sem-SSE) e adiciona as três flags específicas de C++ do achado acima.

### `operator new`/`operator delete` delegam para `malloc`/`free` da libc

Mesma camada de dependência de um sistema hospedado de verdade: a libc C já fornece um alocador funcional (`malloc`/`free`, por sua vez sobre `sbrk`/`SYSCALL_VM_RESERVE`/`SYSCALL_VM_COMMIT` já existentes — ver `libc-minimal-port`). O runtime C++ não precisa (e não deve) reimplementar um alocador próprio — só prover a interface `operator new`/`operator delete` que o compilador espera, delegando diretamente. `operator new` SHALL propagar falha de `malloc` (ponteiro nulo) como seria esperado num ambiente sem exceções habilitadas — sem lançar `std::bad_alloc` (que exigiria exceções, fora de escopo); um `new` que falha e retorna nulo é aceitável nesta etapa mínima (comportamento de `new(std::nothrow)`, não do `new` padrão da linguagem, mas a única opção coerente sem exceções).

### `__cxa_pure_virtual`: stub defensivo, nunca chamado no caminho feliz

Faz parte do Itanium C++ ABI (usado por GCC/Clang em x86_64): toda vtable de uma classe com método virtual puro aponta, para esse slot, para `__cxa_pure_virtual` até ser sobrescrita por uma classe derivada concreta. Chamá-lo de verdade só acontece se uma classe abstrata for instanciada incorretamente (erro de construção de objeto, nunca deveria acontecer em código correto) — implementado como `hlt`-loop (mesmo padrão de canário de erro já usado em `kernel::exit_demo`), não como algo recuperável.

## Risks / Trade-offs

- **[Risco] `operator new` sem exceção, devolvendo o que `malloc` devolver (inclusive nulo em falha)** → Mitigação: aceito como Non-Goal explícito; qualquer programa de teste desta change verifica o ponteiro antes de usar, mesma disciplina de C (`malloc` também pode falhar e já é tratado assim em `userland_c/libc_test_program.c`).
- **[Risco] `-fno-use-cxa-atexit` silenciosamente faz objetos globais com destrutor não-trivial nunca serem destruídos (vazamento, não crash)** → Mitigação: aceito; nenhum programa desta change usa objetos globais com destrutor não-trivial — documentado como Non-Goal, revisitar se/quando necessário.
- **[Risco] HotSpot pode depender de exceções ou RTTI em algum ponto não óbvio, inviabilizando `-fno-exceptions -fno-rtti` mais adiante** → Não mitigado aqui — só será conhecido quando o código-fonte real do HotSpot for vendorizado; decisão de uma sub-change futura, documentada como Non-Goal explícito.

## Migration Plan

Puramente aditivo — nenhum arquivo C ou Rust existente muda de comportamento. Nenhum passo de migração necessário.
