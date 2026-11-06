//! Verificação em boot real da change `kernel-fpu-context-switch`: duas
//! threads de modo usuário concorrentes, cada uma somando `double` (via
//! `addsd`, instrução SSE real — só possível porque esta change reabilitou
//! SSE no target de usuário e ensinou o escalonador a salvar/restaurar
//! `XMM` por thread), com uma semente e um resultado esperado distintos.
//! Cada laço é longo o bastante para ser preemptado várias vezes pelo
//! timer periódico (100 Hz); se `FXSAVE`/`FXRSTOR` misturassem o estado de
//! FPU entre as duas threads, pelo menos uma delas terminaria com uma soma
//! incorreta.
//!
//! Mesmo padrão de relato de `kernel::user_posix_demo`/
//! `kernel::exit_demo`: cada variante escreve sua própria mensagem de
//! resultado uma única vez (via `SYSCALL_WRITE`) e termina (via
//! `SYSCALL_EXIT`). As duas variantes (`a`/`b`) são blocos de assembly
//! quase idênticos, só com semente/resultado esperado diferentes — rótulos
//! de dados prefixados por variante (`.Lfda_`/`.Lfdb_`) para nunca colidir
//! entre si nem com outros módulos de demonstração — ver `KNOW_ISSUES.md`,
//! "Rótulos de dados sem prefixo em `global_asm!` colidem...".

// Cada variante faz 50.000.000 iterações de `addsd` (soma de `double`) —
// grande o bastante para ser preemptada várias vezes pelo timer a 100 Hz,
// mesmo sob emulação por software (QEMU/TCG). Usando incrementos
// constantes (`1.0`/`3.0`), a soma final é um inteiro exatamente
// representável em `double` (mantissa de 53 bits cobre inteiros até 2^53)
// — comparação exata, sem erro de arredondamento, via `cvtsi2sd` +
// `ucomisd`. Valor literal (não uma constante Rust): `global_asm!` é texto
// puro, sem acesso a itens Rust.
core::arch::global_asm!(
    ".global fpu_demo_a_start",
    ".global fpu_demo_a_end",
    "fpu_demo_a_start:",
    "mov eax, 1000", // semente A = 1000.0
    "cvtsi2sd xmm0, eax",
    "mov eax, 1",
    "cvtsi2sd xmm1, eax", // delta = 1.0
    "mov ecx, 50000000",  // ITERATIONS
    ".Lfda_loop:",
    "addsd xmm0, xmm1",
    "dec ecx",
    "jnz .Lfda_loop",
    "mov eax, 50001000", // esperado: 1000 + ITERATIONS
    "cvtsi2sd xmm2, eax",
    "ucomisd xmm0, xmm2",
    "mov byte ptr [rip + .Lfda_st], 'F'",
    "jne .Lfda_report",
    "mov byte ptr [rip + .Lfda_st], 'P'",
    ".Lfda_report:",
    "lea rdi, [rip + .Lfda_msg]",
    "mov esi, [rip + .Lfda_msglen]",
    "xor eax, eax", // SYSCALL_WRITE
    "syscall",
    "mov eax, 21", // SYSCALL_EXIT
    "syscall",
    "9:",
    "jmp 9b",
    ".Lfda_msglen: .long .Lfda_msg_end - .Lfda_msg",
    ".Lfda_msg:",
    ".ascii \"fpu_demo_a: resultado \"",
    ".Lfda_st: .ascii \"?\"",
    ".ascii \" (soma de double preemptada via SSE/FXSAVE)\\n\"",
    ".Lfda_msg_end:",
    "fpu_demo_a_end:",
);

core::arch::global_asm!(
    ".global fpu_demo_b_start",
    ".global fpu_demo_b_end",
    "fpu_demo_b_start:",
    "mov eax, 2000", // semente B = 2000.0 (distinta de A)
    "cvtsi2sd xmm0, eax",
    "mov eax, 3",
    "cvtsi2sd xmm1, eax", // delta = 3.0 (distinto de A, para que uma mistura de estado produza um resultado visivelmente errado em ambos os lados)
    "mov ecx, 50000000",  // ITERATIONS
    ".Lfdb_loop:",
    "addsd xmm0, xmm1",
    "dec ecx",
    "jnz .Lfdb_loop",
    "mov eax, 150002000", // esperado: 2000 + ITERATIONS * 3 = 2000 + 150000000
    "cvtsi2sd xmm2, eax",
    "ucomisd xmm0, xmm2",
    "mov byte ptr [rip + .Lfdb_st], 'F'",
    "jne .Lfdb_report",
    "mov byte ptr [rip + .Lfdb_st], 'P'",
    ".Lfdb_report:",
    "lea rdi, [rip + .Lfdb_msg]",
    "mov esi, [rip + .Lfdb_msglen]",
    "xor eax, eax", // SYSCALL_WRITE
    "syscall",
    "mov eax, 21", // SYSCALL_EXIT
    "syscall",
    "9:",
    "jmp 9b",
    ".Lfdb_msglen: .long .Lfdb_msg_end - .Lfdb_msg",
    ".Lfdb_msg:",
    ".ascii \"fpu_demo_b: resultado \"",
    ".Lfdb_st: .ascii \"?\"",
    ".ascii \" (soma de double preemptada via SSE/FXSAVE)\\n\"",
    ".Lfdb_msg_end:",
    "fpu_demo_b_end:",
);

unsafe extern "C" {
    fn fpu_demo_a_start();
    fn fpu_demo_a_end();
    fn fpu_demo_b_start();
    fn fpu_demo_b_end();
}

fn copy_variant(start: u64, end: u64, dest: u64) {
    debug_assert!(end - start <= crate::paging::PAGE_SIZE, "fpu_demo nao cabe numa unica pagina de codigo");
    unsafe {
        core::ptr::copy_nonoverlapping(start as *const u8, dest as *mut u8, (end - start) as usize);
    }
}

/// Copia a variante A (semente `1000.0`, delta `1.0`) para `dest` — mesmo
/// contrato de [`crate::user_posix_demo::copy_into`].
///
/// # Safety
/// `dest` deve apontar para uma região mapeada, gravável no momento da
/// cópia, com espaço suficiente (uma única página).
pub unsafe fn copy_into_a(dest: u64) {
    copy_variant(fpu_demo_a_start as *const () as u64, fpu_demo_a_end as *const () as u64, dest);
}

/// Copia a variante B (semente `2000.0`, delta `3.0`) para `dest` — mesmo
/// contrato de [`copy_into_a`].
///
/// # Safety
/// Mesmo contrato de [`copy_into_a`].
pub unsafe fn copy_into_b(dest: u64) {
    copy_variant(fpu_demo_b_start as *const () as u64, fpu_demo_b_end as *const () as u64, dest);
}
