//! Interface `syscall`/`sysret` do kernel — ver `design.md` da change
//! `kernel-syscalls`.

extern crate alloc;

use crate::apic::{rdmsr, wrmsr};
use crate::gdt::KERNEL_CODE_SELECTOR;
use crate::paging::PageFlags;
use crate::vmm::VMM;

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

/// Bit `SCE` (System Call Extensions) do MSR `EFER`: habilita as
/// instruções `syscall`/`sysret`.
const EFER_SCE: u64 = 1 << 0;

/// Bit `IF` de `RFLAGS` — mascarado em `SFMASK` para que interrupções
/// fiquem desabilitadas entre a entrada de `syscall` e a rotina de entrada
/// trocar para uma stack de kernel confiável (ver design.md, "Stack de
/// kernel confiável").
const RFLAGS_IF: u64 = 1 << 9;

/// Base aritmética (não um seletor real) usada para `sysret` no MSR `STAR`:
/// `sysret` calcula `CS = (U + 16) | 3` e `SS = (U + 8) | 3`. Com
/// `U = 0x10`, isso aterrissa exatamente em `USER_CODE_SELECTOR` (0x20|3) e
/// `USER_DATA_SELECTOR` (0x18|3) — ver design.md, "Layout do GDT para
/// `syscall`/`sysret`".
const SYSRET_SELECTOR_BASE: u16 = 0x10;

/// Monta o valor do MSR `STAR`: `K` (base para `syscall`, nos bits 32-47) e
/// `U` (base para `sysret`, nos bits 48-63). Função pura, testável sem
/// hardware.
fn star_value(syscall_cs_base: u16, sysret_selector_base: u16) -> u64 {
    ((syscall_cs_base as u64) << 32) | ((sysret_selector_base as u64) << 48)
}

/// Convenção de ABI desta change (ver design.md): número da chamada em
/// `RAX`, argumentos em `RDI`/`RSI`/`RDX`/`R10` (não `RCX`, que `syscall`
/// sobrescreve), retorno em `RAX`.
pub const SYSCALL_WRITE: u64 = 0;
pub const SYSCALL_GET_TICKS: u64 = 1;

/// Ver design.md da change `kernel-jvm-posix-syscalls`, "Numeração de
/// syscalls" — tabela completa de argumentos/retorno de cada uma.
pub const SYSCALL_VM_RESERVE: u64 = 2;
pub const SYSCALL_VM_COMMIT: u64 = 3;
pub const SYSCALL_VM_DECOMMIT: u64 = 4;
pub const SYSCALL_VM_PROTECT: u64 = 5;
pub const SYSCALL_VM_RELEASE: u64 = 6;
pub const SYSCALL_MUTEX_CREATE: u64 = 7;
pub const SYSCALL_MUTEX_LOCK: u64 = 8;
pub const SYSCALL_MUTEX_UNLOCK: u64 = 9;
pub const SYSCALL_COND_CREATE: u64 = 10;
pub const SYSCALL_COND_WAIT: u64 = 11;
pub const SYSCALL_COND_NOTIFY_ONE: u64 = 12;
pub const SYSCALL_COND_NOTIFY_ALL: u64 = 13;
pub const SYSCALL_CLOCK_WALL: u64 = 14;
pub const SYSCALL_CLOCK_MONOTONIC: u64 = 15;

/// Ver design.md da change `kernel-dynamic-linker`. `SYSCALL_DLOPEN`:
/// `arg0`/`arg1` = ponteiro/tamanho do caminho (convenção de
/// `SYSCALL_WRITE`); retorno = `handle` opaco da biblioteca carregada no
/// espaço de endereçamento do chamador, ou `SYSCALL_ERROR`.
/// `SYSCALL_DLSYM`: `arg0` = `handle` devolvido por `SYSCALL_DLOPEN`;
/// `arg1`/`arg2` = ponteiro/tamanho do nome do símbolo; retorno = endereço
/// resolvido, ou `SYSCALL_ERROR`.
pub const SYSCALL_DLOPEN: u64 = 16;
pub const SYSCALL_DLSYM: u64 = 17;

/// Ver design.md da change `kernel-file-syscalls`, "Numeração de syscalls".
/// `SYSCALL_OPEN`: `arg0`/`arg1` = ponteiro/tamanho do caminho absoluto
/// (convenção de `SYSCALL_WRITE`/`SYSCALL_DLOPEN`); retorno = `handle`
/// opaco do arquivo aberto, ou `SYSCALL_ERROR`. `SYSCALL_READ`: `arg0` =
/// `handle` devolvido por `SYSCALL_OPEN`; `arg1`/`arg2` = ponteiro/tamanho
/// do buffer do chamador; retorno = quantidade de bytes efetivamente
/// lidos (`0` no fim do arquivo, não um erro), ou `SYSCALL_ERROR` para
/// `handle` desconhecido. `SYSCALL_CLOSE`: `arg0` = `handle`; retorno =
/// `0`, ou `SYSCALL_ERROR` para `handle` desconhecido.
pub const SYSCALL_OPEN: u64 = 18;
pub const SYSCALL_READ: u64 = 19;
pub const SYSCALL_CLOSE: u64 = 20;

/// Ver design.md da change `libc-minimal-port`, "`SYSCALL_EXIT` e remoção
/// permanente de `ReadyQueue`". Sem argumentos, nunca retorna — a thread
/// chamadora é removida permanentemente da rotação do escalonador.
pub const SYSCALL_EXIT: u64 = 21;

/// Ver design.md da change `os-javaos-platform-sync`. `arg0` = `handle` de
/// mutex devolvido por `SYSCALL_MUTEX_CREATE`; retorno = `0` se adquirido
/// agora, `SYSCALL_ERROR` se já estava tomado ou `handle` desconhecido —
/// nunca bloqueia a thread chamadora, ao contrário de `SYSCALL_MUTEX_LOCK`.
pub const SYSCALL_MUTEX_TRYLOCK: u64 = 22;

/// Valor devolvido em `RAX` para um número de chamada não reconhecido, em
/// vez de executar qualquer serviço — ver spec `kernel-syscalls`, "Despacho
/// por Número de Chamada".
pub const SYSCALL_ERROR: u64 = u64::MAX;

/// Valor devolvido em `RAX` por `SYSCALL_COND_WAIT` quando o prazo pedido
/// (terceiro argumento, `timeout_nanos`) expira antes de qualquer
/// notificação — distinto de `SYSCALL_ERROR` (handle desconhecido) e de `0`
/// (notificada) — ver design.md da change `kernel-timed-wait`.
pub const SYSCALL_TIMEOUT: u64 = u64::MAX - 1;

/// Escreve `len` bytes a partir de `ptr` (endereço do chamador, modo
/// usuário) no console serial do kernel. Devolve a quantidade de bytes
/// escritos.
///
/// # Safety
/// `ptr`/`len` vêm diretamente do chamador em modo usuário, sem validação —
/// ver design.md (Non-Goals de `kernel-process-scheduler`/`kernel-syscalls`:
/// uma exceção causada por um ponteiro inválido do chamador continua
/// haltando o kernel inteiro nesta change, isolamento de falhas fica para
/// uma change futura).
unsafe fn sys_write(ptr: u64, len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    {
        let mut serial = crate::serial::SERIAL1.lock();
        for &byte in bytes {
            serial.write_byte(byte);
        }
    }
    // Espelhamento no console de framebuffer (ver design.md da change
    // `kernel-framebuffer-console`, "Espelhamento"): só para o caso comum de
    // texto UTF-8 válido — bytes arbitrários não decodificáveis não têm uma
    // representação textual sensata para desenhar, mas isso não afeta o
    // contrato da própria syscall (que já escreveu os bytes crus na serial
    // acima, sem essa restrição).
    if let Ok(s) = core::str::from_utf8(bytes) {
        crate::framebuffer::mirror_str(s);
    }
    len
}

fn sys_get_ticks() -> u64 {
    crate::timer::ticks()
}

/// Decodifica a máscara `prot` de uma chamada de VM (bit 0 = gravável, bit 1
/// = executável; demais bits ignorados) em `PageFlags`, forçando sempre
/// `user_accessible: true` — um chamador de modo usuário nunca pode, através
/// desta syscall, pedir nem receber uma página inacessível a partir do anel
/// 3, independentemente do valor bruto recebido. Função pura, testável sem
/// hardware — ver design.md, "Permissões de página".
fn prot_to_page_flags(prot: u64) -> PageFlags {
    PageFlags { writable: prot & 0b01 != 0, user_accessible: true, executable: prot & 0b10 != 0 }
}

/// Reserva `size` bytes de memória virtual anônima — cobre
/// `SYSCALL_VM_RESERVE`. Devolve o endereço base da reserva, ou
/// `SYSCALL_ERROR` (tamanho zero, ou sem espaço virtual livre suficiente).
fn sys_vm_reserve(size: u64) -> u64 {
    match VMM.lock().reserve(size) {
        Ok(region) => region.base(),
        Err(_) => SYSCALL_ERROR,
    }
}

/// Compromete `len` bytes a partir de `addr` (que deve cair dentro de uma
/// reserva viva) com as permissões decodificadas de `prot` — cobre
/// `SYSCALL_VM_COMMIT`. `addr` é traduzido para o `Region` que o contém via
/// `Vmm::region_containing` (ver design.md, "VM por endereço"); qualquer
/// endereço fora de uma reserva viva, ou condição de `VmmError`, devolve
/// `SYSCALL_ERROR`.
fn sys_vm_commit(addr: u64, len: u64, prot: u64) -> u64 {
    let flags = prot_to_page_flags(prot);
    // Mesma ordem de aquisição de `kernel::vmm_demo` (frame allocator antes
    // de `VMM`) — evitar inversão de ordem de locks entre os dois pontos do
    // kernel que tocam ambos.
    let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
    let mut vmm = VMM.lock();
    let Some(region) = vmm.region_containing(addr) else {
        return SYSCALL_ERROR;
    };
    let offset = addr - region.base();
    match vmm.commit(&mut frame_allocator, &region, offset, len, flags) {
        Ok(()) => 0,
        Err(_) => SYSCALL_ERROR,
    }
}

/// Descompromete `len` bytes a partir de `addr` — cobre
/// `SYSCALL_VM_DECOMMIT`. Mesma tradução addr→`Region` de [`sys_vm_commit`].
fn sys_vm_decommit(addr: u64, len: u64) -> u64 {
    let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
    let mut vmm = VMM.lock();
    let Some(region) = vmm.region_containing(addr) else {
        return SYSCALL_ERROR;
    };
    let offset = addr - region.base();
    match vmm.decommit(&mut frame_allocator, &region, offset, len) {
        Ok(()) => 0,
        Err(_) => SYSCALL_ERROR,
    }
}

/// Altera as permissões de `len` bytes a partir de `addr` para as
/// decodificadas de `prot` — cobre `SYSCALL_VM_PROTECT`. Mesma tradução
/// addr→`Region` de [`sys_vm_commit`].
fn sys_vm_protect(addr: u64, len: u64, prot: u64) -> u64 {
    let flags = prot_to_page_flags(prot);
    let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
    let mut vmm = VMM.lock();
    let Some(region) = vmm.region_containing(addr) else {
        return SYSCALL_ERROR;
    };
    let offset = addr - region.base();
    match vmm.protect(&mut frame_allocator, &region, offset, len, flags) {
        Ok(()) => 0,
        Err(_) => SYSCALL_ERROR,
    }
}

/// Libera por completo a reserva cuja base é exatamente `addr` — cobre
/// `SYSCALL_VM_RELEASE`. Ao contrário de `commit`/`decommit`/`protect`
/// (que aceitam qualquer endereço dentro da reserva), `release` exige a
/// base exata: um endereço que caia no meio de uma reserva viva, mas não
/// seja sua base, é rejeitado (mesma convenção de `munmap`, que opera sobre
/// o mapeamento inteiro identificado pelo seu endereço inicial) — ver
/// design.md, tabela de numeração.
fn sys_vm_release(addr: u64) -> u64 {
    let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
    let mut vmm = VMM.lock();
    let Some(region) = vmm.region_containing(addr) else {
        return SYSCALL_ERROR;
    };
    if region.base() != addr {
        return SYSCALL_ERROR;
    }
    match vmm.release(&mut frame_allocator, region) {
        Ok(()) => 0,
        Err(_) => SYSCALL_ERROR,
    }
}

/// Cria um novo mutex nomeado — cobre `SYSCALL_MUTEX_CREATE`. Nunca falha.
fn sys_mutex_create() -> u64 {
    crate::pthread_handles::create_mutex()
}

/// Adquire o mutex identificado por `handle`, bloqueando a thread chamadora
/// (sem busy-wait) se já estiver tomado por outra thread — cobre
/// `SYSCALL_MUTEX_LOCK`. `SYSCALL_ERROR` se `handle` não corresponder a
/// nenhum mutex criado.
fn sys_mutex_lock(handle: u64) -> u64 {
    match crate::pthread_handles::mutex(handle) {
        Some(mutex) => {
            mutex.lock();
            0
        }
        None => SYSCALL_ERROR,
    }
}

/// Tenta adquirir o mutex identificado por `handle` sem bloquear a thread
/// chamadora — cobre `SYSCALL_MUTEX_TRYLOCK`. `0` se adquirido agora,
/// `SYSCALL_ERROR` se já estava tomado (por esta ou por outra thread) ou se
/// `handle` não corresponder a nenhum mutex criado.
fn sys_mutex_trylock(handle: u64) -> u64 {
    match crate::pthread_handles::mutex(handle) {
        Some(mutex) => {
            if mutex.try_acquire() {
                0
            } else {
                SYSCALL_ERROR
            }
        }
        None => SYSCALL_ERROR,
    }
}

/// Libera o mutex identificado por `handle`, acordando uma thread em espera
/// para adquiri-lo, se houver — cobre `SYSCALL_MUTEX_UNLOCK`. `SYSCALL_ERROR`
/// se `handle` não corresponder a nenhum mutex criado.
fn sys_mutex_unlock(handle: u64) -> u64 {
    match crate::pthread_handles::mutex(handle) {
        Some(mutex) => {
            mutex.unlock();
            0
        }
        None => SYSCALL_ERROR,
    }
}

/// Cria uma nova variável de condição nomeada — cobre `SYSCALL_COND_CREATE`.
/// Nunca falha.
fn sys_cond_create() -> u64 {
    crate::pthread_handles::create_cond()
}

/// Libera `mutex_handle` e bloqueia a thread chamadora em `cond_handle`,
/// reaquirindo o mesmo mutex antes de retornar quando notificada ou quando
/// `timeout_nanos` expirar — cobre `SYSCALL_COND_WAIT`. `timeout_nanos == 0`
/// preserva exatamente o comportamento original (espera indefinida, mesmo
/// valor que todo chamador já existente desta syscall sempre passou aqui) —
/// ver design.md da change `kernel-timed-wait`. `SYSCALL_ERROR` se
/// `cond_handle` ou `mutex_handle` não corresponderem a objetos criados, sem
/// bloquear a thread chamadora nem alterar o estado do mutex nesse caso;
/// `SYSCALL_TIMEOUT` se `timeout_nanos > 0` e o prazo expirar antes de
/// qualquer notificação.
fn sys_cond_wait(cond_handle: u64, mutex_handle: u64, timeout_nanos: u64) -> u64 {
    let Some(cond) = crate::pthread_handles::cond(cond_handle) else {
        return SYSCALL_ERROR;
    };
    let Some(mutex) = crate::pthread_handles::mutex(mutex_handle) else {
        return SYSCALL_ERROR;
    };
    if timeout_nanos == 0 {
        cond.wait(mutex);
        return 0;
    }
    let deadline = crate::clock::monotonic_nanos() + timeout_nanos;
    if cond.wait_timeout(mutex, deadline) { 0 } else { SYSCALL_TIMEOUT }
}

/// Acorda a thread esperando há mais tempo na variável de condição
/// identificada por `handle`, se houver — cobre `SYSCALL_COND_NOTIFY_ONE`.
/// `SYSCALL_ERROR` se `handle` não corresponder a nenhuma variável de
/// condição criada.
fn sys_cond_notify_one(handle: u64) -> u64 {
    match crate::pthread_handles::cond(handle) {
        Some(cond) => {
            cond.notify_one();
            0
        }
        None => SYSCALL_ERROR,
    }
}

/// Acorda todas as threads atualmente esperando na variável de condição
/// identificada por `handle` — cobre `SYSCALL_COND_NOTIFY_ALL`.
/// `SYSCALL_ERROR` se `handle` não corresponder a nenhuma variável de
/// condição criada.
fn sys_cond_notify_all(handle: u64) -> u64 {
    match crate::pthread_handles::cond(handle) {
        Some(cond) => {
            cond.notify_all();
            0
        }
        None => SYSCALL_ERROR,
    }
}

/// Consulta o relógio de parede — cobre `SYSCALL_CLOCK_WALL`. Nunca falha.
fn sys_clock_wall() -> u64 {
    crate::clock::wall_clock_nanos()
}

/// Consulta o relógio monotônico — cobre `SYSCALL_CLOCK_MONOTONIC`. Nunca
/// falha.
fn sys_clock_monotonic() -> u64 {
    crate::clock::monotonic_nanos()
}

/// Carrega a biblioteca compartilhada ELF64 (`ET_DYN`) cujo caminho está em
/// `ptr`/`len` no espaço de endereçamento do chamador — cobre
/// `SYSCALL_DLOPEN`. Monta o sistema de arquivos a cada chamada (mesmo
/// padrão de custo já aceito por `kernel::fat32_demo`/`kernel_main`).
///
/// # Safety
/// `ptr`/`len` vêm diretamente do chamador em modo usuário, sem validação —
/// mesmo precedente de `sys_write`; UTF-8 inválido é `SYSCALL_ERROR`, não um
/// pânico.
unsafe fn sys_dlopen(ptr: u64, len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let Ok(path) = core::str::from_utf8(bytes) else {
        return SYSCALL_ERROR;
    };

    let fs = match crate::fat32::mount_boot_partition() {
        Ok(fs) => fs,
        Err(_) => return SYSCALL_ERROR,
    };

    let root = crate::paging::current_root();
    let mut frame_allocator = crate::phys::FRAME_ALLOCATOR.lock();
    let mut loading_stack = alloc::vec::Vec::new();
    match crate::dylib::load_library(&mut frame_allocator, root, &fs, path, &mut loading_stack) {
        Ok(handle) => handle as u64,
        Err(e) => {
            // Mesmo padrão de log de erro de outros subsistemas opcionais
            // (`kernel::storage::init`, `kernel::network::init`): reporta o
            // motivo exato no console serial para depuração, mas a
            // *syscall* em si sempre devolve só o sentinela `SYSCALL_ERROR`
            // — cobre "Falha de Carregamento ou Resolução Não Compromete a
            // Estabilidade do Kernel" da spec `kernel-dynamic-linker`.
            crate::serial_println!("dlopen: falha ao carregar {:?}: {:?}", path, e);
            SYSCALL_ERROR
        }
    }
}

/// Resolve o endereço do símbolo `name_ptr`/`name_len` na biblioteca
/// identificada por `handle` (devolvido por `SYSCALL_DLOPEN`) no espaço de
/// endereçamento do chamador — cobre `SYSCALL_DLSYM`.
///
/// # Safety
/// `name_ptr`/`name_len` vêm diretamente do chamador em modo usuário, sem
/// validação — mesmo precedente de `sys_write`.
unsafe fn sys_dlsym(handle: u64, name_ptr: u64, name_len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(name_ptr as *const u8, name_len as usize) };
    let Ok(name) = core::str::from_utf8(bytes) else {
        return SYSCALL_ERROR;
    };

    let root = crate::paging::current_root();
    crate::dylib::dlsym(root, handle, name).unwrap_or(SYSCALL_ERROR)
}

/// Abre, por caminho absoluto em `path_ptr`/`path_len`, um arquivo do
/// sistema de arquivos montado — cobre `SYSCALL_OPEN`. Devolve o `handle`
/// devolvido por `kernel::file_handles::register`, ou `SYSCALL_ERROR` para
/// UTF-8 inválido, falha de montagem, ou caminho que não corresponde a um
/// arquivo existente (inclui o caso de apontar para um diretório — mesmo
/// sentinela de erro de `Fat32Error::NotAFile`/`NotFound`/`NotADirectory`,
/// sem esquema granular de erro, mesma disciplina das demais *syscalls*).
///
/// # Safety
/// `path_ptr`/`path_len` vêm diretamente do chamador em modo usuário, sem
/// validação — mesmo precedente de `sys_write`/`sys_dlopen`.
unsafe fn sys_open(path_ptr: u64, path_len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len as usize) };
    let Ok(path) = core::str::from_utf8(bytes) else {
        return SYSCALL_ERROR;
    };

    let fs = match crate::fat32::mount_boot_partition() {
        Ok(fs) => fs,
        Err(_) => return SYSCALL_ERROR,
    };

    // Mesmo padrão de `kernel::scheduler::spawn_from_elf`/`kernel::dylib::load_library`:
    // o arquivo inteiro é lido de uma vez para memória (ver design.md,
    // Non-Goals: sem leitura incremental por cluster).
    match crate::storage::with_disk(|disk| fs.read_file(disk, path)) {
        Some(Ok(contents)) => crate::file_handles::register(contents),
        _ => SYSCALL_ERROR,
    }
}

/// Lê até `buf_len` bytes a partir do cursor atual do arquivo identificado
/// por `handle`, copiando-os para `buf_ptr` (buffer do chamador, modo
/// usuário) — cobre `SYSCALL_READ`. Devolve a quantidade de bytes
/// efetivamente copiada (`0` ao alcançar o fim do arquivo, o que não é um
/// erro), ou `SYSCALL_ERROR` se `handle` não corresponder a nenhum arquivo
/// atualmente aberto.
///
/// # Safety
/// `buf_ptr`/`buf_len` vêm diretamente do chamador em modo usuário, sem
/// validação — mesmo precedente de `sys_write`.
unsafe fn sys_read(handle: u64, buf_ptr: u64, buf_len: u64) -> u64 {
    let Some(bytes) = crate::file_handles::read(handle, buf_len as usize) else {
        return SYSCALL_ERROR;
    };
    let dest = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, bytes.len()) };
    dest.copy_from_slice(&bytes);
    bytes.len() as u64
}

/// Fecha o arquivo identificado por `handle`, liberando os recursos
/// associados a ele — cobre `SYSCALL_CLOSE`. `SYSCALL_ERROR` se `handle`
/// não corresponder a nenhum arquivo atualmente aberto.
fn sys_close(handle: u64) -> u64 {
    if crate::file_handles::close(handle) { 0 } else { SYSCALL_ERROR }
}

/// Termina a thread chamadora — cobre `SYSCALL_EXIT`. Marca a thread como
/// terminada (`kernel::scheduler::mark_terminated`, consultada por
/// `schedule_next` para nunca mais devolvê-la à `ReadyQueue`) e então
/// aguarda ociosamente (`sti`/`hlt`, mesmo padrão de `kernel::idle_loop`) a
/// próxima troca de contexto — que, por construção, nunca mais retoma esta
/// chamada: a thread chamadora nunca volta a executar depois disto.
fn sys_exit() -> ! {
    crate::scheduler::mark_terminated(crate::scheduler::current_thread_id());
    loop {
        unsafe { core::arch::asm!("sti", "hlt", options(preserves_flags)) };
    }
}

/// Despacha uma chamada de sistema pelo número (`RAX` na entrada de
/// `syscall`), com até quatro argumentos. Devolve o valor de retorno (`RAX`
/// na saída) — `SYSCALL_ERROR` para um número não reconhecido. `extern "C"`
/// e `#[no_mangle]`: chamada diretamente pela rotina de entrada em
/// assembly ([`syscall_entry`], abaixo), não só por código Rust.
///
/// # Safety
/// Para `SYSCALL_WRITE`, `arg0`/`arg1` são repassados como `ptr`/`len` para
/// [`sys_write`] sem validação — ver lá.
#[unsafe(no_mangle)]
unsafe extern "C" fn dispatch(number: u64, arg0: u64, arg1: u64, arg2: u64, _arg3: u64) -> u64 {
    match number {
        SYSCALL_WRITE => unsafe { sys_write(arg0, arg1) },
        SYSCALL_GET_TICKS => sys_get_ticks(),
        SYSCALL_VM_RESERVE => sys_vm_reserve(arg0),
        SYSCALL_VM_COMMIT => sys_vm_commit(arg0, arg1, arg2),
        SYSCALL_VM_DECOMMIT => sys_vm_decommit(arg0, arg1),
        SYSCALL_VM_PROTECT => sys_vm_protect(arg0, arg1, arg2),
        SYSCALL_VM_RELEASE => sys_vm_release(arg0),
        SYSCALL_MUTEX_CREATE => sys_mutex_create(),
        SYSCALL_MUTEX_LOCK => sys_mutex_lock(arg0),
        SYSCALL_MUTEX_UNLOCK => sys_mutex_unlock(arg0),
        SYSCALL_COND_CREATE => sys_cond_create(),
        SYSCALL_COND_WAIT => sys_cond_wait(arg0, arg1, arg2),
        SYSCALL_COND_NOTIFY_ONE => sys_cond_notify_one(arg0),
        SYSCALL_COND_NOTIFY_ALL => sys_cond_notify_all(arg0),
        SYSCALL_CLOCK_WALL => sys_clock_wall(),
        SYSCALL_CLOCK_MONOTONIC => sys_clock_monotonic(),
        SYSCALL_DLOPEN => unsafe { sys_dlopen(arg0, arg1) },
        SYSCALL_DLSYM => unsafe { sys_dlsym(arg0, arg1, arg2) },
        SYSCALL_OPEN => unsafe { sys_open(arg0, arg1) },
        SYSCALL_READ => unsafe { sys_read(arg0, arg1, arg2) },
        SYSCALL_CLOSE => sys_close(arg0),
        SYSCALL_EXIT => sys_exit(),
        SYSCALL_MUTEX_TRYLOCK => sys_mutex_trylock(arg0),
        _ => SYSCALL_ERROR,
    }
}

core::arch::global_asm!(
    ".global syscall_entry",
    "syscall_entry:",
    // RCX = RIP de retorno, R11 = RFLAGS do chamador (hardware). RSP ainda
    // é o do usuário (não confiável) — salva antes de qualquer outra coisa.
    //
    // Guardado no slot *por-thread* apontado por `CURRENT_SYSCALL_RSP_SLOT`
    // (`kernel::scheduler`) — nunca numa única `static` global: uma syscall
    // bloqueante (`WaitQueue::commit_block`, ex. `SYSCALL_COND_WAIT`) cede a
    // CPU para uma thread completamente diferente, que também pode fazer
    // suas próprias syscalls antes desta ser despertada de novo. Uma única
    // global seria sobrescrita nesse meio tempo, devolvendo a esta thread o
    // `RSP` de usuário de outra — achado real em boot durante a change
    // `kernel-timed-wait`, registrado em `KNOW_ISSUES.md`.
    //
    // R10 (A3 do chamador) é copiado para R8 primeiro, liberando R10 como
    // registrador temporário para guardar o endereço do slot. RAX (número da
    // syscall) ainda precisa sobreviver até `mov rdi, rax`, mais abaixo —
    // nunca usado como temporário aqui.
    "mov r8, r10", // a3 (5o parametro de dispatch) — adiantado para liberar R10
    "mov r10, [CURRENT_SYSCALL_RSP_SLOT]",
    "mov [r10], rsp",
    "mov rsp, [CURRENT_KERNEL_STACK_TOP]",
    // RSP acabou de ser realinhado a partir de um topo de stack (múltiplo
    // de 16); dois `push` de 8 bytes mantêm o alinhamento exigido pela ABI
    // antes do `call` abaixo — sem precisar de nenhum ajuste extra, ao
    // contrário de `scheduler::timer_entry` (que herda um RSP de
    // alinhamento imprevisível de uma interrupção assíncrona).
    "push rcx", // RIP do usuário
    "push r11", // RFLAGS do usuário
    // Reorganiza os argumentos da convenção desta syscall (número em RAX,
    // argumentos em RDI/RSI/RDX, R10 já copiado para R8 acima) para a
    // convenção System V de `dispatch` (RDI/RSI/RDX/RCX/R8) — do último
    // parâmetro para o primeiro, para nunca sobrescrever uma fonte antes de
    // lê-la.
    "mov rcx, rdx",
    "mov rdx, rsi",
    "mov rsi, rdi",
    "mov rdi, rax",
    "call dispatch",
    "pop r11",
    "pop rcx",
    // Mesmo slot por-thread de cima — `CURRENT_SYSCALL_RSP_SLOT` já aponta
    // para o slot desta própria thread neste ponto, mesmo que `dispatch`
    // tenha bloqueado e cedido a CPU para outras threads no meio do caminho
    // (`schedule_next` sempre a atualiza para a thread que está de fato
    // retomando). R10 está livre aqui (indefinido depois de `call dispatch`,
    // mesmo raciocínio de R8/R9/RDI/RSI/RDX/RAX — nenhum valor de usuário
    // depende de sobreviver neles através de uma syscall).
    "mov r10, [CURRENT_SYSCALL_RSP_SLOT]",
    "mov rsp, [r10]",
    "sysretq",
);

unsafe extern "C" {
    fn syscall_entry();
}

/// Configura `STAR`/`LSTAR`/`SFMASK` e habilita `EFER.SCE`, permitindo que
/// código em modo usuário invoque `syscall`, atendida por [`syscall_entry`].
///
/// # Safety
/// Deve ser chamada uma única vez, depois de `gdt::init()` (os seletores de
/// usuário/kernel referenciados por `STAR` dependem do GDT já estar
/// montado).
pub unsafe fn init() {
    unsafe {
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | EFER_SCE);

        wrmsr(IA32_STAR, star_value(KERNEL_CODE_SELECTOR, SYSRET_SELECTOR_BASE));
        wrmsr(IA32_LSTAR, syscall_entry as *const () as u64);
        wrmsr(IA32_FMASK, RFLAGS_IF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdt::{USER_CODE_SELECTOR, USER_DATA_SELECTOR};

    #[test]
    fn star_value_places_syscall_and_sysret_bases_in_the_right_bit_fields() {
        let value = star_value(KERNEL_CODE_SELECTOR, SYSRET_SELECTOR_BASE);
        assert_eq!((value >> 32) & 0xffff, KERNEL_CODE_SELECTOR as u64);
        assert_eq!((value >> 48) & 0xffff, SYSRET_SELECTOR_BASE as u64);
    }

    #[test]
    fn sysret_selector_base_lands_on_the_user_segments() {
        // sysret (64 bits): CS = (U + 16) | 3, SS = (U + 8) | 3 — ver design.md.
        let cs = (SYSRET_SELECTOR_BASE + 16) | 3;
        let ss = (SYSRET_SELECTOR_BASE + 8) | 3;
        assert_eq!(cs, USER_CODE_SELECTOR);
        assert_eq!(ss, USER_DATA_SELECTOR);
    }

    #[test]
    fn syscall_cs_base_lands_on_kernel_segments() {
        // syscall: CS = K, SS = K + 8 — deve bater com os seletores de kernel já existentes.
        assert_eq!(KERNEL_CODE_SELECTOR, crate::gdt::KERNEL_CODE_SELECTOR);
        assert_eq!(KERNEL_CODE_SELECTOR + 8, crate::gdt::KERNEL_DATA_SELECTOR);
    }

    #[test]
    fn unknown_syscall_number_returns_error_sentinel() {
        let result = unsafe { dispatch(0xdead_beef, 0, 0, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn get_ticks_dispatches_to_the_real_tick_counter() {
        let before = crate::timer::ticks();
        let result = unsafe { dispatch(SYSCALL_GET_TICKS, 0, 0, 0, 0) };
        assert_eq!(result, before, "GET_TICKS deve devolver o contador real do kernel no momento da chamada");
    }

    // `SYSCALL_WRITE` não é testado aqui: `sys_write` escreve na UART via
    // `outb` (instrução de I/O privilegiada) — executá-la em host (anel 3)
    // causaria um crash do próprio processo de teste, mesma limitação já
    // documentada para `Mapper::map`/`AddressSpace::new` (que tocam
    // `CR3`/HHDM reais). Verificado em boot real na tarefa 6.1.

    #[test]
    fn prot_to_page_flags_always_forces_user_accessible() {
        // Nenhuma combinação de bits de entrada consegue produzir
        // `user_accessible: false` — ver design.md, "Permissões de página".
        for prot in 0..=0b11u64 {
            assert!(prot_to_page_flags(prot).user_accessible);
        }
        assert_eq!(prot_to_page_flags(0), PageFlags { writable: false, user_accessible: true, executable: false });
        assert_eq!(prot_to_page_flags(0b01), PageFlags { writable: true, user_accessible: true, executable: false });
        assert_eq!(prot_to_page_flags(0b10), PageFlags { writable: false, user_accessible: true, executable: true });
        assert_eq!(prot_to_page_flags(0b11), PageFlags { writable: true, user_accessible: true, executable: true });
    }

    /// Serializa os testes que tocam `kernel::vmm::VMM` — compartilhado com
    /// `kernel::safepoint::tests`, já que módulos diferentes rodam em
    /// paralelo por padrão (um lock local aqui não coordenaria com um lock
    /// local lá) — ver `kernel::vmm::test_support`.
    use crate::vmm::test_support::{VMM_TEST_LOCK as VM_TEST_LOCK, ensure_vmm_initialized};

    #[test]
    fn vm_reserve_returns_an_address_inside_the_managed_window() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ensure_vmm_initialized();
        let addr = unsafe { dispatch(SYSCALL_VM_RESERVE, crate::paging::PAGE_SIZE, 0, 0, 0) };
        assert_ne!(addr, SYSCALL_ERROR);
        assert!(addr >= crate::vmm::VMM_WINDOW_START);
        assert!(addr < crate::vmm::VMM_WINDOW_START + crate::vmm::VMM_WINDOW_SIZE);
    }

    #[test]
    fn vm_reserve_with_zero_size_returns_error_sentinel() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(unsafe { dispatch(SYSCALL_VM_RESERVE, 0, 0, 0, 0) }, SYSCALL_ERROR);
    }

    #[test]
    fn vm_commit_outside_any_reservation_returns_error_sentinel_without_touching_the_mapper() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Endereço bem fora da janela gerenciada: nunca corresponde a nenhuma
        // reserva viva, então `region_containing` rejeita antes de chegar a
        // `Vmm::commit`/`Mapper` (que não seriam seguros de tocar em host).
        let result = unsafe { dispatch(SYSCALL_VM_COMMIT, 0, crate::paging::PAGE_SIZE, 0b01, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn vm_decommit_outside_any_reservation_returns_error_sentinel_without_touching_the_mapper() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result = unsafe { dispatch(SYSCALL_VM_DECOMMIT, 0, crate::paging::PAGE_SIZE, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn vm_protect_outside_any_reservation_returns_error_sentinel_without_touching_the_mapper() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result = unsafe { dispatch(SYSCALL_VM_PROTECT, 0, crate::paging::PAGE_SIZE, 0b01, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn vm_release_outside_any_reservation_returns_error_sentinel() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result = unsafe { dispatch(SYSCALL_VM_RELEASE, 0, 0, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn vm_release_right_after_reserve_succeeds_and_frees_the_address_for_reuse() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ensure_vmm_initialized();
        let size = crate::paging::PAGE_SIZE;
        let addr = unsafe { dispatch(SYSCALL_VM_RESERVE, size, 0, 0, 0) };
        assert_ne!(addr, SYSCALL_ERROR);

        // Nada foi comprometido ainda: `Vmm::release` nunca chega a chamar
        // `Mapper::unmap` (laço sobre offsets comprometidos fica vazio),
        // então este caminho de sucesso é seguro em host — ver design.md,
        // "Escopo de testes em host".
        let release_result = unsafe { dispatch(SYSCALL_VM_RELEASE, addr, 0, 0, 0) };
        assert_eq!(release_result, 0);

        let reused = unsafe { dispatch(SYSCALL_VM_RESERVE, size, 0, 0, 0) };
        assert_eq!(reused, addr, "reserva do mesmo tamanho logo apos liberar deveria reaproveitar o endereco liberado");
    }

    #[test]
    fn mutex_create_then_lock_on_a_free_mutex_succeeds_without_touching_the_scheduler() {
        // `try_acquire` é uma CAS atômica pura, nunca chega a `WaitQueue` —
        // seguro em host mesmo sem o guard de `VM_TEST_LOCK` (não toca
        // `VMM`/`FRAME_ALLOCATOR`).
        let handle = unsafe { dispatch(SYSCALL_MUTEX_CREATE, 0, 0, 0, 0) };
        assert_ne!(handle, SYSCALL_ERROR);
        assert_eq!(unsafe { dispatch(SYSCALL_MUTEX_LOCK, handle, 0, 0, 0) }, 0);
    }

    #[test]
    fn mutex_lock_with_an_unknown_handle_returns_error_sentinel() {
        assert_eq!(unsafe { dispatch(SYSCALL_MUTEX_LOCK, u64::MAX / 2, 0, 0, 0) }, SYSCALL_ERROR);
    }

    #[test]
    fn mutex_unlock_with_an_unknown_handle_returns_error_sentinel() {
        // Só o caminho de rejeição é testável em host: o de sucesso sempre
        // chama `scheduler::with_interrupts_disabled` — ver design.md,
        // "Escopo de testes em host".
        assert_eq!(unsafe { dispatch(SYSCALL_MUTEX_UNLOCK, u64::MAX / 2, 0, 0, 0) }, SYSCALL_ERROR);
    }

    #[test]
    fn mutex_trylock_on_a_free_mutex_succeeds_without_touching_the_scheduler() {
        // Mesma observação de `mutex_create_then_lock_on_a_free_mutex_...`:
        // `try_acquire` é uma CAS atômica pura, seguro em host.
        let handle = unsafe { dispatch(SYSCALL_MUTEX_CREATE, 0, 0, 0, 0) };
        assert_ne!(handle, SYSCALL_ERROR);
        assert_eq!(unsafe { dispatch(SYSCALL_MUTEX_TRYLOCK, handle, 0, 0, 0) }, 0);
    }

    #[test]
    fn mutex_trylock_on_an_already_taken_mutex_fails_without_blocking() {
        let handle = unsafe { dispatch(SYSCALL_MUTEX_CREATE, 0, 0, 0, 0) };
        assert_eq!(unsafe { dispatch(SYSCALL_MUTEX_TRYLOCK, handle, 0, 0, 0) }, 0);
        // Segunda tentativa sobre o mesmo mutex, já tomado pela "thread"
        // deste teste — deve falhar imediatamente, nunca bloquear (bloquear
        // travaria o processo de teste em host, sem nenhum escalonador para
        // acordá-lo).
        assert_eq!(unsafe { dispatch(SYSCALL_MUTEX_TRYLOCK, handle, 0, 0, 0) }, SYSCALL_ERROR);
    }

    #[test]
    fn mutex_trylock_with_an_unknown_handle_returns_error_sentinel() {
        assert_eq!(unsafe { dispatch(SYSCALL_MUTEX_TRYLOCK, u64::MAX / 2, 0, 0, 0) }, SYSCALL_ERROR);
    }

    #[test]
    fn cond_create_returns_a_usable_handle() {
        let handle = unsafe { dispatch(SYSCALL_COND_CREATE, 0, 0, 0, 0) };
        assert_ne!(handle, SYSCALL_ERROR);
    }

    #[test]
    fn cond_wait_with_an_unknown_cond_handle_returns_error_sentinel_without_touching_the_mutex() {
        let mutex_handle = unsafe { dispatch(SYSCALL_MUTEX_CREATE, 0, 0, 0, 0) };
        let result = unsafe { dispatch(SYSCALL_COND_WAIT, u64::MAX / 2, mutex_handle, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn cond_wait_with_an_unknown_mutex_handle_returns_error_sentinel() {
        let cond_handle = unsafe { dispatch(SYSCALL_COND_CREATE, 0, 0, 0, 0) };
        let result = unsafe { dispatch(SYSCALL_COND_WAIT, cond_handle, u64::MAX / 2, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn cond_wait_with_an_unknown_cond_handle_returns_error_sentinel_regardless_of_timeout() {
        // Mesmo teste de `..._without_touching_the_mutex` acima, mas com um
        // prazo > 0 — o handle desconhecido deve ser rejeitado antes de
        // qualquer lógica de prazo entrar em jogo (change `kernel-timed-wait`).
        let mutex_handle = unsafe { dispatch(SYSCALL_MUTEX_CREATE, 0, 0, 0, 0) };
        let result = unsafe { dispatch(SYSCALL_COND_WAIT, u64::MAX / 2, mutex_handle, 1_000_000, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn cond_wait_with_an_unknown_mutex_handle_returns_error_sentinel_regardless_of_timeout() {
        let cond_handle = unsafe { dispatch(SYSCALL_COND_CREATE, 0, 0, 0, 0) };
        let result = unsafe { dispatch(SYSCALL_COND_WAIT, cond_handle, u64::MAX / 2, 1_000_000, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn cond_notify_one_and_all_with_an_unknown_handle_return_error_sentinel() {
        // Mesma limitação do caminho de sucesso de `SYSCALL_MUTEX_UNLOCK`:
        // `notify_one`/`notify_all` sempre chamam
        // `scheduler::with_interrupts_disabled`, mesmo sem ninguém esperando.
        assert_eq!(unsafe { dispatch(SYSCALL_COND_NOTIFY_ONE, u64::MAX / 2, 0, 0, 0) }, SYSCALL_ERROR);
        assert_eq!(unsafe { dispatch(SYSCALL_COND_NOTIFY_ALL, u64::MAX / 2, 0, 0, 0) }, SYSCALL_ERROR);
    }

    #[test]
    fn vm_release_at_a_non_base_address_is_rejected() {
        let _guard = VM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ensure_vmm_initialized();
        let size = 2 * crate::paging::PAGE_SIZE;
        let addr = unsafe { dispatch(SYSCALL_VM_RESERVE, size, 0, 0, 0) };
        assert_ne!(addr, SYSCALL_ERROR);

        let result = unsafe { dispatch(SYSCALL_VM_RELEASE, addr + crate::paging::PAGE_SIZE, 0, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);

        // A reserva original continua viva: liberar pelo endereço base certo
        // ainda funciona depois da tentativa rejeitada.
        assert_eq!(unsafe { dispatch(SYSCALL_VM_RELEASE, addr, 0, 0, 0) }, 0);
    }

    // `SYSCALL_OPEN` não é testado aqui: `sys_open` monta o sistema de
    // arquivos via `kernel::storage::with_disk` (disco AHCI real), mesma
    // limitação já documentada para `SYSCALL_WRITE`/`SYSCALL_DLOPEN` —
    // verificado em boot real na tarefa 3.3. `sys_read`/`sys_close`, por
    // outro lado, só tocam `kernel::file_handles` (lógica pura, sem
    // hardware) — seus caminhos de rejeição por handle desconhecido são
    // testáveis em host independentemente de `SYSCALL_OPEN` já ter sido
    // chamado.

    #[test]
    fn read_with_an_unknown_handle_returns_error_sentinel() {
        let result = unsafe { dispatch(SYSCALL_READ, u64::MAX / 2, 0, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }

    #[test]
    fn close_with_an_unknown_handle_returns_error_sentinel() {
        let result = unsafe { dispatch(SYSCALL_CLOSE, u64::MAX / 2, 0, 0, 0) };
        assert_eq!(result, SYSCALL_ERROR);
    }
}
