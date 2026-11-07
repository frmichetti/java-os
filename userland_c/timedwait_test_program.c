/* Programa C de verificacao da espera com prazo real (change
 * kernel-timed-wait, design.md). ELF real, compilado pelo wrapper de
 * cross-compilador e carregado via kernel::elf/spawn_from_elf (mesmo
 * mecanismo ja usado para /bin/jpthread.elf/libm.elf).
 *
 * Ao contrario de toda espera sem prazo ja entregue por changes anteriores
 * (nunca provada em boot real por falta de syscall de criacao de thread em
 * modo usuario - ver os-javaos-platform-sync), o caminho de EXPIRACAO desta
 * primitiva nao precisa de nenhuma segunda execucao: chama
 * pthread_cond_timedwait com um prazo curto sobre uma variavel de condicao
 * que ninguem mais notifica, mede o tempo decorrido via
 * clock_gettime(CLOCK_MONOTONIC) antes/depois, e confirma (a) que a chamada
 * retornou ETIMEDOUT, e (b) que o tempo decorrido e compativel com o prazo
 * pedido (maior ou igual a ele, dentro da granularidade de 100 Hz do timer).
 * Nao testa o caminho "notificado antes de expirar" - mesma limitacao de
 * sempre (sem syscall de criacao de thread).
 */

/* Mesmo achado/correção de glue.c: pthread.h/time.h do newlib vendorizado só
 * expõem seu conteúdo sob _POSIX_THREADS/_POSIX_TIMERS/_POSIX_MONOTONIC_CLOCK,
 * não definidos automaticamente para o alvo genérico x86_64-elf deste
 * projeto — ver design.md da change kernel-timed-wait. */
#define _POSIX_THREADS 1
#define _POSIX_TIMERS 1
#define _POSIX_MONOTONIC_CLOCK 1

#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define TIMEOUT_MILLIS 50

static int64_t timespec_to_nanos(const struct timespec *ts) {
    return (int64_t)ts->tv_sec * 1000000000LL + (int64_t)ts->tv_nsec;
}

int main(void) {
    char result[2];
    memset(result, '?', sizeof(result));

    pthread_mutex_t mutex;
    pthread_cond_t cond;
    pthread_mutex_init(&mutex, NULL);
    pthread_cond_init(&cond, NULL);

    struct timespec before;
    clock_gettime(CLOCK_MONOTONIC, &before);

    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += TIMEOUT_MILLIS / 1000;
    deadline.tv_nsec += (TIMEOUT_MILLIS % 1000) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_nsec -= 1000000000L;
        deadline.tv_sec += 1;
    }

    pthread_mutex_lock(&mutex);
    int rc = pthread_cond_timedwait(&cond, &mutex, &deadline);
    pthread_mutex_unlock(&mutex);

    struct timespec after;
    clock_gettime(CLOCK_MONOTONIC, &after);

    /* 1) a chamada deve ter retornado ETIMEDOUT (ninguem notificou). */
    result[0] = (rc == ETIMEDOUT) ? 'P' : 'F';

    /* 2) o tempo decorrido deve ser compativel com o prazo pedido. */
    int64_t elapsed_nanos = timespec_to_nanos(&after) - timespec_to_nanos(&before);
    int64_t expected_nanos = (int64_t)TIMEOUT_MILLIS * 1000000LL;
    result[1] = (elapsed_nanos >= expected_nanos) ? 'P' : 'F';

    pthread_cond_destroy(&cond);
    pthread_mutex_destroy(&mutex);

    char msg[96];
    int len = 0;
    const char prefix[] = "timedwait_test_program: resultado ";
    memcpy(msg, prefix, sizeof(prefix) - 1);
    len = (int)(sizeof(prefix) - 1);
    memcpy(msg + len, result, sizeof(result));
    len += (int)sizeof(result);
    const char suffix[] = " (ordem: etimedout,elapsed_ge_requested)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += (int)(sizeof(suffix) - 1);
    write(1, msg, (size_t)len);

    return 0;
}
