//! Programa de teste em modo usuário que exercita, numa única thread de anel
//! 3, as três *syscalls* desta change (`kernel-file-syscalls`) — ver
//! `design.md`, "Numeração de syscalls". Mesmo padrão de
//! `kernel::user_posix_demo`: assembly independente de posição, sem
//! carregador de executáveis, cada verificação grava `'P'`/`'F'` num
//! caractere dedicado de uma única mensagem de resultado reportada via
//! `SYSCALL_WRITE` ao final.
//!
//! Abre `/TEST.TXT` (fixture de 16 bytes, `kernel/testdata/file_syscalls_fixture.txt`,
//! copiada para a imagem de disco por `xtask::build_image` — ver tasks.md,
//! tarefa 3.1), lê os primeiros 8 bytes, depois o restante (pedindo mais do
//! que o disponível, para exercitar "devolve só o restante"), depois uma
//! leitura extra no fim do arquivo (deve devolver zero sem erro), fecha o
//! handle, e então confirma que ler ou fechar novamente o mesmo handle já
//! fechado é rejeitado. Por fim, confirma que abrir um caminho inexistente é
//! rejeitado.

core::arch::global_asm!(
    ".global file_syscalls_demo_start",
    ".global file_syscalls_demo_end",
    "file_syscalls_demo_start:",
    // ---------- 1) SYSCALL_OPEN(\"/TEST.TXT\") ----------
    "lea rdi, [rip + path]",
    "mov esi, [rip + pathlen]",
    "mov eax, 18", // SYSCALL_OPEN
    "syscall",
    "mov r12, rax", // r12 = handle, preservado entre syscalls (callee-saved)
    "mov byte ptr [rip + st0], 'F'",
    "cmp r12, -1",
    "je 1f",
    "mov byte ptr [rip + st0], 'P'",
    "1:",
    // ---------- 2) SYSCALL_READ(handle, buf, 8): primeiros 8 bytes ("ABCDEFGH") ----------
    "mov rdi, r12",
    "lea rsi, [rip + buf]",
    "mov edx, 8",
    "mov eax, 19", // SYSCALL_READ
    "syscall",
    "mov byte ptr [rip + st1], 'F'",
    "cmp rax, 8",
    "jne 1f",
    "mov rax, [rip + buf]",
    "movabs r11, 0x4847464544434241", // "ABCDEFGH" (little-endian)
    "cmp rax, r11",
    "jne 1f",
    "mov byte ptr [rip + st1], 'P'",
    "1:",
    // ---------- 3) SYSCALL_READ(handle, buf, 100): pede mais que o restante (8 bytes: "IJKLMNOP") ----------
    "mov rdi, r12",
    "lea rsi, [rip + buf]",
    "mov edx, 100",
    "mov eax, 19",
    "syscall",
    "mov byte ptr [rip + st2], 'F'",
    "cmp rax, 8",
    "jne 1f",
    "mov rax, [rip + buf]",
    "movabs r11, 0x504f4e4d4c4b4a49", // "IJKLMNOP" (little-endian)
    "cmp rax, r11",
    "jne 1f",
    "mov byte ptr [rip + st2], 'P'",
    "1:",
    // ---------- 4) SYSCALL_READ no fim do arquivo: zero bytes, sem erro ----------
    "mov rdi, r12",
    "lea rsi, [rip + buf]",
    "mov edx, 8",
    "mov eax, 19",
    "syscall",
    "mov byte ptr [rip + st3], 'F'",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + st3], 'P'",
    "1:",
    // ---------- 5) SYSCALL_CLOSE(handle) ----------
    "mov rdi, r12",
    "mov eax, 20", // SYSCALL_CLOSE
    "syscall",
    "mov byte ptr [rip + st4], 'F'",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + st4], 'P'",
    "1:",
    // ---------- 6) SYSCALL_READ apos fechar: rejeitado ----------
    "mov rdi, r12",
    "lea rsi, [rip + buf]",
    "mov edx, 8",
    "mov eax, 19",
    "syscall",
    "mov byte ptr [rip + st5], 'F'",
    "cmp rax, -1",
    "jne 1f",
    "mov byte ptr [rip + st5], 'P'",
    "1:",
    // ---------- 7) SYSCALL_CLOSE num handle ja fechado: rejeitado ----------
    "mov rdi, r12",
    "mov eax, 20",
    "syscall",
    "mov byte ptr [rip + st6], 'F'",
    "cmp rax, -1",
    "jne 1f",
    "mov byte ptr [rip + st6], 'P'",
    "1:",
    // ---------- 8) SYSCALL_OPEN de caminho inexistente: rejeitado ----------
    "lea rdi, [rip + missing_path]",
    "mov esi, [rip + missing_pathlen]",
    "mov eax, 18",
    "syscall",
    "mov byte ptr [rip + st7], 'F'",
    "cmp rax, -1",
    "jne 1f",
    "mov byte ptr [rip + st7], 'P'",
    "1:",
    // ---------- reporta o resultado uma unica vez ----------
    "lea rdi, [rip + msg]",
    "mov esi, [rip + msglen]",
    "xor eax, eax", // SYSCALL_WRITE
    "syscall",
    "9:",
    "jmp 9b",
    "path: .ascii \"/TEST.TXT\"",
    "pathlen: .long 9",
    "missing_path: .ascii \"/NOPE.TXT\"",
    "missing_pathlen: .long 9",
    "buf: .space 16",
    "msglen: .long msg_end - msg",
    "msg:",
    ".ascii \"file_syscalls_demo: resultado \"",
    "st0: .ascii \"?\"",
    "st1: .ascii \"?\"",
    "st2: .ascii \"?\"",
    "st3: .ascii \"?\"",
    "st4: .ascii \"?\"",
    "st5: .ascii \"?\"",
    "st6: .ascii \"?\"",
    "st7: .ascii \"?\"",
    ".ascii \" (ordem: open,read1,read2,read_eof,close,read_after_close,close_twice,open_missing)\\n\"",
    "msg_end:",
    "file_syscalls_demo_end:",
);

unsafe extern "C" {
    fn file_syscalls_demo_start();
    fn file_syscalls_demo_end();
}

/// Endereço, dentro do binário do kernel, onde a rotina de teste começa —
/// nunca usado diretamente como `RIP` de uma thread, só como origem para
/// [`copy_into`] (mesmo papel de `kernel::user_posix_demo::program_start_addr`).
fn program_start_addr() -> u64 {
    file_syscalls_demo_start as *const () as u64
}

/// Tamanho, em bytes, da rotina de teste (código e dados embutidos).
fn program_len() -> u64 {
    (file_syscalls_demo_end as *const () as u64) - (file_syscalls_demo_start as *const () as u64)
}

/// Copia os bytes da rotina de teste para `dest` — mesmo contrato de
/// [`crate::user_posix_demo::copy_into`].
///
/// # Safety
/// `dest` deve apontar para uma região mapeada, gravável no momento da
/// cópia, com pelo menos [`program_len`] bytes de espaço (uma única página,
/// já que o programa inteiro cabe em menos de 4 KiB).
pub unsafe fn copy_into(dest: u64) {
    debug_assert!(program_len() <= crate::paging::PAGE_SIZE, "file_syscalls_demo nao cabe numa unica pagina de codigo");
    unsafe {
        core::ptr::copy_nonoverlapping(program_start_addr() as *const u8, dest as *mut u8, program_len() as usize);
    }
}
