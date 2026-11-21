/* Programa C de verificacao de `pthread_create` real (change
 * `pthread-create-thread`, design.md/tasks.md tarefa 4.1). Ao contrario de
 * sibling_thread_test_program.c (que invoca SYSCALL_THREAD_CREATE bruta,
 * sem nenhuma forma de passar um argumento), este programa usa a API POSIX
 * padrao de verdade (`pthread_create`/`glue.c`), passando um ponteiro para
 * uma struct alocada no heap com um valor conhecido - a thread criada o le
 * e escreve um resultado derivado dele numa variavel compartilhada; a
 * thread principal confirma esse resultado, provando que o argumento
 * chegou intacto atraves do trampolim (`pthread_trampoline`, `glue.c`) e,
 * por extensao, que `SYSCALL_THREAD_CREATE` entrega de verdade o terceiro
 * argumento em `RDI` (`kernel::process::init_stack`).
 */

/* Mesmo achado/correção de userland_c/glue.c: pthread.h do newlib
 * vendorizado só expõe seu conteúdo sob _POSIX_THREADS, que não é definido
 * automaticamente para o alvo genérico x86_64-elf deste projeto — ver
 * design.md da change os-javaos-platform-sync. */
#define _POSIX_THREADS 1

#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Poll limitado por tempo (sem pthread_join real - Non-Goal desta change,
 * ver design.md) - generoso o bastante para o escalonador round-robin
 * certamente revezar para a thread criada dentro desse intervalo, mas
 * nunca trava para sempre se algo der errado. */
#define JOIN_POLL_ITERATIONS 50000000

struct thread_arg {
    int value;
};

static volatile int g_thread_ran = 0;
static volatile int g_result = 0;

static void *thread_entry(void *arg) {
    struct thread_arg *targ = (struct thread_arg *)arg;
    int value = targ->value;
    free(targ);

    g_result = value * 2;
    g_thread_ran = 1;
    return NULL;
}

int main(void) {
    char result[3];
    memset(result, '?', sizeof(result));

    struct thread_arg *targ = malloc(sizeof(struct thread_arg));
    result[0] = (targ != NULL) ? 'P' : 'F';
    targ->value = 21;

    pthread_t tid;
    int rc = pthread_create(&tid, NULL, thread_entry, targ);
    result[1] = (rc == 0) ? 'P' : 'F';

    int joined = 0;
    for (long i = 0; i < JOIN_POLL_ITERATIONS; i++) {
        if (g_thread_ran) {
            joined = 1;
            break;
        }
    }

    /* `result[2]` cobre as duas cenas em um unico "P": a thread realmente
     * executou (joined) E leu exatamente o argumento informado (42 == 21 *
     * 2, nunca lixo nem o valor de outra alocacao). */
    result[2] = (joined && g_result == 42) ? 'P' : 'F';

    char msg[160];
    const char prefix[] = "pthread_create_test_program: resultado ";
    size_t len = sizeof(prefix) - 1;
    memcpy(msg, prefix, len);
    memcpy(msg + len, result, sizeof(result));
    len += sizeof(result);
    const char suffix[] = " (ordem: malloc_arg,pthread_create,join_e_argumento_intacto)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += sizeof(suffix) - 1;
    write(1, msg, len);

    return 0;
}
