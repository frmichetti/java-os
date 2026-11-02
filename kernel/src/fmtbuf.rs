use core::fmt::{self, Write};

/// Buffer de formatação de tamanho fixo em stack — evita a necessidade de um
/// heap allocator apenas para montar mensagens de boot (ver design.md,
/// "Decisões desta Change" — Alocação).
pub struct StackBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StackBuf<N> {
    pub fn new() -> Self {
        Self { buf: [0; N], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("<buffer de boot com bytes invalidos>")
    }
}

impl<const N: usize> Write for StackBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let remaining = N.saturating_sub(self.len);
        let to_copy = bytes.len().min(remaining);
        self.buf[self.len..self.len + to_copy].copy_from_slice(&bytes[..to_copy]);
        self.len += to_copy;
        Ok(())
    }
}
