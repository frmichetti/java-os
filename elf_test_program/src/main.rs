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
