use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const BOOT_TIMEOUT: Duration = Duration::from_secs(15);

const IMAGE_SIZE_MIB: u64 = 64;
const PART_START_SECTOR: u64 = 2048;
const SECTOR_SIZE: u64 = 512;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask deve estar dentro do workspace")
        .to_path_buf()
}

fn run(cmd: &mut Command) {
    eprintln!("+ {:?}", cmd);
    let status = cmd.status().unwrap_or_else(|e| panic!("falha ao executar {:?}: {e}", cmd));
    if !status.success() {
        panic!("comando falhou com status {:?}: {:?}", status, cmd);
    }
}

fn build_kernel(root: &Path) {
    run(Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args([
            "build",
            "-p",
            "kernel",
            "--target",
            "x86_64-java_os.json",
            "-Z",
            "build-std=core,alloc",
            "-Z",
            "build-std-features=compiler-builtins-mem",
        ]));
}

fn kernel_binary(root: &Path) -> PathBuf {
    root.join("target/x86_64-java_os/debug/kernel")
}

/// Compila o executável ELF64 de teste do carregador de executáveis
/// (`kernel::elf`/`kernel::scheduler::spawn_from_elf`) — ver spec
/// `kernel-elf-loader` e `design.md` da change `kernel-elf-loader`. Em
/// modo release (não debug, como o kernel): este binário não precisa de
/// símbolos de depuração, e o release é ordens de magnitude menor (poucos
/// KiB contra quase 1 MiB em debug), mais rápido de copiar para a imagem e
/// de ler pelo carregador em boot real.
fn build_test_program(root: &Path) {
    run(Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args([
            "build",
            "-p",
            "elf_test_program",
            "--target",
            "x86_64-java_os_user.json",
            "--release",
            "-Z",
            "build-std=core",
            "-Z",
            "build-std-features=compiler-builtins-mem",
        ]));
}

fn test_program_binary(root: &Path) -> PathBuf {
    root.join("target/x86_64-java_os_user/release/elf_test_program")
}

/// Compila as duas bibliotecas compartilhadas de teste do linker dinâmico
/// (`kernel::dylib`) — ver spec `kernel-dynamic-linker` e `design.md` da
/// change `kernel-dynamic-linker`. `elf_test_library_base` primeiro (sem
/// dependências); `elf_test_library_user` depois, linkada contra o `.so` da
/// primeira via `-L`/`-l` (produz o `DT_NEEDED` e as relocações
/// `R_X86_64_JUMP_SLOT` resolvidas entre bibliotecas, exercitados em boot
/// real). Ambas com `--hash-style=sysv`: o carregador do kernel só
/// interpreta `DT_HASH` clássico (ver design.md, "Contagem de símbolos via
/// `DT_HASH`") — passado via `RUSTFLAGS` (não via `.cargo/config.toml`,
/// que esse target nem declara) para não precisar de flags fixas que
/// atrapalhariam variar `-L`/`-l` entre as duas bibliotecas.
fn build_test_libraries(root: &Path) {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let lib_dir = root.join("target/x86_64-java_os_user_pic/release");

    run(Command::new(&cargo)
        .current_dir(root)
        .env("RUSTFLAGS", "-C link-arg=--hash-style=sysv")
        .args([
            "build",
            "-p",
            "elf_test_library_base",
            "--target",
            "x86_64-java_os_user_pic.json",
            "--release",
            "-Z",
            "build-std=core",
            "-Z",
            "build-std-features=compiler-builtins-mem",
        ]));

    let rustflags = format!("-C link-arg=--hash-style=sysv -L {} -l base", lib_dir.display());
    run(Command::new(&cargo)
        .current_dir(root)
        .env("RUSTFLAGS", rustflags)
        .args([
            "build",
            "-p",
            "elf_test_library_user",
            "--target",
            "x86_64-java_os_user_pic.json",
            "--release",
            "-Z",
            "build-std=core",
            "-Z",
            "build-std-features=compiler-builtins-mem",
        ]));
}

fn test_library_binary(root: &Path, name: &str) -> PathBuf {
    root.join(format!("target/x86_64-java_os_user_pic/release/lib{name}.so"))
}

/// Fixture de 16 bytes (`"ABCDEFGHIJKLMNOP"`, sem quebra de linha) usada por
/// `kernel::file_syscalls_demo` para verificar `SYSCALL_OPEN`/`SYSCALL_READ`/
/// `SYSCALL_CLOSE` em boot real — ver `design.md`/`tasks.md` da change
/// `kernel-file-syscalls`, tarefa 3.1. Copiada para `::/TEST.TXT` na raiz da
/// partição (nome curto 8.3, sem precisar de nenhum diretório novo).
fn file_syscalls_fixture_path(root: &Path) -> PathBuf {
    root.join("kernel/testdata/file_syscalls_fixture.txt")
}

fn find_ovmf() -> PathBuf {
    if let Ok(p) = env::var("OVMF_PATH") {
        return PathBuf::from(p);
    }
    for candidate in [
        "/usr/share/ovmf/OVMF.fd",
        "/usr/share/OVMF/OVMF.fd",
        "/usr/share/qemu/OVMF.fd",
    ] {
        if Path::new(candidate).exists() {
            return PathBuf::from(candidate);
        }
    }
    panic!("Firmware OVMF não encontrado. Defina a variável de ambiente OVMF_PATH.");
}

fn build_image(root: &Path) -> PathBuf {
    build_kernel(root);
    build_test_program(root);
    build_test_libraries(root);

    let dist = root.join("boot/dist");
    fs::create_dir_all(&dist).expect("falha ao criar boot/dist");
    let image_path = dist.join("java-os.img");

    {
        let f = fs::File::create(&image_path).expect("falha ao criar imagem");
        f.set_len(IMAGE_SIZE_MIB * 1024 * 1024).expect("falha ao dimensionar imagem");
    }

    run(Command::new("sgdisk").args([
        "--clear",
        "--new=1:2048:0",
        "--typecode=1:ef00",
        "--change-name=1:EFI System Partition",
        image_path.to_str().unwrap(),
    ]));

    let offset_bytes = PART_START_SECTOR * SECTOR_SIZE;
    let part = format!("{}@@{}", image_path.display(), offset_bytes);

    run(Command::new("mformat").args(["-i", &part, "-F", "::"]));

    for dir in ["::/EFI", "::/EFI/BOOT", "::/boot", "::/bin", "::/lib"] {
        run(Command::new("mmd").args(["-i", &part, dir]));
    }

    let bootx64 = root.join("boot/limine/BOOTX64.EFI");
    let limine_conf = root.join("boot/limine.conf");
    let kernel_bin = kernel_binary(root);
    let test_program_bin = test_program_binary(root);
    let test_lib_base = test_library_binary(root, "base");
    let test_lib_user = test_library_binary(root, "user");
    let file_syscalls_fixture = file_syscalls_fixture_path(root);

    run(Command::new("mcopy").args([
        "-i",
        &part,
        bootx64.to_str().unwrap(),
        "::/EFI/BOOT/BOOTX64.EFI",
    ]));
    run(Command::new("mcopy").args([
        "-i",
        &part,
        kernel_bin.to_str().unwrap(),
        "::/boot/kernel",
    ]));
    run(Command::new("mcopy").args([
        "-i",
        &part,
        limine_conf.to_str().unwrap(),
        "::/boot/limine.conf",
    ]));
    run(Command::new("mcopy").args([
        "-i",
        &part,
        test_program_bin.to_str().unwrap(),
        "::/bin/test.elf",
    ]));
    run(Command::new("mcopy").args([
        "-i",
        &part,
        test_lib_base.to_str().unwrap(),
        "::/lib/libbase.so",
    ]));
    run(Command::new("mcopy").args([
        "-i",
        &part,
        test_lib_user.to_str().unwrap(),
        "::/lib/libuser.so",
    ]));
    run(Command::new("mcopy").args([
        "-i",
        &part,
        file_syscalls_fixture.to_str().unwrap(),
        "::/TEST.TXT",
    ]));

    eprintln!("Imagem gerada em {}", image_path.display());
    image_path
}

fn run_qemu(root: &Path, extra_args: &[String], headless: bool) -> std::process::ExitStatus {
    let image_path = build_image(root);
    let ovmf = find_ovmf();

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-M").arg("q35")
        .arg("-cpu").arg("max") // exigido a partir de kernel-interrupts-timer: precisa expor x2APIC
        .arg("-m").arg("256M")
        .arg("-bios").arg(&ovmf)
        .arg("-drive").arg(format!("file={},format=raw,if=ide", image_path.display()))
        // Fixa explicitamente o modelo da placa de rede (Intel 82540EM, bem
        // documentado) em vez de depender do padrão do QEMU — ver design.md
        // da change `kernel-network`.
        .arg("-netdev").arg("user,id=n0")
        .arg("-device").arg("e1000,netdev=n0")
        .arg("-no-reboot")
        .arg("-no-shutdown");

    if headless {
        cmd.arg("-display").arg("none");
    }

    cmd.args(extra_args);

    eprintln!("+ {:?}", cmd);
    cmd.status().expect("falha ao executar qemu-system-x86_64")
}

fn split_extra_args(args: &[String]) -> &[String] {
    match args.iter().position(|a| a == "--") {
        Some(idx) => &args[idx + 1..],
        None => args,
    }
}

/// Smoke test de boot (headless): builda a imagem, roda o kernel no QEMU com
/// timeout limitado capturando a saída serial em arquivo, mata o QEMU ao
/// final, e verifica se os marcadores esperados do banner de boot e do mapa
/// de memória (tarefas 3.2/3.3) estão presentes.
///
/// Cobre os cenários "Smoke test de boot passa em um boot bem-sucedido" e
/// "Smoke test de boot falha em um boot quebrado" de specs/kernel-boot/spec.md.
fn run_test(root: &Path) {
    let image_path = build_image(root);
    let ovmf = find_ovmf();

    let serial_log = root.join("target/boot_smoke_serial.log");
    let _ = fs::remove_file(&serial_log);

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-M")
        .arg("q35")
        .arg("-cpu")
        .arg("max")
        .arg("-m")
        .arg("256M")
        .arg("-bios")
        .arg(&ovmf)
        .arg("-drive")
        .arg(format!("file={},format=raw,if=ide", image_path.display()))
        // Mesma flag de rede fixa de `run_qemu` — ver design.md da change
        // `kernel-network`.
        .arg("-netdev")
        .arg("user,id=n0")
        .arg("-device")
        .arg("e1000,netdev=n0")
        .arg("-no-reboot")
        .arg("-no-shutdown")
        .arg("-display")
        .arg("none")
        .arg("-serial")
        .arg(format!("file:{}", serial_log.display()))
        .arg("-monitor")
        .arg("none");

    eprintln!("+ {:?}", cmd);
    let mut child = cmd.spawn().expect("falha ao iniciar qemu-system-x86_64");

    let start = Instant::now();
    let exited_on_its_own = loop {
        if child.try_wait().expect("falha ao verificar status do qemu").is_some() {
            break true;
        }
        if start.elapsed() > BOOT_TIMEOUT {
            break false;
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    if !exited_on_its_own {
        eprintln!(
            "timeout de {}s atingido sem o QEMU encerrar sozinho; matando o processo",
            BOOT_TIMEOUT.as_secs()
        );
        let _ = child.kill();
    }
    let _ = child.wait();

    let output = fs::read_to_string(&serial_log).unwrap_or_default();
    let has_banner = output.contains("boot OK");
    let has_memmap =
        output.contains("memoria usavel total:") || output.contains("mapa de memoria: AUSENTE");

    if has_banner && has_memmap {
        eprintln!("smoke test de boot: PASSOU");
    } else {
        eprintln!(
            "smoke test de boot: FALHOU (banner_presente={has_banner}, memmap_presente={has_memmap})"
        );
        eprintln!("--- saida serial capturada ---\n{output}\n------------------------------");
        std::process::exit(1);
    }
}

fn main() {
    let root = workspace_root();
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("build-image") => {
            build_image(&root);
        }
        Some("run") => {
            let extra = split_extra_args(&args[1..]);
            let status = run_qemu(&root, extra, false);
            std::process::exit(status.code().unwrap_or(1));
        }
        Some("test") => {
            run_test(&root);
        }
        _ => {
            eprintln!("uso: cargo xtask <build-image|run|test> [-- <args extras do qemu>]");
            std::process::exit(1);
        }
    }
}
