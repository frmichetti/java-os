//! Programa de teste em modo usuário que exercita, numa única thread de anel
//! 3, todas as *syscalls* desta change (`kernel-jvm-posix-syscalls`) — ver
//! `design.md`, "Escopo da verificação em boot real". Mesmo padrão de
//! `kernel::user_demo` (assembly independente de posição, sem carregador de
//! executáveis): reserva, compromete, escreve/relê, protege, descompromete e
//! libera uma faixa de memória virtual; cria, adquire e libera um mutex
//! nomeado; cria uma variável de condição nomeada e a notifica (um e todos)
//! sem nenhuma thread esperando; consulta os dois relógios; e invoca
//! deliberadamente `SYSCALL_VM_COMMIT`/`SYSCALL_MUTEX_LOCK` com
//! endereço/handle inválidos, confirmando o sentinela de erro em cada caso.
//!
//! Cada verificação grava `'P'`/`'F'` num caractere dedicado de uma única
//! mensagem de resultado, reportada uma única vez via `SYSCALL_WRITE` ao
//! final — evita precisar de uma rotina de formatação numérica em assembly
//! só para reportar cada passo individualmente (os dois relógios são
//! verificados por faixa plausível de valores, não impressos por extenso).

core::arch::global_asm!(
    ".global user_posix_demo_start",
    ".global user_posix_demo_end",
    "user_posix_demo_start:",
    // ---------- 1) SYSCALL_VM_RESERVE(PAGE_SIZE) ----------
    "mov edi, 0x1000",
    "mov eax, 2", // SYSCALL_VM_RESERVE
    "syscall",
    "mov r12, rax", // r12 = endereco reservado, preservado entre syscalls (callee-saved)
    "mov byte ptr [rip + .Lupd_st0], 'F'",
    "movabs r11, 0xffffc00000000000", // VMM_WINDOW_START
    "cmp r12, r11",
    "jb 1f",
    "movabs r11, 0xffffc01000000000", // VMM_WINDOW_START + VMM_WINDOW_SIZE
    "cmp r12, r11",
    "jae 1f",
    "mov byte ptr [rip + .Lupd_st0], 'P'",
    "1:",
    // ---------- 2) SYSCALL_VM_COMMIT(addr, PAGE_SIZE, RW) ----------
    "mov rdi, r12",
    "mov esi, 0x1000",
    "mov edx, 1", // prot: gravavel
    "mov eax, 3", // SYSCALL_VM_COMMIT
    "syscall",
    "mov byte ptr [rip + .Lupd_st1], 'F'",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + .Lupd_st1], 'P'",
    "1:",
    // ---------- 3) escreve um padrao conhecido e confirma a leitura ----------
    "movabs rax, 0xdeadbeefcafebabe",
    "mov [r12], rax",
    "mov rax, [r12]",
    "mov byte ptr [rip + .Lupd_st2], 'F'",
    "movabs r11, 0xdeadbeefcafebabe",
    "cmp rax, r11",
    "jne 1f",
    "mov byte ptr [rip + .Lupd_st2], 'P'",
    "1:",
    // ---------- 4) SYSCALL_VM_PROTECT(addr, PAGE_SIZE, somente leitura) ----------
    "mov rdi, r12",
    "mov esi, 0x1000",
    "xor edx, edx", // prot: somente leitura
    "mov eax, 5", // SYSCALL_VM_PROTECT
    "syscall",
    "mov byte ptr [rip + .Lupd_st3], 'F'",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + .Lupd_st3], 'P'",
    "1:",
    // ---------- 5) SYSCALL_VM_DECOMMIT(addr, PAGE_SIZE) ----------
    "mov rdi, r12",
    "mov esi, 0x1000",
    "mov eax, 4", // SYSCALL_VM_DECOMMIT
    "syscall",
    "mov byte ptr [rip + .Lupd_st4], 'F'",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + .Lupd_st4], 'P'",
    "1:",
    // ---------- 6) SYSCALL_VM_RELEASE(addr) ----------
    "mov rdi, r12",
    "mov eax, 6", // SYSCALL_VM_RELEASE
    "syscall",
    "mov byte ptr [rip + .Lupd_st5], 'F'",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + .Lupd_st5], 'P'",
    "1:",
    // ---------- 7) mutex: criar, adquirir, liberar ----------
    "mov eax, 7", // SYSCALL_MUTEX_CREATE
    "syscall",
    "mov r13, rax", // r13 = handle do mutex
    "mov byte ptr [rip + .Lupd_st6], 'F'",
    "cmp r13, -1",
    "je 1f",
    "mov rdi, r13",
    "mov eax, 8", // SYSCALL_MUTEX_LOCK
    "syscall",
    "test rax, rax",
    "jnz 1f",
    "mov rdi, r13",
    "mov eax, 9", // SYSCALL_MUTEX_UNLOCK
    "syscall",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + .Lupd_st6], 'P'",
    "1:",
    // ---------- 8) variavel de condicao: criar, notificar um, notificar todos ----------
    "mov eax, 10", // SYSCALL_COND_CREATE
    "syscall",
    "mov r14, rax", // r14 = handle da variavel de condicao
    "mov byte ptr [rip + .Lupd_st7], 'F'",
    "cmp r14, -1",
    "je 1f",
    "mov rdi, r14",
    "mov eax, 12", // SYSCALL_COND_NOTIFY_ONE
    "syscall",
    "test rax, rax",
    "jnz 1f",
    "mov rdi, r14",
    "mov eax, 13", // SYSCALL_COND_NOTIFY_ALL
    "syscall",
    "test rax, rax",
    "jnz 1f",
    "mov byte ptr [rip + .Lupd_st7], 'P'",
    "1:",
    // ---------- 9) SYSCALL_CLOCK_MONOTONIC: deve ser nao-zero ----------
    "mov eax, 15", // SYSCALL_CLOCK_MONOTONIC
    "syscall",
    "mov byte ptr [rip + .Lupd_st8], 'F'",
    "test rax, rax",
    "jz 1f",
    "mov byte ptr [rip + .Lupd_st8], 'P'",
    "1:",
    // ---------- 10) SYSCALL_CLOCK_WALL: deve cair numa faixa plausivel (2024-2035) ----------
    "mov eax, 14", // SYSCALL_CLOCK_WALL
    "syscall",
    "mov byte ptr [rip + .Lupd_st9], 'F'",
    "movabs r11, 0x17a6101701650000", // 2024-01-01T00:00:00Z em nanossegundos desde a epoca
    "cmp rax, r11",
    "jb 1f",
    "movabs r11, 0x1c7767e7ab530000", // 2035-01-01T00:00:00Z em nanossegundos desde a epoca
    "cmp rax, r11",
    "ja 1f",
    "mov byte ptr [rip + .Lupd_st9], 'P'",
    "1:",
    // ---------- 11) SYSCALL_VM_COMMIT com endereco fora de qualquer reserva ----------
    "xor edi, edi", // addr = 0: nunca corresponde a nenhuma reserva viva
    "mov esi, 0x1000",
    "mov edx, 1",
    "mov eax, 3", // SYSCALL_VM_COMMIT
    "syscall",
    "mov byte ptr [rip + .Lupd_st10], 'F'",
    "cmp rax, -1",
    "jne 1f",
    "mov byte ptr [rip + .Lupd_st10], 'P'",
    "1:",
    // ---------- 12) SYSCALL_MUTEX_LOCK com handle inexistente ----------
    "movabs rdi, 0x7fffffffffffffff",
    "mov eax, 8", // SYSCALL_MUTEX_LOCK
    "syscall",
    "mov byte ptr [rip + .Lupd_st11], 'F'",
    "cmp rax, -1",
    "jne 1f",
    "mov byte ptr [rip + .Lupd_st11], 'P'",
    "1:",
    // ---------- reporta o resultado uma unica vez ----------
    "lea rdi, [rip + .Lupd_msg]",
    "mov esi, [rip + .Lupd_msglen]",
    "xor eax, eax", // SYSCALL_WRITE
    "syscall",
    "9:",
    "jmp 9b",
    ".Lupd_msglen: .long .Lupd_msg_end - .Lupd_msg",
    ".Lupd_msg:",
    ".ascii \"user_posix_demo: resultado \"",
    ".Lupd_st0: .ascii \"?\"",
    ".Lupd_st1: .ascii \"?\"",
    ".Lupd_st2: .ascii \"?\"",
    ".Lupd_st3: .ascii \"?\"",
    ".Lupd_st4: .ascii \"?\"",
    ".Lupd_st5: .ascii \"?\"",
    ".Lupd_st6: .ascii \"?\"",
    ".Lupd_st7: .ascii \"?\"",
    ".Lupd_st8: .ascii \"?\"",
    ".Lupd_st9: .ascii \"?\"",
    ".Lupd_st10: .ascii \"?\"",
    ".Lupd_st11: .ascii \"?\"",
    ".ascii \" (ordem: reserve,commit,pattern,protect,decommit,release,mutex,cond,mono,wall,commit_err,mutex_err)\\n\"",
    ".Lupd_msg_end:",
    "user_posix_demo_end:",
);

unsafe extern "C" {
    fn user_posix_demo_start();
    fn user_posix_demo_end();
}

/// Endereço, dentro do binário do kernel, onde a rotina de teste começa —
/// nunca usado diretamente como `RIP` de uma thread, só como origem para
/// [`copy_into`] (mesmo papel de `kernel::user_demo::program_start_addr`).
fn program_start_addr() -> u64 {
    user_posix_demo_start as *const () as u64
}

/// Tamanho, em bytes, da rotina de teste (código e dados embutidos).
fn program_len() -> u64 {
    (user_posix_demo_end as *const () as u64) - (user_posix_demo_start as *const () as u64)
}

/// Copia os bytes da rotina de teste para `dest` — mesmo contrato de
/// [`crate::user_demo::copy_into`].
///
/// # Safety
/// `dest` deve apontar para uma região mapeada, gravável no momento da
/// cópia, com pelo menos [`program_len`] bytes de espaço (uma única página,
/// já que o programa inteiro cabe em menos de 4 KiB).
pub unsafe fn copy_into(dest: u64) {
    debug_assert!(program_len() <= crate::paging::PAGE_SIZE, "user_posix_demo nao cabe numa unica pagina de codigo");
    unsafe {
        core::ptr::copy_nonoverlapping(program_start_addr() as *const u8, dest as *mut u8, program_len() as usize);
    }
}
