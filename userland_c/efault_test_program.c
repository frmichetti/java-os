/* Programa C de verificacao da validacao de ponteiro de usuario (change
 * kernel-user-pointer-validation, design.md). ELF real, compilado pelo
 * wrapper de cross-compilador e carregado via kernel::elf/spawn_from_elf
 * (mesmo mecanismo ja usado para os demais programas de teste).
 *
 * Ao contrario de fault_test_program.c (que causa deliberadamente um page
 * fault para provar o isolamento de falha), este programa nunca deveria
 * causar nenhuma falta de pagina: ele invoca diretamente, via syscall bruta
 * (sem passar pelos wrappers de glue.c, para poder distinguir o sentinela
 * SYSCALL_EFAULT de SYSCALL_ERROR), as syscalls que recebem ponteiro de modo
 * usuario com ponteiros deliberadamente invalidos, e confirma que cada uma
 * e rejeitada com SYSCALL_EFAULT - nunca com uma excecao de CPU.
 */

#include <string.h>
#include <unistd.h>

#define SYS_WRITE 0
#define SYS_OPEN 18
#define SYS_READ 19
#define SYS_CLOSE 20

/* Mesma convencao de glue.c: SYSCALL_ERROR/SYSCALL_EFAULT do kernel sao
 * `u64::MAX`/`u64::MAX - 2`, que como `long` assinado de 64 bits sao
 * exatamente -1/-3. */
#define SYSCALL_ERROR (-1L)
#define SYSCALL_EFAULT (-3L)

/* Mesmo deslocamento de glue.c entre descritor POSIX e handle do kernel. */
#define FD_OFFSET 3

static long raw_syscall(long num, long a0, long a1, long a2, long a3) {
    long ret;
    register long r10 __asm__("r10") = a3;
    __asm__ volatile("syscall"
                      : "=a"(ret)
                      : "a"(num), "D"(a0), "S"(a1), "d"(a2), "r"(r10)
                      : "rcx", "r11", "memory");
    return ret;
}

int main(void) {
    char result[4];
    memset(result, '?', sizeof(result));

    /* 1) ponteiro valido (stack do proprio programa) continua funcionando
     * exatamente como antes desta change. */
    const char valid_msg[] = "x";
    long r = raw_syscall(SYS_WRITE, (long)valid_msg, 1, 0, 0);
    result[0] = (r == 1) ? 'P' : 'F';

    /* 2) ponteiro nulo (pagina 0, nunca mapeada - mesma pagina que
     * fault_test_program.c usa para provocar seu page fault deliberado) em
     * SYS_WRITE e rejeitado com SYSCALL_EFAULT, sem nenhuma excecao de CPU. */
    r = raw_syscall(SYS_WRITE, 0, 8, 0, 0);
    result[1] = (r == SYSCALL_EFAULT) ? 'P' : 'F';

    /* 3) ponteiro de caminho invalido (nao nulo, mas dentro da mesma pagina
     * 0 nunca mapeada) em SYS_OPEN e rejeitado, sem chegar a montar o
     * sistema de arquivos nem ler nada. */
    r = raw_syscall(SYS_OPEN, 1, 4, 0, 0);
    result[2] = (r == SYSCALL_EFAULT) ? 'P' : 'F';

    /* 4) buffer de escrita invalido em SYS_READ e rejeitado sem avancar o
     * cursor do arquivo: uma leitura valida logo depois ainda ve o primeiro
     * byte do arquivo fixture (mesma fixture de 16 bytes de
     * libc_test_program.c, "ABCDEFGHIJKLMNOP"). */
    result[3] = 'F';
    const char path[] = "/TEST.TXT";
    long h = raw_syscall(SYS_OPEN, (long)path, (long)(sizeof(path) - 1), 0, 0);
    if (h != SYSCALL_ERROR) {
        long bad = raw_syscall(SYS_READ, h, 0, 1, 0);
        char first_byte = 0;
        long got = raw_syscall(SYS_READ, h, (long)&first_byte, 1, 0);
        if (bad == SYSCALL_EFAULT && got == 1 && first_byte == 'A') {
            result[3] = 'P';
        }
        raw_syscall(SYS_CLOSE, h, 0, 0, 0);
    }

    char msg[160];
    const char prefix[] = "efault_test_program: resultado ";
    size_t len = sizeof(prefix) - 1;
    memcpy(msg, prefix, len);
    memcpy(msg + len, result, sizeof(result));
    len += sizeof(result);
    const char suffix[] =
        " (ordem: write_valido,write_nulo,open_invalido,read_invalido_sem_consumir_cursor)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += sizeof(suffix) - 1;
    write(1, msg, len);

    return 0;
}
