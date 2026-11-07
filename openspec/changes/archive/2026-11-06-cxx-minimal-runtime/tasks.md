# Tasks

## 1. Wrapper de cross-compilador C++

- [x] 1.1 Criar `scripts/cross/x86_64-elf-g++`: wrapper fino sobre o `g++` do host com `-ffreestanding -nostdlib -fno-pie -no-pie -fno-stack-protector`, as mesmas flags de sem-SSE do wrapper de C (`-mno-sse -mno-sse2 -mno-sse3 -mno-mmx -mno-80387` — ver `scripts/cross/x86_64-elf-gcc`), e `-fno-exceptions -fno-rtti -fno-use-cxa-atexit`
- [x] 1.2 Verificar que `x86_64-elf-g++` compila um `.cpp` trivial com herança e método virtual sobrescrito (sem `new`/`delete` ainda) para um objeto válido, sem erro
- [x] 1.3 Verificar, via `objdump -d`, que nenhuma instrução SSE/x87 aparece no código gerado (mesma verificação já feita para o wrapper de C — confirma que as flags de ABI de ponto flutuante se propagam corretamente também para C++)

## 2. Runtime C++ mínimo (`userland_c/cxx_runtime.cpp`)

- [x] 2.1 Implementar `operator new(size_t)`/`operator new[](size_t)` delegando para `malloc` (declarado via `extern "C"` direto, mesma disciplina de `userland_c/glue.c` — sem incluir `<cstdlib>` do host) — propaga o que `malloc` devolver, inclusive nulo em falha (sem lançar `std::bad_alloc` — ver design.md, sem exceções habilitadas)
- [x] 2.2 Implementar `operator delete(void*)`/`operator delete(void*, size_t)`/`operator delete[](void*)`/`operator delete[](void*, size_t)` delegando para `free`
- [x] 2.3 Implementar `__cxa_pure_virtual` como stub defensivo (laço infinito, mesmo padrão de canário de erro de `kernel::exit_demo`) — documentado que não deveria ser alcançado em código correto
- [x] 2.4 Verificar que `userland_c/cxx_runtime.cpp` compila com o wrapper da tarefa 1 sem erro

## 3. Programa C++ de verificação e integração com `xtask`

- [x] 3.1 Escrever um programa C++ de teste (`userland_c/cxx_test_program.cpp`) que: define uma classe-base com um método virtual **puro**, uma classe derivada que o sobrescreve, aloca uma instância da derivada via `new` através de um ponteiro de classe-base, chama o método virtual e confirma que a implementação derivada foi executada (chegar sem travar em `__cxa_pure_virtual` já é parte da prova), escreve um padrão conhecido num campo do objeto para confirmar que a memória alocada é utilizável, libera via `delete` (exercitando o destrutor virtual), e reporta o resultado via `write` (mesmo padrão de `userland_c/libc_test_program.c`) antes de terminar
- [x] 3.2 Compilar e linkar esse programa com o wrapper da tarefa 1 + o runtime da tarefa 2 + `crt0.o`/`libc.a` já existentes (mesmo `linker.ld` de `userland_c/libc_test_program.c` — estático, não-PIE): `ET_EXEC` com dois `PT_LOAD` (R-E e RW) corretamente separados por página, confirmado via `readelf -h`/`-l`
- [x] 3.3 Estender `xtask::build_image` (`build_cxx_test_program`) para compilar o runtime C++/programa C++ e copiá-los para a imagem de disco (`::/bin/cxxtest.elf`), mesmo mecanismo já usado para `::/bin/libctest.elf`
- [x] 3.4 Conectar o carregamento desse executável em `kernel_main` (`kernel::scheduler::spawn_from_elf` com uma nova stack dedicada `CXX_TEST_PROGRAM_STACK_VIRT`), tratando falha de carregamento explicitamente (sem pânico), mesma disciplina da spec `kernel-elf-loader`

## 4. Verificação em boot real

- [x] 4.1 Verificado em boot real (`cargo xtask run -- -serial stdio -display none`): `cxx_test_program: resultado PP (ordem: virtual_dispatch_new, delete)` — despacho virtual correto (implementação derivada executada através do ponteiro de classe-base, sem cair em `__cxa_pure_virtual`) e objeto alocado/liberado corretamente via `new`/`delete`, nenhum pânico/exceção
- [x] 4.2 `cargo xtask test` (smoke test de boot: PASSOU) e `cargo test -p kernel --lib` (258 testes, 0 falhas) — nenhuma regressão

## 5. Documentação

- [x] 5.1 Atualizar `docs/architecture/overview.md` seção 6, seção 15 e o cabeçalho introdutório: registrado o runtime C++ mínimo entregue por esta change, e atualizado o próximo passo do item 8 (esqueleto `os_javaos`, agora com libc C, ponto flutuante e runtime C++ disponíveis)
- [x] 5.2 Atualizado `ROADMAP.md`: nova seção "Runtime C++ mínimo" em "o que já está implementado", removido da lista de pendências em "Destaque: o que falta para a JVM rodar" (item 3 recalibrado para exceções/RTTI/STL, ainda pendentes), e data de atualização registrada

## Workflow follow-up

- Arquivar esta change (`opsx:archive`) depois que as tasks acima estiverem concluídas e revisadas.
