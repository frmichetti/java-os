/* Programa C de verificacao de TLS (armazenamento local de thread) real
 * por thread (change `kernel-thread-local-storage`, design.md). ELF real,
 * compilado pelo wrapper de cross-compilador e carregado via
 * kernel::elf/spawn_from_elf (mesmo mecanismo ja usado para os demais
 * programas de teste).
 *
 * Declara uma variavel `__thread int` (endereçada via `%fs:offset` pelo
 * codigo gerado pelo gcc - nenhuma syscall bruta especial para isso, e
 * exatamente o ponto: se o kernel nao desse a cada thread seu proprio
 * `FS.base`, as duas threads deste programa enxergariam a MESMA variavel).
 * A thread principal escreve um valor nela, cria uma thread-irma via
 * SYS_THREAD_CREATE que escreve um valor DIFERENTE na sua propria copia, e
 * a thread principal confirma que a sua propria continua com o valor que
 * ela escreveu - nunca o da thread-irma.
 *
 * Mesmo mecanismo de comunicacao de volta (variavel global comum, poll
 * limitado sem pthread_join - Non-Goal desta change) de
 * sibling_thread_test_program.c (change kernel-user-thread-create).
 */

#include <string.h>
#include <unistd.h>

#define SYS_THREAD_CREATE 23
#define SYS_EXIT 21

#define SIBLING_STACK_SIZE (64 * 1024)
#define JOIN_POLL_ITERATIONS 50000000

static long raw_syscall(long num, long a0, long a1, long a2, long a3) {
    long ret;
    register long r10 __asm__("r10") = a3;
    __asm__ volatile("syscall"
                      : "=a"(ret)
                      : "a"(num), "D"(a0), "S"(a1), "d"(a2), "r"(r10)
                      : "rcx", "r11", "memory");
    return ret;
}

/* A variavel sob teste: cada thread deve enxergar sua propria copia. */
static __thread int g_counter;

/* Comunicacao de volta para a thread principal - globais comuns (nao
 * __thread), deliberadamente, para provar que SAO compartilhadas entre as
 * duas threads (ao contrario de `g_counter`). */
static volatile int g_sibling_ran = 0;
static volatile int g_sibling_saw_in_its_own_copy = 0;

static void sibling_main(void) {
    g_counter = 222;
    g_sibling_saw_in_its_own_copy = (g_counter == 222) ? 1 : 0;
    g_sibling_ran = 1;
    raw_syscall(SYS_EXIT, 0, 0, 0, 0);
    /* Nunca alcancado: SYS_EXIT nunca retorna. */
}

int main(void) {
    char result[3];
    memset(result, '?', sizeof(result));

    g_counter = 111;

    /* 1) Cria a thread-irma. */
    long thread_id = raw_syscall(SYS_THREAD_CREATE, (long)&sibling_main, SIBLING_STACK_SIZE, 0, 0);
    result[0] = (thread_id >= 0) ? 'P' : 'F';

    /* 2) Espera a thread-irma terminar (poll limitado). */
    int joined = 0;
    for (long i = 0; i < JOIN_POLL_ITERATIONS; i++) {
        if (g_sibling_ran) {
            joined = 1;
            break;
        }
    }
    result[1] = (joined && g_sibling_saw_in_its_own_copy) ? 'P' : 'F';

    /* 3) A propria copia de g_counter desta thread continua 111 - nao foi
     * sobrescrita pela escrita da thread-irma na copia DELA. */
    result[2] = (g_counter == 111) ? 'P' : 'F';

    char msg[160];
    const char prefix[] = "tls_test_program: resultado ";
    size_t len = sizeof(prefix) - 1;
    memcpy(msg, prefix, len);
    memcpy(msg + len, result, sizeof(result));
    len += sizeof(result);
    const char suffix[] = " (ordem: thread_create,irma_escreveu_sua_copia,copia_principal_intacta)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += sizeof(suffix) - 1;
    write(1, msg, len);

    return 0;
}
