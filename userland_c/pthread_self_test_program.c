/* Programa C de verificacao de `pthread_self` real (change
 * `os-javaos-attach-thread`, design.md/tasks.md tarefa 3.1). Cria duas
 * threads via `pthread_create` (real, de `pthread-create-thread`),
 * guardando o `pthread_t` que a thread criadora recebeu de volta para cada
 * uma; cada thread criada chama `pthread_self()` e grava o resultado numa
 * variavel compartilhada; a thread principal confirma:
 *
 *   1) o `pthread_self()` de cada thread bate exatamente com o `pthread_t`
 *      que a thread criadora recebeu para ela (nunca um valor emprestado
 *      de outra thread nem de uma `syscall` diferente);
 *   2) os dois identificadores sao distintos entre si - nenhuma das duas
 *      threads nunca se confunde com a outra.
 *
 * Mesmo mecanismo de pthread_create_test_program.c (pthread_create real,
 * sem pthread_join - poll limitado por tempo).
 */

/* Mesmo achado/correção de userland_c/glue.c: pthread.h do newlib
 * vendorizado só expõe seu conteúdo sob _POSIX_THREADS, que não é definido
 * automaticamente para o alvo genérico x86_64-elf deste projeto — ver
 * design.md da change os-javaos-platform-sync. */
#define _POSIX_THREADS 1

#include <pthread.h>
#include <string.h>
#include <unistd.h>

/* Poll limitado por tempo (sem pthread_join real - Non-Goal de
 * pthread-create-thread) - generoso o bastante para o escalonador
 * round-robin certamente revezar para as duas threads criadas dentro desse
 * intervalo, mas nunca trava para sempre se algo der errado. */
#define JOIN_POLL_ITERATIONS 50000000

static volatile int g_b_ran = 0;
static volatile int g_c_ran = 0;
static volatile pthread_t g_self_b = 0;
static volatile pthread_t g_self_c = 0;

static void *thread_b_entry(void *arg) {
    (void)arg;
    g_self_b = pthread_self();
    g_b_ran = 1;
    return NULL;
}

static void *thread_c_entry(void *arg) {
    (void)arg;
    g_self_c = pthread_self();
    g_c_ran = 1;
    return NULL;
}

int main(void) {
    char result[4];
    memset(result, '?', sizeof(result));

    pthread_t tid_b = 0;
    pthread_t tid_c = 0;

    int rc_b = pthread_create(&tid_b, NULL, thread_b_entry, NULL);
    result[0] = (rc_b == 0) ? 'P' : 'F';

    int rc_c = pthread_create(&tid_c, NULL, thread_c_entry, NULL);
    result[1] = (rc_c == 0) ? 'P' : 'F';

    int joined = 0;
    for (long i = 0; i < JOIN_POLL_ITERATIONS; i++) {
        if (g_b_ran && g_c_ran) {
            joined = 1;
            break;
        }
    }
    result[2] = joined ? 'P' : 'F';

    /* Cobre as duas cenas do spec delta desta change: cada pthread_self()
     * bate com o pthread_t que a criadora recebeu (nunca emprestado da
     * outra thread), e os dois sao distintos entre si. */
    int self_matches_created_id = (g_self_b == tid_b) && (g_self_c == tid_c);
    int distinct_ids = (g_self_b != g_self_c) && (tid_b != tid_c);
    result[3] = (self_matches_created_id && distinct_ids) ? 'P' : 'F';

    char msg[160];
    const char prefix[] = "pthread_self_test_program: resultado ";
    size_t len = sizeof(prefix) - 1;
    memcpy(msg, prefix, len);
    memcpy(msg + len, result, sizeof(result));
    len += sizeof(result);
    const char suffix[] =
        " (ordem: thread_create_b,thread_create_c,join_poll,self_bate_e_ids_distintos)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += sizeof(suffix) - 1;
    write(1, msg, len);

    return 0;
}
