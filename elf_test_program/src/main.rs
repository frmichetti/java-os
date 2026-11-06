//! Executável ELF64 estático de teste do carregador de executáveis do
//! kernel (`kernel::elf`/`kernel::scheduler::spawn_from_elf`) — ver spec
//! `kernel-elf-loader` e `design.md` da change `kernel-elf-loader`,
//! "Programa de teste: binário ELF real, fora da árvore do kernel".
//!
//! Mesma sequência de teste de `kernel::user_demo` (escreve uma mensagem
//! fixa via `syscall` de escrita, consulta ticks, repete em laço), mas como
//! um arquivo ELF real e independente, gravado na imagem de disco e lido
//! pelo carregador em tempo de execução — não embutido no binário do
//! kernel via `global_asm!`. Código independente de posição (só
//! endereçamento relativo a `RIP` e `syscall`/registradores), já que o
//! endereço final de carga é decidido pelo linker deste crate
//! (`linker.ld`), não pelo kernel.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

core::arch::global_asm!(
    ".global _start",
    "_start:",
    // Verifica, antes de qualquer outra coisa, que o BSS (`bss_byte` abaixo,
    // alocado em `.bss` — sem conteudo correspondente no arquivo ELF) foi
    // zerado pelo carregador (`kernel::elf::load_segment` zera cada pagina
    // nova via o alias HHDM antes de copiar o conteudo do arquivo por cima —
    // ver design.md da change `kernel-elf-loader`, "Zeragem explicita do
    // BSS") — cobre o cenario "Segmento de dados com area de BSS" da spec
    // `kernel-elf-loader` em boot real.
    "mov al, [rip + bss_byte]",
    "test al, al",
    "jnz 5f",
    "lea rdi, [rip + bss_ok_msg]",
    "mov esi, [rip + bss_ok_msglen]",
    "jmp 6f",
    "5:",
    "lea rdi, [rip + bss_fail_msg]",
    "mov esi, [rip + bss_fail_msglen]",
    "6:",
    "xor eax, eax", // SYSCALL_WRITE = 0
    "syscall",
    // Verifica o carregador de bibliotecas dinâmicas (`kernel::dylib`) —
    // cobre "Carregamento de Biblioteca Compartilhada...", "Carregamento de
    // Dependências Declaradas..." e "Resolução e Aplicação de
    // Relocações..." da spec `kernel-dynamic-linker` em boot real.
    // `dlopen("/lib/libuser.so")` carrega essa biblioteca e,
    // recursivamente, sua dependência `DT_NEEDED` (`libbase.so` — nomes
    // curtos deliberados, ver design.md, achado sobre FAT32 8.3) antes de
    // aplicar as relocações de `user_call_base_add` (`R_X86_64_JUMP_SLOT`,
    // resolvida contra `base_add` de `libbase.so`). `dlsym` resolve
    // `user_call_base_add`, que é então chamada com (2, 3) — o resultado
    // esperado (5) confirma que a chamada atravessou corretamente a
    // relocação entre as duas bibliotecas. Precisa rodar **antes** do rótulo
    // `2:` (laço infinito abaixo) — código colocado depois dele nunca seria
    // alcançado.
    "lea rdi, [rip + dlopen_path]",
    "mov esi, [rip + dlopen_path_len]",
    "mov eax, 16", // SYSCALL_DLOPEN
    "syscall",
    "mov r12, rax", // handle (ou SYSCALL_ERROR)
    "cmp r12, -1",
    "je dlopen_fail",
    "lea rsi, [rip + dlsym_name]",
    "mov edx, [rip + dlsym_name_len]",
    "mov rdi, r12",
    "mov eax, 17", // SYSCALL_DLSYM
    "syscall",
    "cmp rax, -1",
    "je dlsym_fail",
    "mov r10, rax", // endereco resolvido de user_call_base_add
    "mov edi, 2",
    "mov esi, 3",
    "call r10",
    "cmp rax, 5",
    "je dlcall_ok",
    "lea rdi, [rip + dyn_wrong_result_msg]",
    "mov esi, [rip + dyn_wrong_result_msglen]",
    "jmp dlcall_report",
    "dlcall_ok:",
    "lea rdi, [rip + dyn_ok_msg]",
    "mov esi, [rip + dyn_ok_msglen]",
    "jmp dlcall_report",
    "dlopen_fail:",
    "lea rdi, [rip + dlopen_fail_msg]",
    "mov esi, [rip + dlopen_fail_msglen]",
    "jmp dlcall_report",
    "dlsym_fail:",
    "lea rdi, [rip + dlsym_fail_msg]",
    "mov esi, [rip + dlsym_fail_msglen]",
    "dlcall_report:",
    "xor eax, eax", // SYSCALL_WRITE = 0
    "syscall",
    "2:",
    "lea rdi, [rip + 3f]",     // ptr da mensagem (arg0 da syscall de escrita)
    "mov esi, [rip + msglen]", // tamanho da mensagem (arg1)
    "xor eax, eax",            // SYSCALL_WRITE = 0
    "syscall",
    "mov eax, 1", // SYSCALL_GET_TICKS = 1
    "syscall",
    "jmp 2b",
    "msglen: .long 4f - 3f", // tamanho da mensagem, como dado (nao cabe como imediato direto)
    "3:",
    ".ascii \"ola do executavel ELF real, carregado do disco!\\n\"",
    "4:",
    "bss_ok_msglen: .long bss_ok_msg_end - bss_ok_msg",
    "bss_ok_msg:",
    ".ascii \"BSS: zerado corretamente pelo carregador - PASSOU\\n\"",
    "bss_ok_msg_end:",
    "bss_fail_msglen: .long bss_fail_msg_end - bss_fail_msg",
    "bss_fail_msg:",
    ".ascii \"BSS: NAO estava zerado pelo carregador - FALHOU\\n\"",
    "bss_fail_msg_end:",
    // Dados usados pelo bloco de verificação do carregador dinâmico (ver
    // código logo antes do rótulo `2:`, acima) — colocados aqui, depois do
    // laço principal, seguindo o mesmo padrão de `msglen`/`bss_ok_msg` etc.
    // (nunca executados como instruções, já que `jmp 2b` nunca cai aqui).
    "dlopen_path_len: .long dlopen_path_end - dlopen_path",
    "dlopen_path:",
    ".ascii \"/lib/libuser.so\"",
    "dlopen_path_end:",
    "dlsym_name_len: .long dlsym_name_end - dlsym_name",
    "dlsym_name:",
    ".ascii \"user_call_base_add\"",
    "dlsym_name_end:",
    "dyn_ok_msglen: .long dyn_ok_msg_end - dyn_ok_msg",
    "dyn_ok_msg:",
    ".ascii \"DLOPEN/DLSYM: chamada entre bibliotecas com resultado correto - PASSOU\\n\"",
    "dyn_ok_msg_end:",
    "dyn_wrong_result_msglen: .long dyn_wrong_result_msg_end - dyn_wrong_result_msg",
    "dyn_wrong_result_msg:",
    ".ascii \"DLOPEN/DLSYM: resultado incorreto da chamada entre bibliotecas - FALHOU\\n\"",
    "dyn_wrong_result_msg_end:",
    "dlopen_fail_msglen: .long dlopen_fail_msg_end - dlopen_fail_msg",
    "dlopen_fail_msg:",
    ".ascii \"DLOPEN: falhou ao carregar a biblioteca - FALHOU\\n\"",
    "dlopen_fail_msg_end:",
    "dlsym_fail_msglen: .long dlsym_fail_msg_end - dlsym_fail_msg",
    "dlsym_fail_msg:",
    ".ascii \"DLSYM: simbolo nao resolvido - FALHOU\\n\"",
    "dlsym_fail_msg_end:",
    ".bss",
    ".align 8",
    "bss_byte: .skip 1",
    ".text",
);

/// Nunca deveria ser alcançado (o laço em `_start` é infinito) — presente só
/// para satisfazer o requisito de `#[panic_handler]` de um binário
/// `#![no_std]`. Não usa `hlt` (privilegiada, causaria `#GP` em anel 3):
/// um `nop` em laço apertado é suficiente, já que este código nunca executa
/// de fato.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("nop") };
    }
}
