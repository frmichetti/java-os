//! Demonstração de verificação de `kernel::vmm` — análoga a
//! `kernel::storage_demo` (uma sequência única de operações, não um fluxo
//! contínuo): reserva uma faixa, compromete uma sub-região, escreve e lê de
//! volta um padrão conhecido, altera a proteção para somente-leitura,
//! descompromete, libera e reserva de novo para confirmar o reaproveitamento
//! do espaço virtual — ver design.md da change `kernel-posix-virtual-memory`.
//!
//! Deliberadamente não tenta nenhum acesso que deveria gerar `#PF` (isso
//! haltaria o kernel pelo resto do boot, via `idt::page_fault_handler`): os
//! quatro cenários de falta de página da spec (`kernel-virtual-memory`) são
//! verificados manualmente, em boot real, com um gatilho temporário — ver
//! design.md, "Achados durante a implementação", e as tarefas 4.2/4.3.

use crate::paging::PAGE_SIZE;
use crate::vmm::VmmError;

const WRITE_FLAGS: crate::paging::PageFlags =
    crate::paging::PageFlags { writable: true, user_accessible: false, executable: false };
const READ_ONLY_FLAGS: crate::paging::PageFlags =
    crate::paging::PageFlags { writable: false, user_accessible: false, executable: false };

/// Padrão de bytes conhecido escrito na sub-região comprometida, para
/// confirmar que a leitura de volta reflete exatamente o que foi escrito.
const PATTERN: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

fn run_fallible() -> Result<(), VmmError> {
    let region_size = 4 * PAGE_SIZE;
    let commit_len = 2 * PAGE_SIZE;

    let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
    let mut vmm = crate::vmm::VMM.lock();

    let region = vmm.reserve(region_size)?;
    crate::serial_println!(
        "vmm: reservados 0x{:x} bytes em 0x{:x}, sem memoria fisica comprometida",
        region.size(),
        region.base()
    );

    vmm.commit(&mut frame_allocator, &region, 0, commit_len, WRITE_FLAGS)?;
    crate::serial_println!("vmm: 0x{:x} bytes comprometidos em 0x{:x}", commit_len, region.base());

    let ptr = region.base() as *mut u8;
    // A memória recém-comprometida deve vir zerada (cobre "Memória
    // comprometida é entregue zerada" da spec).
    let zeroed_before_write = unsafe { core::slice::from_raw_parts(ptr, PATTERN.len()) } == [0u8; PATTERN.len()];
    unsafe { core::ptr::copy_nonoverlapping(PATTERN.as_ptr(), ptr, PATTERN.len()) };
    let read_back = unsafe { core::slice::from_raw_parts(ptr, PATTERN.len()) };
    if zeroed_before_write && read_back == PATTERN {
        crate::serial_println!("vmm: memoria comprometida veio zerada e o padrao escrito foi lido de volta - PASSOU");
    } else {
        crate::serial_println!(
            "vmm: FALHOU (zerada_antes={}, lido_de_volta={:?}, esperado={:?})",
            zeroed_before_write,
            read_back,
            PATTERN
        );
    }

    vmm.protect(&mut frame_allocator, &region, 0, commit_len, READ_ONLY_FLAGS)?;
    crate::serial_println!("vmm: protecao de 0x{:x} bytes em 0x{:x} alterada para somente-leitura", commit_len, region.base());

    vmm.decommit(&mut frame_allocator, &region, 0, commit_len)?;
    crate::serial_println!("vmm: 0x{:x} bytes descomprometidos em 0x{:x}", commit_len, region.base());

    let released_base = region.base();
    vmm.release(&mut frame_allocator, region)?;
    crate::serial_println!("vmm: reserva em 0x{:x} liberada", released_base);

    // Reaproveitamento do espaço virtual liberado (cobre "Reaproveitamento
    // de Faixas Virtuais Liberadas" da spec) — não depende de nenhum #PF,
    // só da contabilidade pura de `RangeAllocator`.
    let region2 = vmm.reserve(region_size)?;
    if region2.base() == released_base {
        crate::serial_println!("vmm: nova reserva reaproveitou o espaco virtual liberado (base 0x{:x}) - PASSOU", region2.base());
    } else {
        crate::serial_println!(
            "vmm: FALHOU (nova reserva em 0x{:x}, esperava reaproveitar 0x{:x})",
            region2.base(),
            released_base
        );
    }
    vmm.release(&mut frame_allocator, region2)?;

    Ok(())
}

/// Executa a demonstração completa, reportando qualquer erro inesperado
/// (nenhum é esperado no caminho feliz) pelo console serial.
pub fn run() {
    if let Err(e) = run_fallible() {
        crate::serial_println!("vmm: demonstracao falhou inesperadamente: {:?}", e);
    }
}
