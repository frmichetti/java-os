//! Programa de teste em modo usuário, usado para verificar o mecanismo de
//! `syscall`/`sysret` ponta a ponta — ver `design.md` da change
//! `kernel-syscalls`, "Primeiro programa de usuário".
//!
//! Escrito em assembly puro, deliberadamente independente de posição (só
//! usa endereçamento relativo a `RIP` e `syscall`/registradores — nunca um
//! endereço absoluto do kernel), para que seus bytes possam ser copiados
//! para uma página de usuário em um endereço virtual diferente de onde
//! foram originalmente linkados. Chama a syscall de escrita com uma
//! mensagem fixa, depois a de consulta de ticks, em laço — não há
//! carregador de executáveis ainda (ver Non-Goals em design.md), então este
//! é o único jeito de ter código de teste rodando em modo usuário.

core::arch::global_asm!(
    ".global user_demo_start",
    ".global user_demo_end",
    "user_demo_start:",
    "2:",
    "lea rdi, [rip + 3f]",       // ptr da mensagem (arg0 da syscall de escrita)
    "mov esi, [rip + .Lud_msglen]", // tamanho da mensagem (arg1)
    "xor eax, eax",              // SYSCALL_WRITE = 0
    "syscall",
    "mov eax, 1", // SYSCALL_GET_TICKS = 1
    "syscall",
    "jmp 2b",
    // `.Lud_msglen` (rótulo local real, não símbolo global) — achado real: sem
    // o prefixo `.L`, "msglen" colidia com o mesmo nome em
    // `kernel::file_syscalls_demo`/`kernel::user_posix_demo` assim que os
    // três `global_asm!` eram relinkados juntos — ver KNOW_ISSUES.md.
    ".Lud_msglen: .long 4f - 3f", // tamanho da mensagem, como dado (não cabe como imediato direto)
    "3:",
    ".ascii \"ola do modo usuario, via syscall!\\n\"",
    "4:",
    "user_demo_end:",
);

unsafe extern "C" {
    fn user_demo_start();
    fn user_demo_end();
}

/// Endereço, dentro do binário do kernel, onde a rotina de teste começa —
/// o endereço *original*, nunca usado diretamente como `RIP` de uma thread
/// (código do kernel não é acessível por usuário); serve só de origem para
/// [`copy_into`].
fn program_start_addr() -> u64 {
    user_demo_start as *const () as u64
}

/// Tamanho, em bytes, da rotina de teste (do início ao fim, incluindo a
/// mensagem embutida).
fn program_len() -> u64 {
    (user_demo_end as *const () as u64) - (user_demo_start as *const () as u64)
}

/// Copia os bytes da rotina de teste para `dest` — qualquer endereço
/// virtual atualmente válido e gravável que corresponda à memória física
/// desejada (ex.: o alias HHDM de um frame, não necessariamente o endereço
/// virtual *final* da página de usuário, que só é acessível depois de uma
/// troca de `CR3` para a address space dona dela — ver
/// `kernel::scheduler::spawn_user_demo`).
///
/// # Safety
/// `dest` deve apontar para uma região de memória mapeada, com permissão de
/// escrita no momento da cópia, e com pelo menos [`program_len`] bytes de
/// espaço.
pub unsafe fn copy_into(dest: u64) {
    unsafe {
        core::ptr::copy_nonoverlapping(program_start_addr() as *const u8, dest as *mut u8, program_len() as usize);
    }
}
