//! Primeira abstração da camada de HAL (hardware abstraction layer) do
//! kernel — ver `design.md` da change `kernel-framebuffer-console`.

/// Qualquer dispositivo que aceita texto como saída — independe de qual
/// dispositivo físico específico o implementa (console serial, console de
/// framebuffer, etc.). Sem métodos próprios nesta primeira versão:
/// `core::fmt::Write` já cobre tudo que as specs atuais exigem.
pub trait Console: core::fmt::Write {}
