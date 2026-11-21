/* Programa C de verificacao de malloc/free concorrentes de verdade entre
 * duas threads do mesmo processo - change `libc-retargetable-locking`,
 * design.md/tasks.md tarefa 4.1. Prova que o locking retargetavel real do
 * newlib (__retarget_lock_*, sobre o mutex do kernel) protege o heap de
 * `malloc` contra corrupcao sob acesso concorrente - sem ele, duas threads
 * chamando malloc/free no mesmo processo corromperiam silenciosamente as
 * estruturas internas do alocador (free-list) do newlib.
 *
 * Nao usa pthread_create (Non-Goal desta change - fica para
 * `pthread-create-thread`): cria a thread-irma via SYS_THREAD_CREATE bruta,
 * mesmo mecanismo de sibling_thread_test_program.c. Cada thread roda um
 * laco independente de malloc/escreve-padrao/releitura/free, sem nenhuma
 * sincronizacao EXTERNA entre as duas - a propria mudanca eh quem deve
 * evitar corrupcao internamente (MALLOC_LOCK/MALLOC_UNLOCK reais).
 */

#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define SYS_EXIT 21
#define SYS_THREAD_CREATE 23

#define SIBLING_STACK_SIZE (64 * 1024)
#define JOIN_POLL_ITERATIONS 200000000

/* Numero de iteracoes do laco de malloc/free de cada thread, e tamanho do
 * "ring" de alocacoes mantidas vivas simultaneamente (nao liberadas na
 * hora) - da tempo real da outra thread interferir no meio do caminho,
 * antes de cada bloco ser reverificado e liberado. Mantido deliberadamente
 * pequeno (cada iteracao envolve pelo menos um malloc/free real, cada um
 * protegido por um SYSCALL_MUTEX_LOCK/UNLOCK de verdade - caro sob o
 * escalonador de 100 Hz/QEMU TCG quando multiplicado por milhares de
 * iteracoes em duas threads concorrentes) - ainda assim generoso o
 * bastante para expor corrupcao real, caso o locking retargetavel nao
 * protegesse o heap. */
#define ITERATIONS 300
#define RING_SIZE 8

static long raw_syscall(long num, long a0, long a1, long a2, long a3) {
    long ret;
    register long r10 __asm__("r10") = a3;
    __asm__ volatile("syscall"
                      : "=a"(ret)
                      : "a"(num), "D"(a0), "S"(a1), "d"(a2), "r"(r10)
                      : "rcx", "r11", "memory");
    return ret;
}

struct ring_entry {
    unsigned char *ptr;
    size_t size;
    unsigned char pattern;
};

/* Preenche `size` bytes em `ptr` com `pattern` e confirma imediatamente que
 * cada byte lido de volta bate. */
static int fill_and_verify(unsigned char *ptr, size_t size, unsigned char pattern) {
    for (size_t i = 0; i < size; i++) {
        ptr[i] = pattern;
    }
    for (size_t i = 0; i < size; i++) {
        if (ptr[i] != pattern) {
            return 0;
        }
    }
    return 1;
}

/* Executa o laco de malloc/free concorrente para uma thread, identificada
 * por `thread_tag` (byte alto do padrao escrito, para que os padroes de
 * uma thread nunca colidam por coincidencia com os da outra). Devolve 1 se
 * nenhuma corrupcao foi detectada em nenhuma das ITERATIONS alocacoes. */
static int malloc_concurrency_loop(unsigned char thread_tag) {
    struct ring_entry ring[RING_SIZE];
    memset(ring, 0, sizeof(ring));
    int ok = 1;

    for (int i = 0; i < ITERATIONS; i++) {
        int slot = i % RING_SIZE;
        struct ring_entry *e = &ring[slot];

        if (e->ptr != NULL) {
            /* Reverifica o conteudo de uma alocacao que ficou viva por
             * varias iteracoes antes de liberar - o momento mais provavel
             * de expor corrupcao real do heap causada pela outra thread. */
            for (size_t j = 0; j < e->size; j++) {
                if (e->ptr[j] != e->pattern) {
                    ok = 0;
                }
            }
            free(e->ptr);
            e->ptr = NULL;
        }

        size_t size = 16 + (size_t)(i % 6) * 16; /* varia 16..96 bytes */
        unsigned char pattern = (unsigned char)(thread_tag ^ (i & 0xff));
        unsigned char *p = (unsigned char *)malloc(size);
        if (p == NULL) {
            ok = 0;
            continue;
        }
        if (!fill_and_verify(p, size, pattern)) {
            ok = 0;
        }
        e->ptr = p;
        e->size = size;
        e->pattern = pattern;
    }

    /* Libera o que sobrou no ring, reverificando antes de cada free. */
    for (int slot = 0; slot < RING_SIZE; slot++) {
        struct ring_entry *e = &ring[slot];
        if (e->ptr != NULL) {
            for (size_t j = 0; j < e->size; j++) {
                if (e->ptr[j] != e->pattern) {
                    ok = 0;
                }
            }
            free(e->ptr);
        }
    }

    return ok;
}

static volatile int g_sibling_done = 0;
static volatile int g_sibling_ok = 0;

static void sibling_main(void) {
    g_sibling_ok = malloc_concurrency_loop(0xA0);
    g_sibling_done = 1;
    raw_syscall(SYS_EXIT, 0, 0, 0, 0);
    /* Nunca alcancado: SYS_EXIT nunca retorna. */
}

int main(void) {
    char result[5];
    memset(result, '?', sizeof(result));

    long thread_id = raw_syscall(SYS_THREAD_CREATE, (long)&sibling_main, SIBLING_STACK_SIZE, 0, 0);
    result[0] = (thread_id >= 0) ? 'P' : 'F';

    int main_ok = malloc_concurrency_loop(0x50);
    result[1] = main_ok ? 'P' : 'F';

    int joined = 0;
    for (long i = 0; i < JOIN_POLL_ITERATIONS; i++) {
        if (g_sibling_done) {
            joined = 1;
            break;
        }
    }
    result[2] = joined ? 'P' : 'F';
    result[3] = g_sibling_ok ? 'P' : 'F';

    /* Sanidade final: o heap continua utilizavel depois de toda a
     * concorrencia acima. */
    void *sanity = malloc(256);
    result[4] = (sanity != NULL) ? 'P' : 'F';
    free(sanity);

    char msg[192];
    const char prefix[] = "malloc_concurrency_test_program: resultado ";
    size_t len = sizeof(prefix) - 1;
    memcpy(msg, prefix, len);
    memcpy(msg + len, result, sizeof(result));
    len += sizeof(result);
    const char suffix[] =
        " (ordem: thread_create,main_sem_corrupcao,join_poll,irma_sem_corrupcao,"
        "heap_ainda_utilizavel)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += sizeof(suffix) - 1;
    write(1, msg, len);

    return 0;
}
