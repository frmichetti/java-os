#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

use core::fmt::Write as _;
use core::panic::PanicInfo;

use kernel::fmtbuf::StackBuf;
use kernel::{memmap_report, serial};

use limine::request::{BootloaderInfoRequest, FramebufferRequest, HhdmRequest, MemmapRequest};
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
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

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

    // Console de framebuffer: inicializado o quanto antes (antes até de
    // HHDM_REQUEST), com base na hipótese de que `Framebuffer::address()`
    // já devolve um ponteiro diretamente utilizável — ver design.md da
    // change `kernel-framebuffer-console`, "Inicialização cedo". A ausência
    // de resposta, de framebuffer, ou um framebuffer rejeitado pela
    // validação são tratados explicitamente: o boot prossegue normalmente,
    // só com o console serial.
    match FRAMEBUFFER_REQUEST.response().and_then(|r| r.framebuffers().first().copied()) {
        Some(fb) => match kernel::framebuffer::init(fb) {
            Ok(()) => kernel::serial_println!(
                "console de framebuffer: inicializado ({}x{}, {} bpp)",
                fb.width,
                fb.height,
                fb.bpp,
            ),
            Err(e) => kernel::serial_println!(
                "console de framebuffer: indisponivel ({:?}); prosseguindo so com o console serial",
                e,
            ),
        },
        None => kernel::serial_println!(
            "console de framebuffer: nenhum framebuffer reportado pelo bootloader; prosseguindo so com o console serial"
        ),
    }

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
    let _ = memmap_report::write_memory_map(&mut serial::MirroredWriter, memmap_entries);

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

    // `kernel::vmm`: depois do heap do kernel pronto, já que sua
    // contabilidade (faixas livres, reservas vivas) usa `alloc::{Vec,
    // BTreeMap}` — ver design.md da change `kernel-posix-virtual-memory`.
    kernel::vmm::VMM.lock().init();
    kernel::serial_println!(
        "vmm: janela de memoria virtual anonima registrada (0x{:x}, {} bytes)",
        kernel::vmm::VMM_WINDOW_START,
        kernel::vmm::VMM_WINDOW_SIZE
    );
    kernel::vmm_demo::run();

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

    // Relógio de parede/monotônico (RTC + TSC calibrado contra o PIT) — ver
    // design.md da change `kernel-wall-clock`. Depois do timer já
    // configurado (o PIT já foi usado para calibração), mas antes de
    // `idle_loop()`/`sti`, mesma janela defendida para o resto do boot.
    kernel::clock::init();
    kernel::clock_demo::run();

    // Registra esta própria execução (já sobre a stack dedicada com guard
    // page) como a primeira thread do escalonador, antes de `idle_loop()`
    // habilitar interrupções — ver design.md da change
    // `kernel-process-scheduler`, "A thread 'boot' é o próprio
    // `continue_boot`".
    kernel::scheduler::register_boot_thread();
    kernel::serial_println!("escalonador: iniciado (thread de boot registrada)");

    // Vetor de software dedicado a `WaitQueue::block_current` — deve ser
    // instalado antes de qualquer bloqueio voluntário de thread (ver
    // design.md da change `kernel-wait-queue`).
    kernel::scheduler::install_block_vector();
    kernel::serial_println!("escalonador: vetor de bloqueio voluntario instalado");

    // Teclado PS/2: depois do escalonador e do Local APIC já estarem
    // prontos — ver `design.md` da change `kernel-keyboard`. Só a
    // inicialização de hardware aqui (controlador 8042, IDT, I/O APIC) —
    // nenhum `AddressSpace::new()` é criado ainda, então pode acontecer
    // antes de `prime_thread_stack_pool` (abaixo). O spawn da thread de
    // demonstração (que cria um processo) é adiado para depois do priming,
    // pela mesma razão do `spawn_user_demo` logo abaixo.
    {
        let mut frame_allocator = kernel::phys::FRAME_ALLOCATOR.lock();
        unsafe { kernel::keyboard::init(&mut frame_allocator) };
    }
    kernel::serial_println!("teclado PS/2: configurado (I/O APIC rotea IRQ1 para o Local APIC)");

    // Armazenamento AHCI: depois do escalonador e do I/O APIC já prontos —
    // ver `design.md` da change `kernel-storage`. Indisponibilidade (sem
    // controlador AHCI, sem dispositivo SATA) é tratada explicitamente:
    // reporta e prossegue o boot normalmente, sem travar.
    {
        let mut frame_allocator = kernel::phys::FRAME_ALLOCATOR.lock();
        match kernel::storage::init(&mut frame_allocator) {
            Ok(()) => {
                kernel::serial_println!("armazenamento: controlador AHCI inicializado");
                drop(frame_allocator);
                kernel::storage_demo::run();
            }
            Err(e) => {
                kernel::serial_println!(
                    "armazenamento: indisponivel ({:?}); prosseguindo sem armazenamento",
                    e
                );
            }
        }
    }

    // Rede (placa e1000): depois do escalonador e do I/O APIC já prontos —
    // ver `design.md` da change `kernel-network`. Indisponibilidade (sem
    // placa de rede encontrada) é tratada explicitamente: reporta e
    // prossegue o boot normalmente, sem travar.
    {
        let mut frame_allocator = kernel::phys::FRAME_ALLOCATOR.lock();
        match kernel::network::init(&mut frame_allocator) {
            Ok(()) => {
                kernel::serial_println!("rede: placa e1000 inicializada");
                drop(frame_allocator);
                kernel::network_demo::run();
            }
            Err(e) => {
                kernel::serial_println!("rede: indisponivel ({:?}); prosseguindo sem rede", e);
            }
        }
    }

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
        // Demonstração das novas syscalls desta change (VM anônima, mutex/
        // condvar nomeados, relógios) — ver design.md da change
        // `kernel-jvm-posix-syscalls`, "Escopo da verificação em boot real".
        kernel::scheduler::spawn_user_posix_demo(&mut frame_allocator)
            .expect("spawn do processo de demonstracao das syscalls posix da jvm falhou");
        // Thread de demonstração do teclado (ver `design.md` da change
        // `kernel-keyboard`): compartilha o espaço de endereçamento da
        // thread de boot (não precisa de isolamento próprio), evitando uma
        // troca de CR3 extra a cada tick do escalonador.
        kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::keyboard_demo::run)
            .expect("spawn da demonstracao de teclado falhou");
        // Demonstração de `kernel::wait_queue` (produtora/consumidora) — ver
        // design.md da change `kernel-wait-queue`.
        kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::wait_demo::producer)
            .expect("spawn da produtora de wait_demo falhou");
        kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::wait_demo::consumer)
            .expect("spawn da consumidora de wait_demo falhou");

        // Demonstração de `kernel::pthread` (Mutex/CondVar) — ver design.md
        // da change `kernel-pthread-sync`.
        for _ in 0..kernel::pthread_demo::COUNTER_THREAD_COUNT {
            kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::pthread_demo::counter_worker)
                .expect("spawn de uma thread do cenario de contador de pthread_demo falhou");
        }
        kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::pthread_demo::box_producer)
            .expect("spawn da produtora da caixa de pthread_demo falhou");
        kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::pthread_demo::box_consumer)
            .expect("spawn da consumidora da caixa de pthread_demo falhou");

        // Demonstração de `kernel::safepoint` (página de poll armada/liberada
        // via `kernel::vmm`) — ver design.md da change
        // `kernel-safepoint-signal`.
        kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::safepoint_demo::coordinator)
            .expect("spawn da coordenadora de safepoint_demo falhou");
        kernel::scheduler::spawn_sharing_current_address_space(&mut frame_allocator, kernel::safepoint_demo::poller)
            .expect("spawn da sondadora de safepoint_demo falhou");
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
