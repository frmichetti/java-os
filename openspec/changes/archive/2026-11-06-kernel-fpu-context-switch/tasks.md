# Tasks

## 1. Inicialização de CR0/CR4 e template de FPU (`kernel::fpu`)

- [x] 1.1 Criar `kernel::fpu` com `#[repr(C, align(16))] struct FxSaveArea([u8; 512])` e uma função `pub fn new_area() -> u64` que aloca via `Box::leak` uma cópia do template inicial (ver tarefa 1.2) e devolve o ponteiro
- [x] 1.2 Implementar a inicialização do template: `FNINIT` + `LDMXCSR` (valor padrão `0x1F80`) + `FXSAVE` numa área estática — verificado com teste de host (`generated_template_has_the_default_mxcsr_value`) que o template gerado tem o byte de `MXCSR` no valor esperado. **Achado real durante esta tarefa**: `cargo test -p kernel --lib` revelou um bug pré-existente não relacionado (rótulos de dados sem prefixo em `global_asm!` de módulos de demo diferentes colidindo) — corrigido, registrado em `KNOW_ISSUES.md`
- [x] 1.3 Implementar `pub unsafe fn init()`: limpa `CR0.EM`, seta `CR0.MP`; seta `CR4.OSFXSR`, `CR4.OSXMMEXCPT` — mesmo estilo de `asm!` raw já usado em `kernel::paging` para `CR3`
- [x] 1.4 Chamar `kernel::fpu::init()` em `kernel_main`, logo após `kernel::gdt::init()` e antes de `kernel::scheduler::register_boot_thread()`; verificado em boot real que o kernel continua inicializando normalmente até o banner de boot e todos os demos existentes continuam passando (nenhuma exceção `#UD`/`#GP` introduzida)

## 2. Área de FPU por thread e troca de contexto

- [x] 2.1 Adicionar `pub fpu_state: u64` a `kernel::process::Context`, inicializado em todos os pontos de construção existentes (valor temporário `0`, sobrescrito pela tarefa 2.2)
- [x] 2.2 Em `kernel::scheduler::register_thread`, alocar a área de FPU da nova thread via `kernel::fpu::new_area()` e gravá-la no campo `fpu_state` do `Context` recém-registrado
- [x] 2.3 Adicionar `static mut CURRENT_FPU_AREA: u64` em `kernel::scheduler` (mesmo papel de `CURRENT_KERNEL_STACK_TOP`); atualizá-la dentro de `schedule_next`, no mesmo trecho que já atualiza `set_current_kernel_stack`/`switch_address_space`, para o `fpu_state` da nova thread corrente. **Achado real durante esta tarefa**: isso sozinho não bastava — `register_boot_thread` também precisou inicializar `CURRENT_FPU_AREA` explicitamente (mesmo racional já existente para `set_current_kernel_stack`), já que o primeiro tick do timer executa `FXSAVE` antes de `schedule_next` rodar pela primeira vez; sem essa correção, boot real confirmou `EXCECAO #PF (page fault): endereco=0x0` no primeiro tick
- [x] 2.4 Alterar `timer_entry`/`block_entry` (`global_asm!`): `FXSAVE [CURRENT_FPU_AREA]` logo após os `push` de registradores de uso geral (antes do `call`); `FXRSTOR [CURRENT_FPU_AREA]` logo após `mov rsp, rax` (antes dos `pop`) — verificado em boot real sem nenhuma exceção, com todos os demos existentes continuando a passar
- [x] 2.5 Testes de host para a lógica pura que não toca hardware: já cobertos por `fxsave_area_has_the_size_and_alignment_fxsave_requires` (tarefa 1.1/1.2)

## 3. Reabilitar SSE no target de usuário

- [x] 3.1 Editar `x86_64-java_os_user.json`: remover `-sse`/`-sse2`/`-sse3`/`-ssse3`/`-sse4.1`/`-sse4.2` da lista de `features` negadas e remover `+soft-float`/`rustc-abi: softfloat` — `-mmx`/`-avx`/`-avx2` permanecem desabilitados (AVX precisaria de `XSAVE`, mais largo que a área legada de `FXSAVE` desta change; MMX seguiu fora de escopo, sem necessidade identificada). `x86_64-java_os_user_pic.json` (bibliotecas compartilhadas de teste) deliberadamente não alterado nesta change — fora do escopo definido no proposal.md
- [x] 3.2 Recompilar os binários de teste de usuário já existentes (`elf_test_program` via `cargo xtask build-image`) contra o target atualizado e verificar que continuam compilando e rodando normalmente em boot real (`cargo xtask test`: smoke test PASSOU; boot real: `/bin/test.elf` carregado e executando, todos os demos com seus resultados `P` de sempre, nenhuma exceção)

## 4. Verificação em boot real: FPU sobrevive à preempção sem se misturar entre threads

- [x] 4.1 Escrever um programa de teste em modo usuário (`kernel::fpu_demo`, assembly puro via `global_asm!`, mesmo padrão de `kernel::user_posix_demo`) que executa um laço de 50.000.000 iterações de `addsd` (soma de `double` real, via SSE), longo o bastante para ser preemptado várias vezes pelo timer a 100 Hz, comparando o resultado final via `cvtsi2sd`+`ucomisd` (exato, sem erro de arredondamento — soma de incrementos inteiros cabe na mantissa de 53 bits do `double`)
- [x] 4.2 Duas variantes (`fpu_demo_a`: semente `1000.0`, delta `1.0`; `fpu_demo_b`: semente `2000.0`, delta `3.0`) rodando concorrentemente em processos distintos (`kernel::scheduler::spawn_fpu_demo`), cada uma com seu próprio `AddressSpace` — o escalonador realmente alterna entre elas e com todas as outras threads de demonstração já existentes
- [x] 4.3 Verificado em boot real (`cargo xtask run -- -serial stdio -display none`): `fpu_demo_a: resultado P` e `fpu_demo_b: resultado P`, confirmado intercalado com dezenas de iterações de outras threads em loop (prova de preempção real repetida) — nenhum valor misturado entre as duas variantes. **Achado real durante esta tarefa**: a primeira rodada reportou `fpu_demo_b: resultado F` — não um bug do kernel, e sim um erro aritmético meu no valor esperado codificado no programa de teste (`152000` em vez de `2000 + 50000000×3 = 150002000`); corrigido, ambas passam
- [x] 4.4 `cargo xtask test` (smoke test de boot: PASSOU) e `cargo test -p kernel --lib` (258 testes, 0 falhas) — nenhuma regressão

## 5. Documentação

- [x] 5.1 Atualizar `docs/architecture/overview.md`: remover/atualizar a nota de limitação "FPU/SSE/XMM não são salvos nem restaurados" (seção 3, registrada pela change `libc-minimal-port`), registrando que foi resolvida por esta change
- [x] 5.2 Atualizar `docs/architecture/overview.md` seção 6 (Estratégia de Portabilidade do OpenJDK), seção 15 (Roadmap Completo) e o cabeçalho introdutório: registrar `kernel-fpu-context-switch` como mais uma sub-change concluída do item 8, e atualizar o próximo passo

## Workflow follow-up

- Arquivar esta change (`opsx:archive`) depois que as tasks acima estiverem concluídas e revisadas.
