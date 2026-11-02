//! Demonstração de verificação do driver de teclado — análoga ao
//! `kernel::user_demo` de `kernel-syscalls`, mas em modo kernel (sem motivo
//! para modo usuário aqui, já que não há syscall de teclado nem isolamento
//! a provar). Só ecoa os eventos recebidos nos consoles existentes
//! (serial + framebuffer, via `serial_println!`) — não é um shell real, ver
//! design.md da change `kernel-keyboard`, "Demonstração".

/// Thread de kernel: consulta `keyboard::poll_event()` em loop e imprime
/// cada evento com caractere ASCII presente. Criada via
/// `scheduler::spawn(frame_allocator, keyboard_demo::run)` em
/// `continue_boot`.
pub extern "C" fn run() -> ! {
    loop {
        if let Some(event) = crate::keyboard::poll_event() {
            if let Some(ascii) = event.ascii {
                let estado = if event.pressed { "pressionada" } else { "solta" };
                crate::serial_println!("teclado: '{}' (0x{:02x}) {}", ascii as char, ascii, estado);
            }
        } else {
            // Achado durante a implementação (ver design.md, "IRQ1 nunca
            // chegava a ser entregue de verdade"): fazer polling num loop
            // apertado, sem nenhuma pausa, mascara/desmascara interrupções a
            // cada chamada de `poll_event` com tanta frequência que o QEMU
            // (TCG) praticamente nunca encontrava uma janela real para
            // reconhecer a IRQ1 — o timer, que não depende desse
            // vaivém, continuava funcionando normalmente. `hlt` (com
            // interrupções habilitadas) resolve isso: para a CPU até a
            // próxima interrupção de qualquer tipo, dando uma janela de
            // reconhecimento de verdade, sem gastar ciclos à toa enquanto
            // não há evento algum.
            unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
        }
    }
}
