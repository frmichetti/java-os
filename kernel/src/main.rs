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

use kernel::{hlt_loop, idle_loop};

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    // Defensivo: interrupções ficam desabilitadas até a IDT/APIC estarem
    // prontos (não há handler próprio antes disso). Reabilitadas no final
    // da sequência de boot, em idle_loop().
    unsafe { core::arch::asm!("cli") };

    unsafe { kernel::gdt::init() };
    kernel::idt::install_default_handlers();
    kernel::idt::install_exception_handlers();
    unsafe { kernel::idt::load() };

    serial::init();

    if !BASE_REVISION.is_supported() {
        kernel::serial_println_emergency!(
            "ERRO FATAL: revisao base do protocolo Limine nao suportada (revisao atual do bootloader: {:?}; maxima suportada por este kernel: {}). Abortando o boot.",
            BASE_REVISION.actual_revision(),
            BaseRevision::MAX_SUPPORTED,
        );
        hlt_loop();
    }

    let Some(hhdm_info) = HHDM_REQUEST.response() else {
        kernel::serial_println_emergency!(
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
            kernel::serial_println_emergency!(
                "ERRO FATAL: falha ao inicializar o alocador de frames fisicos: {:?}. Abortando o boot.",
                e
            );
            hlt_loop();
        }
    }

    // A partir daqui o alocador de frames (e, por consequência, a
    // paginação) já estão prontos: troca-se para a stack dedicada do
    // kernel, protegida por guard page, o quanto antes (ver design.md da
    // change `kernel-stack-guard-page`). O lock do alocador de frames
    // precisa ser liberado (escopo normal, abaixo) antes do salto: como
    // `switch_to` nunca retorna, qualquer guard ainda vivo no momento da
    // chamada nunca seria destruído, deixando o lock preso para sempre.
    let stack_top = {
        let mut frame_allocator = kernel::phys::FRAME_ALLOCATOR.lock();
        match kernel::stack::map(&mut frame_allocator) {
            Ok(top) => top,
            Err(e) => {
                kernel::serial_println_emergency!(
                    "ERRO FATAL: falha ao mapear a stack dedicada do kernel: {:?}. Abortando o boot.",
                    e
                );
                hlt_loop();
            }
        }
    };

    unsafe { kernel::stack::switch_to(stack_top, continue_boot) };
}

/// Continuação de `kernel_main` depois da troca para a stack dedicada do
/// kernel (ver [`kernel::stack::switch_to`]). Não deve depender de nenhum
/// estado local de `kernel_main` — apenas dos `static`s globais já
/// inicializados antes da troca.
extern "C" fn continue_boot() -> ! {
    {
        let mut frame_allocator = kernel::phys::FRAME_ALLOCATOR.lock();
        let mapper = kernel::paging::Mapper::new(&mut frame_allocator);
        kernel::serial_println!(
            "stack do kernel: 0x{:x}-0x{:x}; guard page em 0x{:x} ({})",
            kernel::stack::KERNEL_STACK_BASE,
            kernel::stack::KERNEL_STACK_BASE + kernel::stack::KERNEL_STACK_SIZE,
            kernel::stack::GUARD_PAGE_ADDR,
            match mapper.translate(kernel::stack::GUARD_PAGE_ADDR) {
                None => "nao mapeada, como esperado",
                Some(_) => "MAPEADA - inesperado",
            },
        );
    }

    let heap_init_result = kernel::heap::init(&mut kernel::phys::FRAME_ALLOCATOR.lock());
    match heap_init_result {
        Ok(()) => {
            kernel::serial_println!("heap do kernel: {} bytes mapeados", kernel::heap::mapped_size());
        }
        Err(e) => {
            kernel::serial_println_emergency!(
                "ERRO FATAL: falha ao inicializar o heap do kernel: {:?}. Abortando o boot.",
                e
            );
            hlt_loop();
        }
    }

    if !kernel::apic::supports_x2apic() {
        kernel::serial_println_emergency!(
            "ERRO FATAL: CPU nao suporta x2APIC (exigido por este kernel). Abortando o boot."
        );
        hlt_loop();
    }
    unsafe { kernel::apic::init() };
    kernel::serial_println!("Local APIC (x2APIC) inicializado; PIC 8259 desabilitado");

    unsafe { kernel::timer::init() };
    kernel::serial_println!("timer do APIC: configurado (periodico)");

    // Registra esta própria execução (já sobre a stack dedicada com guard
    // page) como a primeira thread do escalonador, antes de `idle_loop()`
    // habilitar interrupções — ver design.md da change
    // `kernel-process-scheduler`, "A thread 'boot' é o próprio
    // `continue_boot`".
    kernel::scheduler::register_boot_thread();
    kernel::serial_println!("escalonador: iniciado (thread de boot registrada)");

    // Depois da thread de boot já registrada (TSS.rsp0/CURRENT_KERNEL_STACK_TOP
    // corretos — ver design.md da change `kernel-syscalls`).
    unsafe { kernel::syscall::init() };
    kernel::serial_println!("syscall/sysret: configurado (STAR/LSTAR/SFMASK, EFER.SCE)");

    // Processo de demonstração em modo usuário: prova o mecanismo completo
    // de `syscall`/`sysret` ponta a ponta — ver `design.md` da change
    // `kernel-syscalls`, "Primeiro programa de usuário".
    {
        let mut frame_allocator = kernel::phys::FRAME_ALLOCATOR.lock();
        // Antes de QUALQUER `AddressSpace::new()` (criação de processo) —
        // ver `kernel::stack::prime_thread_stack_pool`.
        kernel::stack::prime_thread_stack_pool(&mut frame_allocator)
            .expect("preparar o pool de stacks de thread falhou");
        kernel::scheduler::spawn_user_demo(&mut frame_allocator)
            .expect("spawn do processo de demonstracao em modo usuario falhou");
    }

    idle_loop();
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    kernel::serial_println_emergency!(
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
    kernel::serial_println_emergency!("{}", buf.as_str());
    hlt_loop();
}
