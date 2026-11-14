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

use crate::process::ProcessId;
use crate::sync::Spinlock;

/// Um arquivo aberto: seu dono, conteúdo completo (lido de uma vez na
/// abertura, ver design.md — Non-Goals: sem leitura incremental por cluster)
/// e a posição do cursor de leitura sequencial dentro dele.
struct OpenFile {
    owner: ProcessId,
    contents: Vec<u8>,
    cursor: usize,
}

static OPEN_FILES: Spinlock<Vec<Option<OpenFile>>> = Spinlock::new(Vec::new());

/// Registra um novo arquivo aberto a partir de seu conteúdo já lido, dono
/// `owner`, devolvendo o `handle` (índice na tabela) pelo qual ele pode ser
/// referenciado por [`read`]/[`close`] (só pelo mesmo `owner` — ver
/// design.md da change `kernel-handle-ownership`). Nunca falha — a tabela só
/// cresce.
pub fn register(contents: Vec<u8>, owner: ProcessId) -> u64 {
    let mut table = OPEN_FILES.lock();
    table.push(Some(OpenFile { owner, contents, cursor: 0 }));
    (table.len() - 1) as u64
}

/// Lê até `len` bytes a partir do cursor atual do arquivo identificado por
/// `handle`, avançando o cursor pela quantidade efetivamente lida. Devolve
/// um vetor vazio (não `None`) ao alcançar o fim do arquivo — isso não é um
/// erro. Devolve `None` se `handle` não corresponder a nenhum arquivo
/// atualmente aberto, ou se `caller` não for o processo que o abriu (mesmo
/// sentinela de erro das duas situações).
pub fn read(handle: u64, len: usize, caller: ProcessId) -> Option<Vec<u8>> {
    let mut table = OPEN_FILES.lock();
    let slot = table.get_mut(handle as usize)?;
    let file = slot.as_mut()?;
    if file.owner != caller {
        return None;
    }
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
/// atualmente aberto pertencente a `caller` (desconhecido, já fechado, ou
/// pertencente a outro processo).
pub fn close(handle: u64, caller: ProcessId) -> bool {
    let mut table = OPEN_FILES.lock();
    let Some(slot) = table.get_mut(handle as usize) else {
        return false;
    };
    if slot.as_ref().is_some_and(|f| f.owner == caller) {
        *slot = None;
        true
    } else {
        false
    }
}

/// Fecha todo arquivo ainda aberto pertencente a `owner` — chamada quando o
/// processo dono termina (`kernel::scheduler::schedule_next`, mesmo ponto
/// que já chama `AddressSpace::free_user_mappings` — ver design.md da change
/// `kernel-handle-ownership`). Devolve quantos arquivos foram fechados, só
/// para diagnóstico (ver `kernel::scheduler::schedule_next`).
pub(crate) fn close_all_owned_by(owner: ProcessId) -> usize {
    let mut table = OPEN_FILES.lock();
    let mut closed = 0;
    for slot in table.iter_mut() {
        if slot.as_ref().is_some_and(|f| f.owner == owner) {
            *slot = None;
            closed += 1;
        }
    }
    closed
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const OWNER: ProcessId = ProcessId(1);
    const OTHER: ProcessId = ProcessId(2);

    #[test]
    fn register_twice_returns_distinct_handles() {
        let a = register(vec![1, 2, 3], OWNER);
        let b = register(vec![4, 5, 6], OWNER);
        assert_ne!(a, b);
    }

    #[test]
    fn read_within_bounds_returns_expected_bytes_and_advances_cursor() {
        let handle = register(vec![1, 2, 3, 4, 5], OWNER);
        assert_eq!(read(handle, 3, OWNER), Some(vec![1, 2, 3]));
        assert_eq!(read(handle, 3, OWNER), Some(vec![4, 5]));
    }

    #[test]
    fn read_past_end_of_file_returns_only_remaining_bytes() {
        let handle = register(vec![1, 2, 3], OWNER);
        assert_eq!(read(handle, 10, OWNER), Some(vec![1, 2, 3]));
    }

    #[test]
    fn read_with_cursor_already_at_end_returns_empty_vec_without_error() {
        let handle = register(vec![1, 2, 3], OWNER);
        assert_eq!(read(handle, 10, OWNER), Some(vec![1, 2, 3]));
        assert_eq!(read(handle, 10, OWNER), Some(vec![]));
    }

    #[test]
    fn read_with_an_unknown_handle_returns_none() {
        let handle = register(vec![1], OWNER);
        assert_eq!(read(handle + 1_000_000, 1, OWNER), None);
    }

    #[test]
    fn read_of_a_file_opened_by_another_process_returns_none() {
        let handle = register(vec![1, 2, 3], OWNER);
        assert_eq!(read(handle, 1, OTHER), None);
    }

    #[test]
    fn two_handles_opened_simultaneously_have_independent_cursors() {
        let a = register(vec![1, 2, 3], OWNER);
        let b = register(vec![9, 9, 9], OWNER);
        assert_eq!(read(a, 1, OWNER), Some(vec![1]));
        // Avançar o cursor de `a` não afeta a posição de `b`, que ainda lê
        // desde o início do seu próprio conteúdo.
        assert_eq!(read(b, 2, OWNER), Some(vec![9, 9]));
        assert_eq!(read(a, 2, OWNER), Some(vec![2, 3]));
    }

    #[test]
    fn close_on_an_open_handle_frees_it_and_a_later_read_returns_none() {
        let handle = register(vec![1, 2, 3], OWNER);
        assert!(close(handle, OWNER));
        assert_eq!(read(handle, 1, OWNER), None);
    }

    #[test]
    fn close_of_a_file_opened_by_another_process_returns_false_without_closing_it() {
        let handle = register(vec![1, 2, 3], OWNER);
        assert!(!close(handle, OTHER));
        // Ainda aberto para o dono de verdade.
        assert_eq!(read(handle, 1, OWNER), Some(vec![1]));
    }

    #[test]
    fn close_with_an_unknown_or_already_closed_handle_returns_false_without_affecting_others() {
        let handle = register(vec![1, 2, 3], OWNER);
        assert!(!close(handle + 1_000_000, OWNER));
        assert!(close(handle, OWNER));
        // Já fechado: uma segunda tentativa de fechar o mesmo handle também
        // falha, em vez de reportar sucesso novamente.
        assert!(!close(handle, OWNER));

        // Um outro handle aberto continua intacto.
        let other = register(vec![9], OWNER);
        assert_eq!(read(other, 1, OWNER), Some(vec![9]));
    }

    #[test]
    fn close_all_owned_by_closes_only_the_requested_owner_s_files() {
        let mine_a = register(vec![1], OWNER);
        let mine_b = register(vec![2], OWNER);
        let theirs = register(vec![3], OTHER);

        assert_eq!(close_all_owned_by(OWNER), 2);

        assert_eq!(read(mine_a, 1, OWNER), None);
        assert_eq!(read(mine_b, 1, OWNER), None);
        // O arquivo de outro processo não foi afetado.
        assert_eq!(read(theirs, 1, OTHER), Some(vec![3]));
    }
}
