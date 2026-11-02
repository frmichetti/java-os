# Tasks

## 1. Consulta somente-leitura de mapeamento em `paging`

- [x] 1.1 Implementar `Mapper::translate(&self, virt: u64) -> Option<u64>` em `kernel/src/paging.rs`, percorrendo PML4→PDPT→PD→PT sem criar nem remover tabelas. Como `translate` depende de `read_cr3()`/HHDM reais (mesma razão pela qual `map`/`unmap` não têm testes de host hoje), verificar com teste unitário (host, sem hardware) apenas o guard de desalinhamento (retorna `None` sem tocar `CR3`); os casos de endereço mapeado/não mapeado são verificados em boot real na tarefa 3.3.

## 2. Stack dedicada do kernel com guard page

- [x] 2.1 Criar `kernel/src/stack.rs` com as constantes da faixa virtual reservada (`KERNEL_STACK_BASE = 0xffff_a000_0000_0000`, `KERNEL_STACK_SIZE = 64 * 1024`) e registrar o módulo em `kernel/src/lib.rs`; verificar que `cargo build` compila.
- [x] 2.2 Implementar a função de mapeamento da stack (16 páginas de 4 KiB a partir de `KERNEL_STACK_BASE`, com `PageFlags::KERNEL_RW`, via `Mapper`), deixando deliberadamente sem mapear a página em `KERNEL_STACK_BASE - PAGE_SIZE`. Como o mapeamento real depende de `read_cr3()`/HHDM (não testável em host, mesma limitação de `map`/`unmap`), verificar que `cargo build` compila e, em boot real (tarefa 3.3), que todas as páginas da stack ficam acessíveis e `Mapper::translate` retorna `None` para a guard page.
- [x] 2.3 Implementar `switch_to(stack_top, continuation: extern "C" fn() -> !) -> !`, que calcula `new_rsp = stack_top - 8` (alinhamento ABI, ver design.md) e executa o handoff via `asm!("mov rsp, {0}", "jmp {1}", ..., options(noreturn))`, deliberadamente separado de `map` (ver design.md — "Por que duas funções" — combinar as duas causou deadlock real em boot por um `MutexGuard` nunca liberado através do `jmp` divergente); verificado via boot real (tarefa 3.2/4.1): `RSP` cai dentro do intervalo esperado da nova stack e o boot prossegue sem travar.

## 3. Integração em `kernel_main`

- [x] 3.1 Dividir `kernel_main` em `kernel_main` (GDT/IDT, serial, HHDM, banner, memmap, alocador de frames) e uma nova `extern "C" fn continue_boot() -> !` (heap, verificação/init de x2APIC, timer, `idle_loop`) em `kernel/src/main.rs`, sem alterar o comportamento de nenhuma etapa; verificar que `cargo xtask test` ainda passa (banner, memmap e demais logs inalterados).
- [x] 3.2 Chamar `kernel::stack::map`/`switch_to` imediatamente após `FRAME_ALLOCATOR.lock().init(...)` retornar `Ok` (antes de `heap::init()`), com o lock liberado por escopo normal entre as duas chamadas (ver design.md), substituindo a chamada direta a `continue_boot`/restante do corpo; verificado via `cargo xtask test` (log serial completo: stack nova, heap, APIC e timer todos reportados após a troca — primeira tentativa, com um `init_and_switch` combinado, travava em deadlock de spinlock; corrigido antes de marcar esta tarefa).
- [x] 3.3 Adicionar ao log de boot (mesmo padrão de `memmap_report`) uma linha reportando o endereço da guard page e o resultado de `Mapper::translate` sobre ela (deve logar "não mapeada"); verificado lendo a saída serial de `cargo xtask test`: `stack do kernel: 0xffffa00000000000-0xffffa00000010000; guard page em 0xffff9ffffffff000 (nao mapeada, como esperado)`.

## 4. Page fault com stack dedicada (IST)

Descoberto necessário durante a tarefa 4.2 original (ver design.md — "Page fault também precisa de stack dedicada (IST)"): sem isso, um estouro de stack escala direto para `#DF` sem o handler de `#PF` nunca rodar, violando o requisito de `kernel-stack-protection`. Modifica `kernel-interrupts` (ver `specs/kernel-interrupts/spec.md` nesta change e `proposal.md` — Modified Capabilities).

- [x] 4.1 Adicionar `PAGE_FAULT_STACK` (16 KiB, mesmo tamanho/padrão de `DOUBLE_FAULT_STACK`) e `PAGE_FAULT_IST_INDEX = 2` em `kernel/src/gdt.rs`; configurar `TSS.ist[(PAGE_FAULT_IST_INDEX - 1) as usize]` em `gdt::init()`; verificar que `cargo build` compila.
- [x] 4.2 Atualizar `idt.rs` para registrar `page_fault_handler` com `crate::gdt::PAGE_FAULT_IST_INDEX as u8` em vez de `0`; atualizar/estender o teste unitário `exception_handlers_are_installed_at_correct_vectors_with_correct_ist` para cobrir o novo IST do `#PF`; verificar com `cargo test -p kernel --lib`.

## 5. Verificação end-to-end e fechamento do know issue

- [x] 5.1 Validar manualmente via GDB (ou mecanismo equivalente já usado em `kernel-interrupts-timer`) que, logo após o `jmp` da tarefa 2.3, `RSP` cai dentro do intervalo `[KERNEL_STACK_BASE, KERNEL_STACK_BASE + KERNEL_STACK_SIZE)`; verificado: `RSP = 0xffffa0000000fec0` (offset `0xfec0`, dentro de `[0, 0x10000)`) no breakpoint em `continue_boot`, sem endereço de retorno pendurado no backtrace (consistente com o `jmp`, não `call`).
- [x] 5.2 Provocar um estouro real de stack (recursão profunda, mesmo cenário do know issue) após a troca e confirmar via log/GDB que o `#PF` resultante referencia um endereço dentro da guard page (`KERNEL_STACK_BASE - PAGE_SIZE` até `KERNEL_STACK_BASE - 1`), antes de qualquer escrita bem-sucedida além do limite inferior da stack mapeada — cobre os dois requisitos de `specs/kernel-stack-protection/spec.md`. **Primeira tentativa (antes do Grupo 4) falhou:** escalou direto para `#DF` sem nenhum `#PF`. Depois de 4.1/4.2: `EXCECAO #PF (page fault): endereco=0xffff9ffffffffff8 codigo_erro=0x2 rip=0xffffffff80020954` — endereço dentro da guard page (`[0xffff9ffffffff000, 0xffffa00000000000)`), sem `#DF` subsequente. Código de teste temporário (sonda de recursão) removido após a verificação.
- [x] 5.3 Atualizar `KNOW_ISSUES.md` removendo (ou marcando como resolvida) a entrada "Ausência de guard page na stack principal do kernel", referenciando esta change; verificar lendo o arquivo atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para as modificações propostas.
