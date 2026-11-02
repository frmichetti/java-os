//! GDT e TSS próprios do kernel, substituindo os que o Limine deixa
//! montados no boot — ver `design.md` da change `kernel-interrupts-timer`.

/// Tamanho da stack dedicada ao handler de double fault (IST1). Memória
/// estática no `.bss` do kernel: o handler de double fault precisa
/// funcionar mesmo que o subsistema de heap/paginação seja a própria causa
/// da falha (ver design.md — "Stack do double fault em memória estática").
const DOUBLE_FAULT_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
struct DoubleFaultStack([u8; DOUBLE_FAULT_STACK_SIZE]);

static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; DOUBLE_FAULT_STACK_SIZE]);

/// Índice (1-based) da entrada de IST usada pelo handler de double fault.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;

/// Tamanho da stack dedicada ao handler de page fault (IST2). Mesmo
/// racional/tamanho de `DOUBLE_FAULT_STACK`: o handler de `#PF` precisa ser
/// entregável mesmo quando a stack corrente já estourou para dentro da
/// guard page do kernel (ver `design.md` da change `kernel-stack-guard-page`
/// — "Page fault também precisa de stack dedicada (IST)").
const PAGE_FAULT_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
struct PageFaultStack([u8; PAGE_FAULT_STACK_SIZE]);

static mut PAGE_FAULT_STACK: PageFaultStack = PageFaultStack([0; PAGE_FAULT_STACK_SIZE]);

/// Índice (1-based) da entrada de IST usada pelo handler de page fault.
pub const PAGE_FAULT_IST_INDEX: u16 = 2;

const ACCESS_PRESENT: u8 = 1 << 7;
const ACCESS_NOT_SYSTEM: u8 = 1 << 4; // S=1: descritor de código/dados (não de sistema)
const ACCESS_EXECUTABLE: u8 = 1 << 3;
const ACCESS_READ_WRITE: u8 = 1 << 1;
const ACCESS_TSS_AVAILABLE: u8 = 0b1001; // Type=1001: TSS de 64 bits disponível
/// DPL=3 (bits 5-6 do byte de access) — privilégio de anel 3 (modo usuário).
const ACCESS_DPL_USER: u8 = 0b011 << 5;

const FLAG_LONG_MODE: u8 = 1 << 1; // bit 53 do descritor (L)

/// Uma entrada de GDT de código/dados (8 bytes) — base e limite são
/// ignorados pelo hardware em modo 64 bits para esses tipos de segmento;
/// apenas os bits de `access`/`flags` importam.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct GdtEntry(u64);

impl GdtEntry {
    pub const fn null() -> Self {
        Self(0)
    }

    const fn new(access: u8, flags_nibble: u8) -> Self {
        let mut value: u64 = 0;
        value |= (access as u64) << 40;
        value |= ((flags_nibble & 0x0f) as u64) << 52;
        Self(value)
    }

    pub const fn kernel_code64() -> Self {
        Self::new(
            ACCESS_PRESENT | ACCESS_NOT_SYSTEM | ACCESS_EXECUTABLE | ACCESS_READ_WRITE,
            FLAG_LONG_MODE,
        )
    }

    pub const fn kernel_data() -> Self {
        Self::new(ACCESS_PRESENT | ACCESS_NOT_SYSTEM | ACCESS_READ_WRITE, 0)
    }

    /// Segmento de dados de modo usuário (DPL=3) — ver `design.md` da
    /// change `kernel-syscalls`, "Layout do GDT para `syscall`/`sysret`":
    /// precisa vir imediatamente antes de `user_code64()` no GDT (dados
    /// antes de código), para bater com a aritmética do MSR `STAR`.
    pub const fn user_data() -> Self {
        Self::new(ACCESS_PRESENT | ACCESS_DPL_USER | ACCESS_NOT_SYSTEM | ACCESS_READ_WRITE, 0)
    }

    /// Segmento de código de 64 bits de modo usuário (DPL=3).
    pub const fn user_code64() -> Self {
        Self::new(
            ACCESS_PRESENT | ACCESS_DPL_USER | ACCESS_NOT_SYSTEM | ACCESS_EXECUTABLE | ACCESS_READ_WRITE,
            FLAG_LONG_MODE,
        )
    }

    pub fn raw(&self) -> u64 {
        self.0
    }
}

/// Descritor de TSS de 64 bits: ocupa duas entradas de GDT (16 bytes), pois
/// o campo de base precisa dos 64 bits completos do endereço do TSS.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct TssDescriptor([u64; 2]);

impl TssDescriptor {
    pub fn new(tss_addr: u64, limit: u16) -> Self {
        let base_low = tss_addr & 0xff_ffff;
        let base_mid = (tss_addr >> 24) & 0xff;
        let base_high = (tss_addr >> 32) & 0xffff_ffff;
        let access = (ACCESS_PRESENT | ACCESS_TSS_AVAILABLE) as u64;

        let mut low: u64 = 0;
        low |= limit as u64;
        low |= base_low << 16;
        low |= access << 40;
        low |= base_mid << 56;

        let high: u64 = base_high;

        Self([low, high])
    }

    pub const fn empty() -> Self {
        Self([0, 0])
    }

    pub fn raw(&self) -> [u64; 2] {
        self.0
    }
}

/// Task State Segment (formato de 64 bits — apenas os campos `rsp0` e
/// `ist1..7` são usados por este kernel; os demais ficam zerados).
#[repr(C, packed)]
pub struct TaskStateSegment {
    reserved0: u32,
    pub rsp: [u64; 3],
    reserved1: u64,
    pub ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp: [0; 3],
            reserved1: 0,
            ist: [0; 7],
            reserved2: 0,
            reserved3: 0,
            iomap_base: core::mem::size_of::<TaskStateSegment>() as u16,
        }
    }
}

static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// GDT do kernel: nulo, código de kernel (0x08), dados de kernel (0x10),
/// dados de usuário (0x18), código de usuário (0x20), TSS (0x28, ocupando
/// também o slot em 0x30). Dados de usuário vêm **antes** de código de
/// usuário deliberadamente — ver `design.md` da change `kernel-syscalls`,
/// "Layout do GDT para `syscall`/`sysret`".
#[repr(C)]
pub struct Gdt {
    pub null: GdtEntry,
    pub kernel_code: GdtEntry,
    pub kernel_data: GdtEntry,
    pub user_data: GdtEntry,
    pub user_code: GdtEntry,
    pub tss: TssDescriptor,
}

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
/// Já inclui o RPL 3 (modo usuário) — pronto para uso direto em CS/SS.
pub const USER_DATA_SELECTOR: u16 = 0x18 | 3;
/// Já inclui o RPL 3 (modo usuário) — pronto para uso direto em CS/SS.
pub const USER_CODE_SELECTOR: u16 = 0x20 | 3;
pub const TSS_SELECTOR: u16 = 0x28;

/// Formato aceito por `lgdt`/`lidt`: tamanho da tabela em bytes menos 1, e
/// seu endereço linear.
#[repr(C, packed)]
pub struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

static mut GDT: Gdt = Gdt {
    null: GdtEntry::null(),
    kernel_code: GdtEntry::kernel_code64(),
    kernel_data: GdtEntry::kernel_data(),
    user_data: GdtEntry::user_data(),
    user_code: GdtEntry::user_code64(),
    tss: TssDescriptor::empty(),
};

/// Atualiza `TSS.rsp0` — a stack que a CPU usa automaticamente sempre que
/// uma interrupção ou exceção chega com troca de privilégio (anel 3 →
/// anel 0). Deve ser chamada a cada troca de contexto para o escalonador
/// manter esse valor sempre coerente com a stack de kernel da thread
/// corrente — ver `design.md` da change `kernel-syscalls`, "`TSS.rsp0`
/// atualizado a cada troca de contexto".
///
/// # Safety
/// Deve ser chamada depois de `init()` (o TSS já montado e carregado via
/// `ltr`).
pub unsafe fn set_kernel_stack(top: u64) {
    unsafe {
        (*core::ptr::addr_of_mut!(TSS)).rsp[0] = top;
    }
}

/// Monta o TSS (com a stack de IST1 para double fault), monta o GDT
/// (completando o descritor de TSS com o endereço real do `static` acima),
/// carrega ambos via `lgdt`/`ltr`, e recarrega os seletores de segmento.
///
/// # Safety
/// Deve ser chamada uma única vez, antes de qualquer uso de interrupções ou
/// de troca de privilégio.
pub unsafe fn init() {
    unsafe {
        let double_fault_stack_top = core::ptr::addr_of!(DOUBLE_FAULT_STACK) as u64 + DOUBLE_FAULT_STACK_SIZE as u64;
        (*core::ptr::addr_of_mut!(TSS)).ist[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = double_fault_stack_top;

        let page_fault_stack_top = core::ptr::addr_of!(PAGE_FAULT_STACK) as u64 + PAGE_FAULT_STACK_SIZE as u64;
        (*core::ptr::addr_of_mut!(TSS)).ist[(PAGE_FAULT_IST_INDEX - 1) as usize] = page_fault_stack_top;

        let tss_addr = core::ptr::addr_of!(TSS) as u64;
        let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u16;
        (*core::ptr::addr_of_mut!(GDT)).tss = TssDescriptor::new(tss_addr, tss_limit);

        let gdt_ptr = DescriptorTablePointer {
            limit: (core::mem::size_of::<Gdt>() - 1) as u16,
            base: core::ptr::addr_of!(GDT) as u64,
        };
        core::arch::asm!("lgdt [{0}]", in(reg) &gdt_ptr, options(readonly, nostack, preserves_flags));

        reload_segments();

        core::arch::asm!("ltr {0:x}", in(reg) TSS_SELECTOR, options(nostack, preserves_flags));
    }
}

/// Recarrega `CS` (via `retfq`, único jeito de trocar `CS` em modo 64 bits
/// sem uma troca de privilégio real) e os demais seletores de segmento para
/// os do novo GDT.
unsafe fn reload_segments() {
    unsafe {
        core::arch::asm!(
            "push {sel}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            sel = in(reg) u64::from(KERNEL_CODE_SELECTOR),
            tmp = lateout(reg) _,
            options(preserves_flags),
        );
        core::arch::asm!(
            "mov ds, {0:x}",
            "mov es, {0:x}",
            "mov ss, {0:x}",
            "mov fs, {0:x}",
            "mov gs, {0:x}",
            in(reg) KERNEL_DATA_SELECTOR,
            options(nostack, preserves_flags),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_code64_sets_present_executable_long_mode() {
        let entry = GdtEntry::kernel_code64().raw();
        let access = (entry >> 40) & 0xff;
        let flags = (entry >> 52) & 0x0f;
        assert_eq!(access & ACCESS_PRESENT as u64, ACCESS_PRESENT as u64);
        assert_eq!(access & ACCESS_EXECUTABLE as u64, ACCESS_EXECUTABLE as u64);
        assert_eq!(flags & FLAG_LONG_MODE as u64, FLAG_LONG_MODE as u64);
    }

    #[test]
    fn kernel_data_is_not_executable() {
        let entry = GdtEntry::kernel_data().raw();
        let access = (entry >> 40) & 0xff;
        assert_eq!(access & ACCESS_EXECUTABLE as u64, 0);
        assert_eq!(access & ACCESS_PRESENT as u64, ACCESS_PRESENT as u64);
    }

    fn dpl_of(entry: GdtEntry) -> u64 {
        ((entry.raw() >> 40) & 0xff) & (ACCESS_DPL_USER as u64)
    }

    #[test]
    fn user_code64_has_dpl_3_present_executable_long_mode() {
        let entry = GdtEntry::user_code64();
        let access = (entry.raw() >> 40) & 0xff;
        let flags = (entry.raw() >> 52) & 0x0f;
        assert_eq!(dpl_of(entry), ACCESS_DPL_USER as u64, "user_code64 deve ter DPL=3");
        assert_eq!(access & ACCESS_PRESENT as u64, ACCESS_PRESENT as u64);
        assert_eq!(access & ACCESS_EXECUTABLE as u64, ACCESS_EXECUTABLE as u64);
        assert_eq!(flags & FLAG_LONG_MODE as u64, FLAG_LONG_MODE as u64);
    }

    #[test]
    fn user_data_has_dpl_3_and_is_not_executable() {
        let entry = GdtEntry::user_data();
        let access = (entry.raw() >> 40) & 0xff;
        assert_eq!(dpl_of(entry), ACCESS_DPL_USER as u64, "user_data deve ter DPL=3");
        assert_eq!(access & ACCESS_EXECUTABLE as u64, 0);
        assert_eq!(access & ACCESS_PRESENT as u64, ACCESS_PRESENT as u64);
    }

    #[test]
    fn kernel_segments_have_dpl_0() {
        assert_eq!(dpl_of(GdtEntry::kernel_code64()), 0);
        assert_eq!(dpl_of(GdtEntry::kernel_data()), 0);
    }

    #[test]
    fn tss_descriptor_encodes_base_address_across_both_halves() {
        let tss_addr: u64 = 0x1234_5678_9abc;
        let desc = TssDescriptor::new(tss_addr, 103).raw();
        let low = desc[0];
        let high = desc[1];

        let base_low = (low >> 16) & 0xff_ffff;
        let base_mid = (low >> 56) & 0xff;
        let base_high = high & 0xffff_ffff;

        let reconstructed = base_low | (base_mid << 24) | (base_high << 32);
        assert_eq!(reconstructed, tss_addr);

        let limit = low & 0xffff;
        assert_eq!(limit, 103);
    }
}
