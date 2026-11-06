//! Programa de teste em modo usuário que verifica `SYSCALL_EXIT` (change
//! `libc-minimal-port`) em boot real: escreve uma mensagem uma única vez,
//! chama `SYSCALL_EXIT`, e nunca mais deveria executar depois disso. Um
//! canário de erro (laço que imprimiria repetidamente uma mensagem de falha)
//! existe logo após a chamada só para tornar óbvio, no log serial, se o
//! escalonador algum dia voltasse a retomar esta thread por engano — mesmo
//! padrão de mensagem única de `kernel::user_posix_demo`/
//! `kernel::file_syscalls_demo`.

core::arch::global_asm!(
    ".global exit_demo_start",
    ".global exit_demo_end",
    "exit_demo_start:",
    "lea rdi, [rip + msg1]",
    "mov esi, [rip + msg1len]",
    "xor eax, eax", // SYSCALL_WRITE
    "syscall",
    "mov eax, 21", // SYSCALL_EXIT
    "syscall",
    // Nunca deveria ser alcançado: SYSCALL_EXIT nunca retorna. Se o
    // escalonador tiver um bug e retomar esta thread mesmo assim, isto
    // imprime repetidamente em vez de falhar silenciosamente.
    "9:",
    "lea rdi, [rip + msg_err]",
    "mov esi, [rip + msg_err_len]",
    "xor eax, eax",
    "syscall",
    "jmp 9b",
    "msg1len: .long msg1_end - msg1",
    "msg1:",
    ".ascii \"exit_demo: mensagem antes de SYSCALL_EXIT\\n\"",
    "msg1_end:",
    "msg_err_len: .long msg_err_end - msg_err",
    "msg_err:",
    ".ascii \"exit_demo: ERRO - thread retomada apos SYSCALL_EXIT\\n\"",
    "msg_err_end:",
    "exit_demo_end:",
);

unsafe extern "C" {
    fn exit_demo_start();
    fn exit_demo_end();
}

/// Endereço, dentro do binário do kernel, onde a rotina de teste começa —
/// nunca usado diretamente como `RIP` de uma thread, só como origem para
/// [`copy_into`] (mesmo papel de `kernel::user_posix_demo::program_start_addr`).
fn program_start_addr() -> u64 {
    exit_demo_start as *const () as u64
}

/// Tamanho, em bytes, da rotina de teste (código e dados embutidos).
fn program_len() -> u64 {
    (exit_demo_end as *const () as u64) - (exit_demo_start as *const () as u64)
}

/// Copia os bytes da rotina de teste para `dest` — mesmo contrato de
/// [`crate::user_posix_demo::copy_into`].
///
/// # Safety
/// `dest` deve apontar para uma região mapeada, gravável no momento da
/// cópia, com pelo menos [`program_len`] bytes de espaço (uma única página,
/// já que o programa inteiro cabe em menos de 4 KiB).
pub unsafe fn copy_into(dest: u64) {
    debug_assert!(program_len() <= crate::paging::PAGE_SIZE, "exit_demo nao cabe numa unica pagina de codigo");
    unsafe {
        core::ptr::copy_nonoverlapping(program_start_addr() as *const u8, dest as *mut u8, program_len() as usize);
    }
}
