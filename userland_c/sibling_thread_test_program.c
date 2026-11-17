/* Programa C de verificacao da nova syscall de criacao de thread de usuario
 * (change `kernel-user-thread-create`, design.md). ELF real, compilado pelo
 * wrapper de cross-compilador e carregado via kernel::elf/spawn_from_elf
 * (mesmo mecanismo ja usado para os demais programas de teste).
 *
 * Ao contrario de javaos_pthread_test_program.c (que so podia provar o que
 * uma unica thread consegue fazer sozinha - comentario la, "sem nenhuma
 * syscall de criacao de thread de usuario, que nao existe ainda"), este
 * programa cria uma thread-irma de verdade via SYS_THREAD_CREATE e prova:
 *
 *   1) a thread-irma realmente executa (ve o endereco de entrada passado);
 *   2) ela enxerga e opera sobre o mesmo mutex e o mesmo arquivo aberto do
 *      processo (posse de handle e por processo, nao por thread - ver
 *      kernel-handle-ownership);
 *   3) depois que a thread-irma termina (via SYS_EXIT), a thread principal
 *      continua usando normalmente esse mesmo mutex/arquivo - o encerramento
 *      de uma thread nao destroi os recursos do processo enquanto outra
 *      thread dele continua viva (o proprio motivo do gate de "ultima
 *      thread" em kernel::scheduler::schedule_next);
 *   4) a thread principal, terminando por ultimo (via `crt0`/`_exit` normal
 *      ao `main` retornar), aciona a limpeza completa do processo sem
 *      travar.
 *
 * Nao usa pthread_create nem os wrappers de glue.c (Non-Goal desta change -
 * ver design.md): invoca a syscall bruta diretamente, mesmo mecanismo de
 * efault_test_program.c.
 *
 * O ponto de entrada da thread-irma (`sibling_main`) e uma funcao C comum,
 * nunca chamada via `crt0`/`main` - o kernel zera todos os registradores de
 * uso geral antes de retoma-la pela primeira vez (ver
 * `kernel::process::init_stack`), entao ela nao recebe nenhum argumento;
 * toda comunicacao com a thread principal e feita via variaveis globais
 * (o mesmo espaco de enderecamento, ja que as duas threads compartilham o
 * processo). `sibling_main` nunca pode retornar normalmente (nenhum
 * endereco de retorno foi empilhado para ela) - encerra sempre via
 * SYS_EXIT.
 */

#include <string.h>
#include <unistd.h>

#define SYS_WRITE 0
#define SYS_MUTEX_CREATE 7
#define SYS_MUTEX_LOCK 8
#define SYS_MUTEX_UNLOCK 9
#define SYS_OPEN 18
#define SYS_READ 19
#define SYS_CLOSE 20
#define SYS_EXIT 21
#define SYS_THREAD_CREATE 23

/* Tamanho de stack de usuario pedido para a thread-irma - bem abaixo de
 * `kernel::stack::USER_THREAD_STACK_MAX_SIZE` (8 MiB). */
#define SIBLING_STACK_SIZE (64 * 1024)

/* Numero de iteracoes do poll limitado por tempo (ver design.md - sem
 * `pthread_join` nesta change, Non-Goal). Generoso o bastante para o
 * escalonador round-robin certamente revezar para a thread-irma dentro
 * desse intervalo, mas nunca trava para sempre se algo der errado. */
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

/* Handles do processo, preenchidos pela thread principal antes de criar a
 * thread-irma - lidos por ela como prova de posse por processo, nao por
 * thread (kernel-handle-ownership). */
static volatile long g_mutex_handle = -1;
static volatile long g_file_handle = -1;

/* Comunicacao de volta para a thread principal. */
static volatile int g_sibling_ran = 0;
static volatile int g_sibling_read_first_byte_ok = 0;

static void sibling_main(void) {
    /* Opera sobre o mutex e o arquivo do PROCESSO (criados pela thread
     * principal antes desta thread existir) - nunca os cria de novo. */
    long lock_r = raw_syscall(SYS_MUTEX_LOCK, g_mutex_handle, 0, 0, 0);
    if (lock_r == 0) {
        char first_byte = 0;
        long got = raw_syscall(SYS_READ, g_file_handle, (long)&first_byte, 1, 0);
        g_sibling_read_first_byte_ok = (got == 1 && first_byte == 'A') ? 1 : 0;
        raw_syscall(SYS_MUTEX_UNLOCK, g_mutex_handle, 0, 0, 0);
    }

    g_sibling_ran = 1;
    raw_syscall(SYS_EXIT, 0, 0, 0, 0);
    /* Nunca alcancado: SYS_EXIT nunca retorna. */
}

int main(void) {
    char result[8];
    memset(result, '?', sizeof(result));

    /* 1) Mutex e arquivo do processo, criados pela thread principal antes
     * de a thread-irma existir. */
    long mutex = raw_syscall(SYS_MUTEX_CREATE, 0, 0, 0, 0);
    result[0] = (mutex >= 0) ? 'P' : 'F';

    const char path[] = "/TEST.TXT";
    long file = raw_syscall(SYS_OPEN, (long)path, (long)(sizeof(path) - 1), 0, 0);
    result[1] = (file >= 0) ? 'P' : 'F';

    g_mutex_handle = mutex;
    g_file_handle = file;

    /* 2) Cria a thread-irma, compartilhando o espaco de enderecamento deste
     * processo. */
    long thread_id = raw_syscall(SYS_THREAD_CREATE, (long)&sibling_main, SIBLING_STACK_SIZE, 0, 0);
    result[2] = (thread_id >= 0) ? 'P' : 'F';

    /* 3) Espera a thread-irma terminar (poll limitado - sem pthread_join
     * nesta change). */
    int joined = 0;
    for (long i = 0; i < JOIN_POLL_ITERATIONS; i++) {
        if (g_sibling_ran) {
            joined = 1;
            break;
        }
    }
    result[3] = joined ? 'P' : 'F';

    /* 4) A thread-irma realmente leu o primeiro byte do arquivo
     * compartilhado ('A', fixture de 16 bytes "ABCDEFGHIJKLMNOP" - mesma
     * fixture de libc_test_program.c/efault_test_program.c), sob o mutex
     * compartilhado. */
    result[4] = g_sibling_read_first_byte_ok ? 'P' : 'F';

    /* 5) Depois que a thread-irma terminou, o mutex do processo continua
     * utilizavel pela thread principal - seu encerramento (SYS_EXIT) nao
     * destruiu os recursos do processo, ja que esta thread ainda esta viva
     * (gate de "ultima thread" em kernel::scheduler::schedule_next). */
    long relock_r = raw_syscall(SYS_MUTEX_LOCK, mutex, 0, 0, 0);
    long unlock_r = raw_syscall(SYS_MUTEX_UNLOCK, mutex, 0, 0, 0);
    result[5] = (relock_r == 0 && unlock_r == 0) ? 'P' : 'F';

    /* 6) O arquivo do processo tambem continua utilizavel, com o cursor
     * realmente avancado pela leitura da thread-irma (compartilhado entre
     * as duas - mesmo handle, mesmo processo): o proximo byte e 'B', nao
     * 'A' de novo. */
    char second_byte = 0;
    long got2 = raw_syscall(SYS_READ, file, (long)&second_byte, 1, 0);
    result[6] = (got2 == 1 && second_byte == 'B') ? 'P' : 'F';

    /* 7) Fecha o arquivo (so a thread principal - a thread-irma nunca
     * fecha nada, para nao invalidar o handle enquanto a thread principal
     * ainda o usa). */
    long close_r = raw_syscall(SYS_CLOSE, file, 0, 0, 0);
    result[7] = (close_r == 0) ? 'P' : 'F';

    char msg[224];
    const char prefix[] = "sibling_thread_test_program: resultado ";
    size_t len = sizeof(prefix) - 1;
    memcpy(msg, prefix, len);
    memcpy(msg + len, result, sizeof(result));
    len += sizeof(result);
    const char suffix[] =
        " (ordem: mutex_create,file_open,thread_create,join_poll,irma_leu_primeiro_byte,"
        "mutex_apos_irma,arquivo_cursor_compartilhado,file_close)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += sizeof(suffix) - 1;
    write(1, msg, len);

    /* A thread principal termina por ultimo (via `crt0`/`_exit` normal ao
     * `main` retornar) - aciona a limpeza completa do processo (gate de
     * "ultima thread"). */
    return 0;
}
