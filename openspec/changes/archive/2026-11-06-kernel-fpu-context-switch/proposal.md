# Proposal

## Why

A change `libc-minimal-port` (arquivada) teve que excluir todo suporte a ponto flutuante da libc C recém-portada (`libm` inteira, conversão `float`/`strtod`) porque descobriu, na prática, que o kernel desabilita SSE inteiramente — tanto no seu próprio target quanto no de modo usuário — e `kernel::process::Context`/`kernel::scheduler` nunca salvam nem restauram registradores XMM/FPU entre threads. Usar `float`/`double` hoje corromperia esse estado silenciosamente na primeira preempção, já que a troca de contexto só cobre registradores de uso geral. Sem esta change, nem a libc C recém-portada nem o futuro esqueleto `os_javaos` do HotSpot (que usa `double`/`float` como tipos primitivos Java em praticamente toda parte) podem avançar — é o próximo passo explicitamente identificado pela própria `libc-minimal-port`.

## What Changes

- Inicialização explícita de `CR0` (limpa `EM`, seta `MP`) e `CR4` (seta `OSFXSR`, `OSXMMEXCPT`) durante o boot do kernel — pré-requisito de hardware para `FXSAVE`/`FXRSTOR`/SSE funcionarem corretamente, hoje nunca configurado pelo kernel (depende implicitamente do que o firmware UEFI/Limine deixou), inconsistente com a disciplina já seguida para GDT/IDT/APIC (sempre configurados explicitamente).
- Uma área de `FXSAVE` por thread (512 bytes, alinhada a 16 bytes, alocada uma vez na criação da thread e nunca liberada — mesma disciplina de "leak aceitável por enquanto" já usada para `kernel::pthread_handles`/`kernel::file_handles`), inicializada com um estado de FPU válido (não zerada crua — `MXCSR` tem bits reservados que não podem ser arbitrários).
- As rotinas de troca de contexto em assembly (`timer_entry`, `block_entry`) passam a executar `FXSAVE` na área da thread que está saindo antes de trocar `RSP`, e `FXRSTOR` da área da nova thread corrente depois de trocar `RSP` — mesmo padrão já usado para `TSS.rsp0`/`CR3` (uma variável global "área de FPU corrente", atualizada por `schedule_next`, analogamente a `CURRENT_KERNEL_STACK_TOP`).
- **BREAKING (de build, não de comportamento em tempo de execução)**: `x86_64-java_os_user.json` reabilita SSE/SSE2 (remove `+soft-float`/`rustc-abi: softfloat` para esse target) — qualquer programa de usuário recompilado a partir de agora pode usar `float`/`double` de verdade. O target do próprio kernel (`x86_64-java_os.json`) permanece soft-float, sem alteração — o kernel nunca precisou de ponto flutuante e continua sem precisar.
- Verificação em boot real: múltiplas threads de usuário concorrentes, cada uma fazendo uma computação de ponto flutuante real e distinta (ex.: somas/multiplicações de `double` com resultado conhecido), deliberadamente preemptadas pelo timer repetidamente — cada uma confirma seu próprio resultado ao final, provando que o estado de FPU de cada thread sobrevive à preempção sem se misturar com o de outra.

## Capabilities

### New Capabilities

(nenhuma)

### Modified Capabilities

- `kernel-scheduler`: o requisito existente "Troca de Contexto Preserva Estado da Thread" passa a cobrir explicitamente também o estado de FPU/SSE/XMM da thread, não só registradores de uso geral/RIP/RFLAGS.

## Impact

- **Código afetado**: novo módulo `kernel::fpu` (inicialização de `CR0`/`CR4`, estrutura da área de `FXSAVE`, template de estado inicial válido); `kernel::process::Context` (novo campo com o ponteiro da área de FPU da thread); `kernel::scheduler` (nova global "área de FPU corrente", `register_thread` passa a alocar a área de cada thread nova, `timer_entry`/`block_entry` ganham `FXSAVE`/`FXRSTOR`, `schedule_next` atualiza a global a cada troca); `x86_64-java_os_user.json` (reabilita SSE/SSE2); `main.rs` (chama `kernel::fpu::init()` cedo no boot, antes de qualquer thread poder ser escalonada).
- **Dependências**: nenhuma nova — só instruções x86_64 já disponíveis (`FXSAVE`/`FXRSTOR`, bits de `CR0`/`CR4`).
- **Compatibilidade**: puramente aditivo no comportamento em tempo de execução (nenhum programa existente, todos sem ponto flutuante, muda de comportamento); programas de usuário precisam ser recompilados contra o target atualizado para poderem usar `float`/`double` — nenhum binário já compilado deixa de funcionar (SSE desabilitado não impedia a execução de código sem instruções SSE).
- **Documentação**: `docs/architecture/overview.md` (seção 1 ou 3, remove a nota de limitação "FPU/SSE/XMM não são salvos" registrada pela change `libc-minimal-port`, e seção 6, atualiza o próximo passo do item 8).
