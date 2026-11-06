//! Tabela de identificador (`handle`) opaco para arquivos abertos via
//! `SYSCALL_OPEN`, com um cursor de leitura sequencial por handle — ver
//! `design.md` da change `kernel-file-syscalls`, "Estrutura da tabela" e
//! "Liberação de memória ao fechar".
//!
//! Ao contrário de `kernel::pthread_handles` (cujos `Mutex`/`CondVar`
//! precisam de referência `'static` e por isso nunca são destruídos), o
//! conteúdo de um arquivo aberto é só um `Vec<u8>` lido uma vez — não
//! precisa de `Box::leak`, e [`close`] libera essa memória imediatamente.
//! A tabela em si só cresce (nenhum índice é reaproveitado, mesma política
//! de `pthread_handles`, para evitar que um handle antigo passe a apontar
//! silenciosamente para um arquivo diferente — ver design.md, alternativa
//! rejeitada "reaproveitar slot livre").

extern crate alloc;

use alloc::vec::Vec;

use crate::sync::Spinlock;

/// Um arquivo aberto: seu conteúdo completo (lido de uma vez na abertura,
/// ver design.md — Non-Goals: sem leitura incremental por cluster) e a
/// posição do cursor de leitura sequencial dentro dele.
struct OpenFile {
    contents: Vec<u8>,
    cursor: usize,
}

static OPEN_FILES: Spinlock<Vec<Option<OpenFile>>> = Spinlock::new(Vec::new());

/// Registra um novo arquivo aberto a partir de seu conteúdo já lido,
/// devolvendo o `handle` (índice na tabela) pelo qual ele pode ser
/// referenciado por [`read`]/[`close`]. Nunca falha — a tabela só cresce.
pub fn register(contents: Vec<u8>) -> u64 {
    let mut table = OPEN_FILES.lock();
    table.push(Some(OpenFile { contents, cursor: 0 }));
    (table.len() - 1) as u64
}

/// Lê até `len` bytes a partir do cursor atual do arquivo identificado por
/// `handle`, avançando o cursor pela quantidade efetivamente lida. Devolve
/// um vetor vazio (não `None`) ao alcançar o fim do arquivo — isso não é um
/// erro. Devolve `None` se `handle` não corresponder a nenhum arquivo
/// atualmente aberto.
pub fn read(handle: u64, len: usize) -> Option<Vec<u8>> {
    let mut table = OPEN_FILES.lock();
    let slot = table.get_mut(handle as usize)?;
    let file = slot.as_mut()?;
    let remaining = file.contents.len() - file.cursor;
    let amount = len.min(remaining);
    let start = file.cursor;
    let bytes = file.contents[start..start + amount].to_vec();
    file.cursor += amount;
    Some(bytes)
}

/// Fecha o arquivo identificado por `handle`, liberando seu conteúdo e
/// tornando o handle inválido para operações futuras. Devolve `true` em
/// sucesso, `false` se `handle` não corresponder a nenhum arquivo
/// atualmente aberto (desconhecido ou já fechado).
pub fn close(handle: u64) -> bool {
    let mut table = OPEN_FILES.lock();
    match table.get_mut(handle as usize) {
        Some(slot @ Some(_)) => {
            *slot = None;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn register_twice_returns_distinct_handles() {
        let a = register(vec![1, 2, 3]);
        let b = register(vec![4, 5, 6]);
        assert_ne!(a, b);
    }

    #[test]
    fn read_within_bounds_returns_expected_bytes_and_advances_cursor() {
        let handle = register(vec![1, 2, 3, 4, 5]);
        assert_eq!(read(handle, 3), Some(vec![1, 2, 3]));
        assert_eq!(read(handle, 3), Some(vec![4, 5]));
    }

    #[test]
    fn read_past_end_of_file_returns_only_remaining_bytes() {
        let handle = register(vec![1, 2, 3]);
        assert_eq!(read(handle, 10), Some(vec![1, 2, 3]));
    }

    #[test]
    fn read_with_cursor_already_at_end_returns_empty_vec_without_error() {
        let handle = register(vec![1, 2, 3]);
        assert_eq!(read(handle, 10), Some(vec![1, 2, 3]));
        assert_eq!(read(handle, 10), Some(vec![]));
    }

    #[test]
    fn read_with_an_unknown_handle_returns_none() {
        let handle = register(vec![1]);
        assert_eq!(read(handle + 1_000_000, 1), None);
    }

    #[test]
    fn two_handles_opened_simultaneously_have_independent_cursors() {
        let a = register(vec![1, 2, 3]);
        let b = register(vec![9, 9, 9]);
        assert_eq!(read(a, 1), Some(vec![1]));
        // Avançar o cursor de `a` não afeta a posição de `b`, que ainda lê
        // desde o início do seu próprio conteúdo.
        assert_eq!(read(b, 2), Some(vec![9, 9]));
        assert_eq!(read(a, 2), Some(vec![2, 3]));
    }

    #[test]
    fn close_on_an_open_handle_frees_it_and_a_later_read_returns_none() {
        let handle = register(vec![1, 2, 3]);
        assert!(close(handle));
        assert_eq!(read(handle, 1), None);
    }

    #[test]
    fn close_with_an_unknown_or_already_closed_handle_returns_false_without_affecting_others() {
        let handle = register(vec![1, 2, 3]);
        assert!(!close(handle + 1_000_000));
        assert!(close(handle));
        // Já fechado: uma segunda tentativa de fechar o mesmo handle também
        // falha, em vez de reportar sucesso novamente.
        assert!(!close(handle));

        // Um outro handle aberto continua intacto.
        let other = register(vec![9]);
        assert_eq!(read(other, 1), Some(vec![9]));
    }
}
