//! Primeira abstração da camada de HAL (hardware abstraction layer) do
//! kernel — ver `design.md` da change `kernel-framebuffer-console`.

/// Qualquer dispositivo que aceita texto como saída — independe de qual
/// dispositivo físico específico o implementa (console serial, console de
/// framebuffer, etc.). Sem métodos próprios nesta primeira versão:
/// `core::fmt::Write` já cobre tudo que as specs atuais exigem.
pub trait Console: core::fmt::Write {}

/// Qualquer dispositivo que produz eventos de tecla — independe de qual
/// dispositivo físico específico o implementa (hoje só o teclado PS/2,
/// `kernel::keyboard::Ps2Keyboard`). Único método, não-bloqueante: o kernel
/// ainda não tem nenhuma primitiva de bloqueio/espera de thread, então
/// "nenhum evento pendente" é representado por `None`, não por esperar —
/// ver `design.md` da change `kernel-keyboard`.
pub trait KeyboardDevice {
    fn poll_event(&mut self) -> Option<crate::keyboard::KeyEvent>;
}
