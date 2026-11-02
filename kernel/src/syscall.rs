//! Interface `syscall`/`sysret` do kernel — ver `design.md` da change
//! `kernel-syscalls`.

use crate::apic::{rdmsr, wrmsr};
use crate::gdt::KERNEL_CODE_SELECTOR;

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

/// Bit `SCE` (System Call Extensions) do MSR `EFER`: habilita as
/// instruções `syscall`/`sysret`.
const EFER_SCE: u64 = 1 << 0;

/// Bit `IF` de `RFLAGS` — mascarado em `SFMASK` para que interrupções
/// fiquem desabilitadas entre a entrada de `syscall` e a rotina de entrada
/// trocar para uma stack de kernel confiável (ver design.md, "Stack de
/// kernel confiável").
const RFLAGS_IF: u64 = 1 << 9;

/// Base aritmética (não um seletor real) usada para `sysret` no MSR `STAR`:
/// `sysret` calcula `CS = (U + 16) | 3` e `SS = (U + 8) | 3`. Com
/// `U = 0x10`, isso aterrissa exatamente em `USER_CODE_SELECTOR` (0x20|3) e
/// `USER_DATA_SELECTOR` (0x18|3) — ver design.md, "Layout do GDT para
/// `syscall`/`sysret`".
const SYSRET_SELECTOR_BASE: u16 = 0x10;

/// Monta o valor do MSR `STAR`: `K` (base para `syscall`, nos bits 32-47) e
/// `U` (base para `sysret`, nos bits 48-63). Função pura, testável sem
/// hardware.
fn star_value(syscall_cs_base: u16, sysret_selector_base: u16) -> u64 {
    ((syscall_cs_base as u64) << 32) | ((sysret_selector_base as u64) << 48)
}

/// Convenção de ABI desta change (ver design.md): número da chamada em
/// `RAX`, argumentos em `RDI`/`RSI`/`RDX`/`R10` (não `RCX`, que `syscall`
/// sobrescreve), retorno em `RAX`.
pub const SYSCALL_WRITE: u64 = 0;
pub const SYSCALL_GET_TICKS: u64 = 1;

/// Valor devolvido em `RAX` para um número de chamada não reconhecido, em
/// vez de executar qualquer serviço — ver spec `kernel-syscalls`, "Despacho
/// por Número de Chamada".
pub const SYSCALL_ERROR: u64 = u64::MAX;

/// Escreve `len` bytes a partir de `ptr` (endereço do chamador, modo
/// usuário) no console serial do kernel. Devolve a quantidade de bytes
/// escritos.
///
/// # Safety
/// `ptr`/`len` vêm diretamente do chamador em modo usuário, sem validação —
/// ver design.md (Non-Goals de `kernel-process-scheduler`/`kernel-syscalls`:
/// uma exceção causada por um ponteiro inválido do chamador continua
/// haltando o kernel inteiro nesta change, isolamento de falhas fica para
/// uma change futura).
unsafe fn sys_write(ptr: u64, len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    {
        let mut serial = crate::serial::SERIAL1.lock();
        for &byte in bytes {
            serial.write_byte(byte);
        }
    }
    // Espelhamento no console de framebuffer (ver design.md da change
    // `kernel-framebuffer-console`, "Espelhamento"): só para o caso comum de
    // texto UTF-8 válido — bytes arbitrários não decodificáveis não têm uma
    // representação textual sensata para desenhar, mas isso não afeta o
    // contrato da própria syscall (que já escreveu os bytes crus na serial
    // acima, sem essa restrição).
    if let Ok(s) = core::str::from_utf8(bytes) {
        crate::framebuffer::mirror_str(s);
    }
    len
}

fn sys_get_ticks() -> u64 {
    crate::timer::ticks()
}

/// Despacha uma chamada de sistema pelo número (`RAX` na entrada de
/// `syscall`), com até quatro argumentos. Devolve o valor de retorno (`RAX`
/// na saída) — `SYSCALL_ERROR` para um número não reconhecido. `extern "C"`
/// e `#[no_mangle]`: chamada diretamente pela rotina de entrada em
/// assembly ([`syscall_entry`], abaixo), não só por código Rust.
///
/// # Safety
/// Para `SYSCALL_WRITE`, `arg0`/`arg1` são repassados como `ptr`/`len` para
/// [`sys_write`] sem validação — ver lá.
#[unsafe(no_mangle)]
unsafe extern "C" fn dispatch(number: u64, arg0: u64, arg1: u64, _arg2: u64, _arg3: u64) -> u64 {
    match number {
        SYSCALL_WRITE => unsafe { sys_write(arg0, arg1) },
        SYSCALL_GET_TICKS => sys_get_ticks(),
        _ => SYSCALL_ERROR,
    }
}

/// Escrita (pela rotina de entrada em assembly) e leitura (nunca, por
/// enquanto) de um único valor: o `RSP` de modo usuário da chamada em
/// andamento, salvo antes de trocar para a stack de kernel confiável e
/// restaurado logo antes de `sysretq`. Seguro como `static mut` simples
/// (sem lock): o kernel é single-core e `SFMASK` mascara `IF` durante toda
/// a janela em que este valor está "emprestado" (ver design.md — "Stack de
/// kernel confiável"), então não há reentrância possível.
#[unsafe(no_mangle)]
static mut SYSCALL_USER_RSP_SCRATCH: u64 = 0;

core::arch::global_asm!(
    ".global syscall_entry",
    "syscall_entry:",
    // RCX = RIP de retorno, R11 = RFLAGS do chamador (hardware). RSP ainda
    // é o do usuário (não confiável) — salva antes de qualquer outra coisa.
    "mov [SYSCALL_USER_RSP_SCRATCH], rsp",
    "mov rsp, [CURRENT_KERNEL_STACK_TOP]",
    // RSP acabou de ser realinhado a partir de um topo de stack (múltiplo
    // de 16); dois `push` de 8 bytes mantêm o alinhamento exigido pela ABI
    // antes do `call` abaixo — sem precisar de nenhum ajuste extra, ao
    // contrário de `scheduler::timer_entry` (que herda um RSP de
    // alinhamento imprevisível de uma interrupção assíncrona).
    "push rcx", // RIP do usuário
    "push r11", // RFLAGS do usuário
    // Reorganiza os argumentos da convenção desta syscall (número em RAX,
    // argumentos em RDI/RSI/RDX/R10) para a convenção System V de
    // `dispatch` (RDI/RSI/RDX/RCX/R8) — do último parâmetro para o
    // primeiro, para nunca sobrescrever uma fonte antes de lê-la.
    "mov r8, r10",
    "mov rcx, rdx",
    "mov rdx, rsi",
    "mov rsi, rdi",
    "mov rdi, rax",
    "call dispatch",
    "pop r11",
    "pop rcx",
    "mov rsp, [SYSCALL_USER_RSP_SCRATCH]",
    "sysretq",
);

unsafe extern "C" {
    fn syscall_entry();
}

/// Configura `STAR`/`LSTAR`/`SFMASK` e habilita `EFER.SCE`, permitindo que
/// código em modo usuário invoque `syscall`, atendida por [`syscall_entry`].
///
/// # Safety
/// Deve ser chamada uma única vez, depois de `gdt::init()` (os seletores de
/// usuário/kernel referenciados por `STAR` dependem do GDT já estar
/// montado).
pub unsafe fn init() {
    unsafe {
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | EFER_SCE);

        wrmsr(IA32_STAR, star_value(KERNEL_CODE_SELECTOR, SYSRET_SELECTOR_BASE));
        wrmsr(IA32_LSTAR, syscall_entry as *const () as u64);
        wrmsr(IA32_FMASK, RFLAGS_IF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdt::{USER_CODE_SELECTOR, USER_DATA_SELECTOR};

    #[test]
    fn star_value_places_syscall_and_sysret_bases_in_the_right_bit_fields() {
        let value = star_value(KERNEL_CODE_SELECTOR, SYSRET_SELECTOR_BASE);
        assert_eq!((value >> 32) & 0xffff, KERNEL_CODE_SELECTOR as u64);
        assert_eq!((value >> 48) & 0xffff, SYSRET_SELECTOR_BASE as u64);
    }

    #[test]
    fn sysret_selector_base_lands_on_the_user_segments() {
        // sysret (64 bits): CS = (U + 16) | 3, SS = (U + 8) | 3 — ver design.md.
        let cs = (SYSRET_SELECTOR_BASE + 16) | 3;
        let ss = (SYSRET_SELECTOR_BASE + 8) | 3;
        assert_eq!(cs, USER_CODE_SELECTOR);
        assert_eq!(ss, USER_DATA_SELECTOR);
    }

    #[test]
    fn syscall_cs_base_lands_on_kernel_segments() {
        // syscall: CS = K, SS = K + 8 — deve bater com os seletores de kernel já existentes.
        assert_eq!(KERNEL_CODE_SELECTOR, crate::gdt::KERNEL_CODE_SELECTOR);
        assert_eq!(KERNEL_CODE_SELECTOR + 8, crate::gdt::KERNEL_DATA_SELECTOR);
    }

    #[test]
    fn unknown_syscall_number_returns_error_sentinel() {
        let result = unsafe { dispatch(0xdead_beef, 0, 0, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn get_ticks_dispatches_to_the_real_tick_counter() {
        let before = crate::timer::ticks();
        let result = unsafe { dispatch(SYSCALL_GET_TICKS, 0, 0, 0, 0) };
        assert_eq!(result, before, "GET_TICKS deve devolver o contador real do kernel no momento da chamada");
    }

    // `SYSCALL_WRITE` não é testado aqui: `sys_write` escreve na UART via
    // `outb` (instrução de I/O privilegiada) — executá-la em host (anel 3)
    // causaria um crash do próprio processo de teste, mesma limitação já
    // documentada para `Mapper::map`/`AddressSpace::new` (que tocam
    // `CR3`/HHDM reais). Verificado em boot real na tarefa 6.1.
}
