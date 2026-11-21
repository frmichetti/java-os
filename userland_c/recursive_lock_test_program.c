/* Programa C de verificacao de reentrancia real de um lock interno
 * recursivo do newlib (__retarget_lock_acquire_recursive/
 * __retarget_lock_release_recursive, change `libc-retargetable-locking`,
 * design.md/tasks.md tarefa 4.2). Chama as funcoes de retarget
 * diretamente - nao ha API publica de mais alto nivel para isso neste
 * projeto (ver design.md, Non-Goals: __retarget_lock_* e struct __lock
 * sao infraestrutura interna do retarget do newlib, nunca uma capability
 * de usuario final exposta por este projeto).
 *
 * Prova as duas cenas de specs/libc-retargetable-locking/spec.md,
 * Requirement "Reentrancia Segura em Locks Recursivos":
 *   1) a mesma thread adquire o mesmo lock recursivo duas vezes seguidas
 *      sem bloquear a si mesma;
 *   2) uma thread diferente, tentando adquirir o mesmo lock enquanto a
 *      primeira o detem (mesmo depois de UMA liberacao, mas nao das
 *      duas), continua bloqueada - so prossegue depois da segunda
 *      liberacao correspondente.
 *
 * Mesmo mecanismo de sibling_thread_test_program.c para a thread-irma
 * (SYS_THREAD_CREATE bruta, sem pthread_create - Non-Goal desta change).
 */

#include <string.h>
#include <sys/lock.h>
#include <unistd.h>

#define SYS_EXIT 21
#define SYS_THREAD_CREATE 23

#define SIBLING_STACK_SIZE (64 * 1024)
/* Atraso fixo (sem `break` antecipado) para dar tempo real da thread-irma
 * tentar adquirir o lock e observar se ele foi liberado cedo demais -
 * custa sempre o tempo cheio, ao contrario de JOIN_POLL_ITERATIONS
 * (quebra assim que a condicao esperada e observada). */
#define DELAY_ITERATIONS 3000000
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

static _LOCK_T g_lock;

/* Protocolo de sincronizacao com a thread-irma, via variaveis globais -
 * mesmo espaco de enderecamento (ver sibling_thread_test_program.c). */
static volatile int g_main_acquired_twice = 0;
static volatile int g_sibling_acquired = 0;
static volatile int g_sibling_done = 0;

static void sibling_main(void) {
    /* So tenta adquirir depois que a thread principal garantidamente ja
     * adquiriu duas vezes - garante que esta aquisicao realmente encontra
     * o lock detido (bloqueio de verdade), nao uma corrida. */
    while (!g_main_acquired_twice) {
    }

    __retarget_lock_acquire_recursive(g_lock);
    g_sibling_acquired = 1;
    __retarget_lock_release_recursive(g_lock);

    g_sibling_done = 1;
    raw_syscall(SYS_EXIT, 0, 0, 0, 0);
    /* Nunca alcancado: SYS_EXIT nunca retorna. */
}

int main(void) {
    char result[5];
    memset(result, '?', sizeof(result));

    __retarget_lock_init_recursive(&g_lock);
    result[0] = (g_lock != NULL) ? 'P' : 'F';

    /* 1) A mesma thread adquire o mesmo lock recursivo duas vezes seguidas
     * sem bloquear a si mesma - se bloqueasse, este programa nunca
     * chegaria ate aqui (sem pthread_join nesta change, um deadlock aqui
     * apareceria como timeout de boot, nao como 'F'). */
    __retarget_lock_acquire_recursive(g_lock);
    __retarget_lock_acquire_recursive(g_lock);
    result[1] = 'P';

    long thread_id = raw_syscall(SYS_THREAD_CREATE, (long)&sibling_main, SIBLING_STACK_SIZE, 0, 0);
    result[2] = (thread_id >= 0) ? 'P' : 'F';

    g_main_acquired_twice = 1;

    /* 2) Libera so uma vez (depth 2 -> 1) - o lock continua detido pela
     * thread principal, entao a thread-irma deve continuar bloqueada.
     * Espera um numero generoso de iteracoes e confirma que ela realmente
     * nao adquiriu ainda. */
    __retarget_lock_release_recursive(g_lock);
    for (long i = 0; i < DELAY_ITERATIONS; i++) {
    }
    result[3] = (!g_sibling_acquired) ? 'P' : 'F';

    /* 3) Libera a segunda vez (depth 1 -> 0) - so agora o mutex real do
     * kernel e liberado, e a thread-irma (bloqueada em SYS_MUTEX_LOCK)
     * finalmente consegue adquirir. */
    __retarget_lock_release_recursive(g_lock);

    int joined = 0;
    for (long i = 0; i < JOIN_POLL_ITERATIONS; i++) {
        if (g_sibling_done) {
            joined = 1;
            break;
        }
    }
    result[4] = (joined && g_sibling_acquired) ? 'P' : 'F';

    char msg[224];
    const char prefix[] = "recursive_lock_test_program: resultado ";
    size_t len = sizeof(prefix) - 1;
    memcpy(msg, prefix, len);
    memcpy(msg + len, result, sizeof(result));
    len += sizeof(result);
    const char suffix[] =
        " (ordem: lock_init,mesma_thread_reentra_sem_bloquear,thread_create,"
        "uma_liberacao_nao_basta,segunda_liberacao_libera_irma)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += sizeof(suffix) - 1;
    write(1, msg, len);

    return 0;
}
