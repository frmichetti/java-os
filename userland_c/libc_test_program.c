/* Programa C de verificacao do port minimo de libc (newlib + glue sobre as
 * syscalls do Java OS) - ver change libc-minimal-port, design.md/tasks.md,
 * grupo 4. Nao Rust, nao embutido no kernel: um arquivo ELF real, compilado
 * pelo wrapper de cross-compilador e carregado via kernel::elf/spawn_from_elf
 * (mesmo mecanismo ja usado para /bin/test.elf - kernel-elf-loader).
 *
 * Exercita, nesta ordem, exatamente o conjunto de comportamentos da spec
 * libc-runtime: aloca memoria com malloc (via _sbrk), escreve uma mensagem
 * com fputs/write (via SYSCALL_WRITE), abre e le um arquivo fixture via
 * fopen+fread (via SYSCALL_OPEN/SYSCALL_READ/SYSCALL_CLOSE), confirma o
 * conteudo lido byte a byte, e termina via exit(0) (via SYSCALL_EXIT).
 *
 * Mesmo padrao de relato de 'P'/'F' por caractere dos demos em assembly do
 * kernel (ex. kernel::file_syscalls_demo), para nao precisar de nenhuma
 * rotina de formatacao numerica - so write() de uma unica mensagem com o
 * resultado.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Mesma fixture de 16 bytes ja usada por kernel-file-syscalls
 * (kernel/testdata/file_syscalls_fixture.txt), copiada para ::/TEST.TXT na
 * imagem de disco. */
static const char EXPECTED[16] = "ABCDEFGHIJKLMNOP";

int main(void) {
    char result[8];
    memset(result, '?', sizeof(result));

    /* 1) malloc: aloca e escreve um padrao conhecido. */
    unsigned char *block = malloc(64);
    result[0] = 'F';
    if (block != NULL) {
        for (int i = 0; i < 64; i++) {
            block[i] = (unsigned char)i;
        }
        int ok = 1;
        for (int i = 0; i < 64; i++) {
            if (block[i] != (unsigned char)i) {
                ok = 0;
                break;
            }
        }
        if (ok) {
            result[0] = 'P';
        }
    }

    /* 2) escrita no console via fputs (stdout). */
    result[1] = 'F';
    if (fputs("libc_test_program: ola via fputs/stdout\n", stdout) >= 0) {
        result[1] = 'P';
    }

    /* 3) abrir e ler o arquivo fixture via fopen/fread. */
    result[2] = 'F';
    result[3] = 'F';
    FILE *f = fopen("/TEST.TXT", "r");
    if (f != NULL) {
        result[2] = 'P';
        char buf[sizeof(EXPECTED)];
        size_t n = fread(buf, 1, sizeof(buf), f);
        if (n == sizeof(EXPECTED) && memcmp(buf, EXPECTED, sizeof(EXPECTED)) == 0) {
            result[3] = 'P';
        }
        fclose(f);
    }

    /* 4) reporta o resultado uma unica vez, via write() bruto (sem
     * depender de printf/%f, que esta libc nao suporta - ver design.md). */
    char msg[128];
    int len = 0;
    const char prefix[] = "libc_test_program: resultado ";
    memcpy(msg, prefix, sizeof(prefix) - 1);
    len = (int)(sizeof(prefix) - 1);
    memcpy(msg + len, result, 4);
    len += 4;
    const char suffix[] = " (ordem: malloc,write,open,read)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += (int)(sizeof(suffix) - 1);
    write(1, msg, (size_t)len);

    free(block);
    return 0;
}
