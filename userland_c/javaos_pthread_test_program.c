/* Programa C de verificacao das funcoes pthread_mutex_* e pthread_cond_*
 * reais de userland_c/glue.c (change os-javaos-platform-sync, design.md). ELF
 * real,
 * compilado pelo wrapper de cross-compilador e carregado via
 * kernel::elf/spawn_from_elf (mesmo mecanismo ja usado para
 * /bin/libctest.elf/cxxtest.elf/javaosvm.elf).
 *
 * Exercita, nesta ordem, exatamente o que uma unica thread consegue provar
 * sem nenhuma syscall de criacao de thread em modo usuario (que nao existe
 * ainda - ver design.md): pthread_mutex_init/lock/trylock(falha, ja tomado
 * pela propria thread)/unlock/trylock(sucesso, livre de novo)/unlock/destroy,
 * e pthread_cond_init/signal/broadcast/destroy sem nenhum esperador.
 *
 * Deliberadamente NAO chama pthread_cond_wait: sem nenhuma outra execucao
 * para notificar de volta, bloquearia este processo de teste para sempre,
 * travando toda a verificacao em boot real (mesma cautela ja documentada em
 * javaos_vm_test_program.c sobre nao provocar um #PF real apos
 * protect(somente leitura)).
 */

/* Mesmo achado/correção de userland_c/glue.c: pthread.h do newlib
 * vendorizado só expõe seu conteúdo sob _POSIX_THREADS, que não é definido
 * automaticamente para o alvo genérico x86_64-elf deste projeto — ver
 * design.md da change os-javaos-platform-sync. */
#define _POSIX_THREADS 1

#include <errno.h>
#include <pthread.h>
#include <string.h>
#include <unistd.h>

int main(void) {
    char result[10];
    memset(result, '?', sizeof(result));

    /* 1) pthread_mutex_init. */
    pthread_mutex_t mutex;
    result[0] = (pthread_mutex_init(&mutex, NULL) == 0) ? 'P' : 'F';

    /* 2) pthread_mutex_lock sobre um mutex livre. */
    result[1] = (pthread_mutex_lock(&mutex) == 0) ? 'P' : 'F';

    /* 3) pthread_mutex_trylock sobre o mesmo mutex, ja tomado pela propria
     * thread chamadora - deve falhar sem bloquear. */
    result[2] = (pthread_mutex_trylock(&mutex) == EBUSY) ? 'P' : 'F';

    /* 4) pthread_mutex_unlock. */
    result[3] = (pthread_mutex_unlock(&mutex) == 0) ? 'P' : 'F';

    /* 5) pthread_mutex_trylock sobre o mutex livre de novo - deve ter
     * sucesso imediato. */
    result[4] = (pthread_mutex_trylock(&mutex) == 0) ? 'P' : 'F';

    /* 6) pthread_mutex_unlock de novo (liberar o que a tarefa 5 tomou). */
    result[5] = (pthread_mutex_unlock(&mutex) == 0) ? 'P' : 'F';

    /* 7) pthread_mutex_destroy. */
    result[6] = (pthread_mutex_destroy(&mutex) == 0) ? 'P' : 'F';

    /* 8) pthread_cond_init. */
    pthread_cond_t cond;
    result[7] = (pthread_cond_init(&cond, NULL) == 0) ? 'P' : 'F';

    /* 9) pthread_cond_signal/pthread_cond_broadcast sem nenhum esperador. */
    result[8] =
        (pthread_cond_signal(&cond) == 0 && pthread_cond_broadcast(&cond) == 0) ? 'P' : 'F';

    /* 10) pthread_cond_destroy. */
    result[9] = (pthread_cond_destroy(&cond) == 0) ? 'P' : 'F';

    char msg[192];
    int len = 0;
    const char prefix[] = "javaos_pthread_test_program: resultado ";
    memcpy(msg, prefix, sizeof(prefix) - 1);
    len = (int)(sizeof(prefix) - 1);
    memcpy(msg + len, result, sizeof(result));
    len += (int)sizeof(result);
    const char suffix[] =
        " (ordem: mutex_init,lock,trylock_busy,unlock,trylock_free,unlock2,mutex_destroy,"
        "cond_init,signal_broadcast,cond_destroy)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += (int)(sizeof(suffix) - 1);
    write(1, msg, (size_t)len);

    return 0;
}
