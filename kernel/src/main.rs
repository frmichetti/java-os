#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

use core::fmt::Write as _;
use core::panic::PanicInfo;

use kernel::fmtbuf::StackBuf;
use kernel::{memmap_report, serial};

use limine::request::{BootloaderInfoRequest, HhdmRequest, MemmapRequest};
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};

const KERNEL_NAME: &str = "Java OS kernel";
const KERNEL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
static BOOTLOADER_INFO_REQUEST: BootloaderInfoRequest = BootloaderInfoRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemmapRequest = MemmapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _REQUESTS_START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

/// Halt definitivo: interrupções desabilitadas, `hlt` em loop. Usado tanto ao
/// final de um boot bem-sucedido quanto a partir do `panic_handler` e de
/// qualquer caminho de erro irrecuperável durante o boot (ver spec,
/// "Comportamento Definido de Halt").
fn hlt_loop() -> ! {
    unsafe { core::arch::asm!("cli") };
    loop {
        unsafe { core::arch::asm!("hlt") }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    // Sem IDT própria ainda (roadmap item 3): qualquer interrupção de
    // hardware que chegasse aqui desviaria a execução para a IDT obsoleta
    // deixada pelo firmware/Limine. Desabilitar interrupções imediatamente
    // evita isso até que uma IDT própria exista.
    unsafe { core::arch::asm!("cli") };

    serial::init();

    if !BASE_REVISION.is_supported() {
        kernel::serial_println!(
            "ERRO FATAL: revisao base do protocolo Limine nao suportada (revisao atual do bootloader: {:?}; maxima suportada por este kernel: {}). Abortando o boot.",
            BASE_REVISION.actual_revision(),
            BaseRevision::MAX_SUPPORTED,
        );
        hlt_loop();
    }

    let Some(hhdm_info) = HHDM_REQUEST.response() else {
        kernel::serial_println!(
            "ERRO FATAL: bootloader nao respondeu a HhdmRequest (higher-half direct map indisponivel). Abortando o boot."
        );
        hlt_loop();
    };
    kernel::hhdm::init(hhdm_info.offset);

    let mut banner = StackBuf::<256>::new();
    match BOOTLOADER_INFO_REQUEST.response() {
        Some(info) => {
            let _ = write!(
                banner,
                "{} v{} - boot OK - bootloader: {} {} (protocolo base rev {:?})",
                KERNEL_NAME,
                KERNEL_VERSION,
                info.name(),
                info.version(),
                BASE_REVISION.actual_revision(),
            );
        }
        None => {
            let _ = write!(
                banner,
                "{} v{} - boot OK - bootloader: <BootloaderInfoRequest sem resposta>",
                KERNEL_NAME, KERNEL_VERSION,
            );
        }
    }
    kernel::serial_println!("{}", banner.as_str());

    let memmap_entries = MEMORY_MAP_REQUEST.response().map(|r| r.entries());
    let _ = memmap_report::write_memory_map(&mut *serial::SERIAL1.lock(), memmap_entries);

    let phys_init_result = kernel::phys::FRAME_ALLOCATOR.lock().init(memmap_entries);
    match phys_init_result {
        Ok(()) => {
            let allocator = kernel::phys::FRAME_ALLOCATOR.lock();
            kernel::serial_println!(
                "memoria fisica: {} frames totais, {} livres",
                allocator.total_frames(),
                allocator.free_frames(),
            );
        }
        Err(e) => {
            kernel::serial_println!(
                "ERRO FATAL: falha ao inicializar o alocador de frames fisicos: {:?}. Abortando o boot.",
                e
            );
            hlt_loop();
        }
    }

    let heap_init_result = kernel::heap::init(&mut kernel::phys::FRAME_ALLOCATOR.lock());
    match heap_init_result {
        Ok(()) => {
            kernel::serial_println!("heap do kernel: {} bytes mapeados", kernel::heap::mapped_size());
        }
        Err(e) => {
            kernel::serial_println!(
                "ERRO FATAL: falha ao inicializar o heap do kernel: {:?}. Abortando o boot.",
                e
            );
            hlt_loop();
        }
    }

    hlt_loop();
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    kernel::serial_println!(
        "ERRO FATAL: falha de alocacao de heap (tamanho={}, alinhamento={}). Abortando o boot.",
        layout.size(),
        layout.align(),
    );
    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut buf = StackBuf::<256>::new();
    let _ = write!(buf, "PANIC: {info}");
    kernel::serial_println!("{}", buf.as_str());
    hlt_loop();
}
