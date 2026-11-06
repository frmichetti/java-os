# Design

## Context

`kernel::process::Context` hoje só guarda `rsp`, `kernel_stack_top` e `address_space_root` — registradores de uso geral, `RIP` e `RFLAGS` são salvos/restaurados via `push`/`pop` diretamente na stack de kernel da própria thread, pelas rotinas em assembly `timer_entry`/`block_entry` (`global_asm!` em `kernel/src/scheduler.rs`); não há nenhum struct separado para eles. `schedule_next` (Rust) decide a próxima thread, atualiza `TSS.rsp0`/`CR3` via `set_current_kernel_stack`/`switch_address_space`, e devolve o `rsp` salvo dela — a rotina assembly então troca `RSP` e faz `pop` dos registradores de uso geral da nova thread.

`x86_64-java_os.json` (kernel) e `x86_64-java_os_user.json` (usuário) hoje desabilitam SSE inteiramente (`-mmx,-sse,-sse2,...`) e usam `rustc-abi: softfloat` — uma ABI de soft-float que o LLVM/Rust suporta de fato para x86_64 (confirmado durante a change `libc-minimal-port`: o GCC **não** tem equivalente utilizável nessa arquitetura — `-mno-sse` ali simplesmente quebra a compilação de qualquer função com `float`/`double` por valor). Como o kernel nunca emite instruções SSE sob esse target, código de modo kernel nunca toca XMM — só código de modo usuário, uma vez que SSE for reabilitado para ele, passará a usar esses registradores.

Nenhuma inicialização de `CR0`/`CR4` existe hoje em nenhum lugar do kernel (confirmado por busca no código-fonte) — o estado desses registradores de controle depende inteiramente do que o firmware UEFI/Limine deixou configurado antes de entregar o controle ao kernel.

A entrada de `syscall` (`kernel::syscall::syscall_entry`) roda inteiramente com interrupções desabilitadas (`IA32_FMASK` mascara `IF` durante toda a *syscall* — já documentado pela change `kernel-syscalls`), então uma preempção pelo timer nunca pode acontecer no meio de uma *syscall*; a troca de contexto só é disparada pelas duas rotinas de interrupção já existentes (`timer_entry`, `block_entry`).

Ver proposal.md para motivação; ver spec delta de `kernel-scheduler` para o contrato comportamental completo.

## Goals / Non-Goals

**Goals:**
- Uma thread de modo usuário com um cálculo de ponto flutuante em andamento sobrevive a qualquer número de preempções sem seu estado de FPU/SSE/XMM ser corrompido ou misturado com o de outra thread.
- `CR0`/`CR4` configurados explicitamente pelo próprio kernel, não herdados implicitamente do firmware.

**Non-Goals:**
- AVX/AVX2 ou qualquer estado mais largo que XMM (`YMM`/`ZMM`) — `FXSAVE`/`FXRSTOR` (formato legado, cobre x87/MMX/SSE até XMM) é suficiente para o que a libc/HotSpot precisam nesta etapa. `XSAVE`/`XRSTOR` (necessário para AVX) fica para uma change futura, se/quando necessário.
- Reabilitar ponto flutuante na build do newlib em si (`libm`, conversão `float`/`strtod`) — a change `libc-minimal-port` excluiu essas partes explicitamente da build; esta change só torna SSE disponível na CPU/ABI, não reconstrói o newlib. Fica para uma sub-change futura decidir se/como.
- Qualquer mudança no target do próprio kernel (`x86_64-java_os.json` permanece soft-float).
- Salvar/restaurar FPU na entrada de `syscall` — desnecessário, ver Contexto (syscall roda com `IF=0`, nunca preemptada).

## Decisions

### Área de `FXSAVE` por thread: novo campo em `Context`, alocada uma vez, nunca liberada

Cada thread ganha uma área de 512 bytes, alinhada a 16 bytes (`#[repr(C, align(16))] struct FxSaveArea([u8; 512])`), alocada via `Box::leak` (heap do kernel — acessível a partir de qualquer espaço de endereçamento, já que o heap vive na metade superior compartilhada) no momento em que a thread é registrada (`kernel::scheduler::register_thread`, o único ponto de registro de todas as threads, já confirmado nas changes anteriores). O ponteiro fica num novo campo `pub fpu_state: u64` de `Context`, ao lado de `rsp`/`kernel_stack_top`/`address_space_root`. Mesma disciplina de "leak aceitável por enquanto" já aceita para `kernel::pthread_handles`/`kernel::file_handles`/`CONTEXTS` em geral.

**Alternativa rejeitada — reaproveitar espaço na própria stack de kernel da thread**: `FXSAVE`/`FXRSTOR` exigem um endereço alinhado a 16 bytes e uma região fixa de tamanho conhecido — ao contrário dos registradores de uso geral (que cabem naturalmente em `push`/`pop`, sem exigir alinhamento especial), forçar isso na stack exigiria lógica de alinhamento adicional a cada troca de contexto sem nenhum benefício real sobre uma alocação fixa própria.

### Template de estado inicial válido (nunca zerado cru)

Zerar os 512 bytes ingenuamente deixaria `MXCSR` (word de controle de SSE, dentro da área) com bits reservados em valores potencialmente inválidos. `kernel::fpu::init()` executa `FNINIT` (reinicia o estado x87) seguido de `LDMXCSR` com o valor padrão (`0x1F80`, todas as exceções mascaradas), depois um único `FXSAVE` numa área estática de template. Cada nova área de thread é inicializada como uma cópia desse template (`memcpy`), nunca zerada diretamente.

### `timer_entry`/`block_entry`: `FXSAVE`/`FXRSTOR` ao redor da troca de `RSP`, via uma global "área de FPU corrente"

Mesmo padrão já usado para `TSS.rsp0`/`CR3`: uma nova `static mut CURRENT_FPU_AREA: u64` (ao lado de `CURRENT_KERNEL_STACK_TOP`), atualizada por `schedule_next` (dentro do mesmo trecho que já atualiza `set_current_kernel_stack`/`switch_address_space`) para apontar à área de FPU da nova thread corrente. Nas rotinas assembly:
- Logo após os 15 `push` de registradores de uso geral (register `rax` já salvo nesse ponto, livre para uso como escrátio de endereço) e antes de `call on_timer_tick`/`on_block_request`: `FXSAVE [CURRENT_FPU_AREA]` — salva o estado da thread que está saindo.
- Logo após `mov rsp, rax` (troca para o `rsp` da nova thread) e antes da sequência de `pop`: `FXRSTOR [CURRENT_FPU_AREA]` — neste ponto a global já foi atualizada por `schedule_next` para a nova thread corrente, então restaura o estado dela. `rax` já não é mais necessário para nada além do próprio `pop rax` final (que lê da stack da nova thread, não do valor antigo).

### `CR0`/`CR4`: inicializados uma única vez, cedo no boot

`kernel::fpu::init()` (novo módulo) limpa `CR0.EM` (bit 2) e seta `CR0.MP` (bit 1); seta `CR4.OSFXSR` (bit 9) e `CR4.OSXMMEXCPT` (bit 10) — sequência padrão documentada pela Intel/AMD para habilitar `FXSAVE`/`FXRSTOR`/SSE sob um SO que gerencia esse estado por thread. Chamado em `kernel_main` logo após `kernel::gdt::init()`, antes de `kernel::scheduler::register_boot_thread()` (primeiro ponto em que uma área de FPU é alocada e poderia, em tese, ser tocada).

## Risks / Trade-offs

- **[Risco] Esquecer de atualizar `CURRENT_FPU_AREA` em algum caminho que também deveria trocar `TSS.rsp0`/`CR3` deixaria `FXRSTOR` restaurando o estado da thread errada** → Mitigação: atualizado no mesmo trecho de `schedule_next` que já atualiza os outros dois, não um caminho separado — mesma superfície de erro já mitigada para `TSS.rsp0`/`CR3` por essa coesão.
- **[Risco] Reabilitar SSE no target de usuário sem `CR0`/`CR4` corretamente configurados causaria `#UD`/`#NM` na primeira instrução SSE executada por um programa de usuário** → Mitigação: `kernel::fpu::init()` roda incondicionalmente, cedo no boot, antes de qualquer thread de usuário poder ser escalonada.
- **[Risco] Um programa de usuário recompilado com SSE habilitado, mas ainda rodando um binário antigo da versão anterior do target (sem SSE)** → Mitigação: nenhuma necessária — binários antigos simplesmente não contêm instruções SSE, continuam funcionando exatamente como antes; a mudança de target só afeta compilações futuras.

## Migration Plan

Aditivo no runtime; a única mudança de build é o target `x86_64-java_os_user.json` — programas de usuário existentes continuam funcionando sem recompilar (não usam SSE); só precisam ser recompilados para poderem passar a usar `float`/`double` de verdade. Nenhum passo de migração de dados ou estado persistente.
