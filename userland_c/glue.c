/* Stubs de retarget do newlib sobre as syscalls do Java OS - ver change
 * libc-minimal-port, design.md, "Sem libc/sys do alvo dentro da arvore do
 * newlib".
 *
 * Achado durante a implementacao (ver tasks.md, grupo 4): a build do
 * newlib para um alvo sem `sys_dir` conhecido (nosso caso) define
 * `MISSING_SYSCALL_NAMES`, que faz `newlib/libc/include/_syslist.h`
 * redirecionar as chamadas internas dos arquivos de `newlib/libc/reent`
 * dos nomes com underscore (`_read`, `_write`, ...) para os nomes POSIX
 * sem underscore (`read`, `write`, ...) - confirmado pelo erro de link
 * "undefined reference to `read'" apontando exatamente para dentro de
 * `libc/reent/readr.c`. Por isso as funcoes abaixo usam os nomes SEM
 * underscore (exceto `_exit`, que nao faz parte desse mapeamento - `_exit`
 * e o nome POSIX padrao em si).
 *
 * Compilado fora da arvore do newlib e linkado junto com `libc.a` no link
 * final de cada programa.
 */

/* Achado real durante a implementação (change `os-javaos-platform-sync`):
 * `pthread.h` do newlib vendorizado envolve TODO o seu conteúdo (protótipos
 * de função e macros como `PTHREAD_MUTEX_INITIALIZER`) num único
 * `#if defined(_POSIX_THREADS)` (ver `sys/features.h`) — e esse macro só é
 * definido automaticamente dentro de blocos específicos de SO
 * (`__rtems__`/`__XMK__`/`__CYGWIN__`/etc.) que não se aplicam ao alvo
 * genérico `x86_64-elf` deste projeto (mesma categoria de achado de
 * `libc-minimal-port` sobre flags de build do newlib). Sem isto, nenhuma
 * declaração nem macro de `pthread.h` fica visível — definido aqui, não via
 * flag de build, porque só este arquivo e `javaos_pthread_test_program.c`
 * precisam de verdade do conteúdo completo de `pthread.h` nesta change.
 * Seguro: `libc.a` não foi buildado com `--enable-newlib-multithread`
 * (`scripts/cross/build-newlib.sh`), então não define nenhum símbolo
 * `pthread_mutex_*`/`pthread_cond_*` próprio que colidiria com os definidos
 * abaixo. */
#define _POSIX_THREADS 1

#include <pthread.h>
#include <stddef.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "include/javaos_vm.h"

/* `newlib/libc/reent/*.c` (ex. readr.c) declara `errno` como um `extern
 * int` simples, nao via a macro reentrante de <errno.h> - precisamos
 * atualizar exatamente esse simbolo simples, nao `*__errno()`, para que o
 * valor seja visto por quem nos chama. Mesmo padrao que o proprio newlib
 * usa internamente nesses arquivos. */
#include <errno.h>
#undef errno
extern int errno;

/* Números de syscall - mesma convenção de kernel::syscall (número em RAX,
 * argumentos em RDI/RSI/RDX/R10, retorno em RAX). Mantidos em sincronia
 * manualmente com kernel/src/syscall.rs (sem cabeçalho compartilhado entre
 * Rust e C nesta change - ver tasks.md/design.md, Non-Goals). */
#define SYS_WRITE 0
#define SYS_VM_RESERVE 2
#define SYS_VM_COMMIT 3
#define SYS_VM_DECOMMIT 4
#define SYS_VM_PROTECT 5
#define SYS_VM_RELEASE 6
#define SYS_MUTEX_CREATE 7
#define SYS_MUTEX_LOCK 8
#define SYS_MUTEX_UNLOCK 9
#define SYS_COND_CREATE 10
#define SYS_COND_WAIT 11
#define SYS_COND_NOTIFY_ONE 12
#define SYS_COND_NOTIFY_ALL 13
#define SYS_OPEN 18
#define SYS_READ 19
#define SYS_CLOSE 20
#define SYS_EXIT 21
#define SYS_MUTEX_TRYLOCK 22

/* SYSCALL_ERROR do kernel é `u64::MAX`, que como `long` assinado de 64 bits
 * é exatamente -1. */
#define SYSCALL_ERROR (-1L)

static inline long javaos_syscall(long num, long a0, long a1, long a2, long a3) {
    long ret;
    register long r10 __asm__("r10") = a3;
    __asm__ volatile("syscall"
                      : "=a"(ret)
                      : "a"(num), "D"(a0), "S"(a1), "d"(a2), "r"(r10)
                      : "rcx", "r11", "memory");
    return ret;
}

/* Descritores 0/1/2 são reservados para stdin/stdout/stderr (convenção
 * POSIX que o newlib já assume) — nenhum deles corresponde a um handle real
 * de `kernel::file_handles` (que também começa a numerar em 0). Um
 * descritor de arquivo de verdade, devolvido por `open`, é o `handle` do
 * kernel deslocado por este offset, para nunca colidir com 0/1/2. */
#define FD_OFFSET 3

int read(int fd, void *buf, size_t cnt) {
    if (fd < FD_OFFSET) {
        errno = EBADF;
        return -1;
    }
    long r = javaos_syscall(SYS_READ, fd - FD_OFFSET, (long)buf, (long)cnt, 0);
    if (r == SYSCALL_ERROR) {
        errno = EBADF;
        return -1;
    }
    return (int)r;
}

int write(int fd, const void *buf, size_t cnt) {
    if (fd == 1 || fd == 2) {
        return (int)javaos_syscall(SYS_WRITE, (long)buf, (long)cnt, 0, 0);
    }
    errno = EBADF;
    return -1;
}

int open(const char *path, int flags, int mode) {
    (void)flags;
    (void)mode;
    long h = javaos_syscall(SYS_OPEN, (long)path, (long)strlen(path), 0, 0);
    if (h == SYSCALL_ERROR) {
        errno = ENOENT;
        return -1;
    }
    return (int)(h + FD_OFFSET);
}

int close(int fd) {
    if (fd < FD_OFFSET) {
        return 0; /* fechar stdin/stdout/stderr: sucesso trivial, sem efeito */
    }
    long r = javaos_syscall(SYS_CLOSE, fd - FD_OFFSET, 0, 0, 0);
    if (r == SYSCALL_ERROR) {
        errno = EBADF;
        return -1;
    }
    return 0;
}

off_t lseek(int fd, off_t pos, int whence) {
    (void)fd;
    (void)pos;
    (void)whence;
    /* As syscalls de arquivo atuais (SYSCALL_READ) só avançam o cursor por
     * leitura sequencial — sem busca por posição arbitrária. Ver spec
     * `kernel-filesystem`. */
    errno = ESPIPE;
    return (off_t)-1;
}

/* Janela de heap fixa reservada uma única vez, comprometida sob demanda —
 * ver design.md, "_sbrk sobre SYSCALL_VM_RESERVE/SYSCALL_VM_COMMIT já
 * existentes". */
#define HEAP_WINDOW_SIZE (16UL * 1024 * 1024)
#define PAGE_SIZE 4096UL

static long heap_base = 0;
static long heap_committed_end = 0;
static long heap_cur = 0;

void *sbrk(ptrdiff_t incr) {
    if (heap_base == 0) {
        long base = javaos_syscall(SYS_VM_RESERVE, (long)HEAP_WINDOW_SIZE, 0, 0, 0);
        if (base == SYSCALL_ERROR) {
            errno = ENOMEM;
            return (void *)-1;
        }
        heap_base = base;
        heap_cur = base;
        heap_committed_end = base;
    }

    long new_cur = heap_cur + (long)incr;
    if (new_cur < heap_base || new_cur > heap_base + (long)HEAP_WINDOW_SIZE) {
        errno = ENOMEM;
        return (void *)-1;
    }

    if (new_cur > heap_committed_end) {
        long commit_to = (new_cur + (long)PAGE_SIZE - 1) & ~((long)PAGE_SIZE - 1);
        long commit_len = commit_to - heap_committed_end;
        long prot = 1; /* bit0 = gravavel, ver kernel::syscall::prot_to_page_flags */
        long r = javaos_syscall(SYS_VM_COMMIT, heap_committed_end, commit_len, prot, 0);
        if (r == SYSCALL_ERROR) {
            errno = ENOMEM;
            return (void *)-1;
        }
        heap_committed_end = commit_to;
    }

    void *result = (void *)heap_cur;
    heap_cur = new_cur;
    return result;
}

int fstat(int fd, struct stat *st) {
    (void)fd;
    *st = (struct stat){0};
    st->st_mode = S_IFCHR; /* mesma convenção mínima de outros ports: trata todo descritor valido como dispositivo de caractere */
    return 0;
}

int isatty(int fd) {
    return (fd == 0 || fd == 1 || fd == 2) ? 1 : 0;
}

void _exit(int status) {
    (void)status;
    javaos_syscall(SYS_EXIT, 0, 0, 0, 0);
    for (;;) {
    } /* nunca alcançado: SYSCALL_EXIT nunca retorna */
}

int kill(int pid, int sig) {
    (void)pid;
    (void)sig;
    errno = EINVAL; /* sem sinais reais ainda */
    return -1;
}

int getpid(void) {
    return 1; /* sem PIDs reais ainda — único "processo" do ponto de vista desta libc */
}

/* Implementação das cinco operações de VM declaradas em include/javaos_vm.h
 * — ver change os-javaos-memory-pd, design.md. `prot` já chega na mesma
 * convenção que kernel::syscall::prot_to_page_flags espera (bit 0 =
 * gravável, bit 1 = executável), repassado sem tradução. */

void *javaos_vm_reserve(size_t bytes) {
    long base = javaos_syscall(SYS_VM_RESERVE, (long)bytes, 0, 0, 0);
    if (base == SYSCALL_ERROR) {
        return NULL;
    }
    return (void *)base;
}

int javaos_vm_commit(void *addr, size_t bytes, int prot) {
    long r = javaos_syscall(SYS_VM_COMMIT, (long)addr, (long)bytes, prot, 0);
    return (r == SYSCALL_ERROR) ? -1 : 0;
}

int javaos_vm_decommit(void *addr, size_t bytes) {
    long r = javaos_syscall(SYS_VM_DECOMMIT, (long)addr, (long)bytes, 0, 0);
    return (r == SYSCALL_ERROR) ? -1 : 0;
}

int javaos_vm_protect(void *addr, size_t bytes, int prot) {
    long r = javaos_syscall(SYS_VM_PROTECT, (long)addr, (long)bytes, prot, 0);
    return (r == SYSCALL_ERROR) ? -1 : 0;
}

int javaos_vm_release(void *addr) {
    long r = javaos_syscall(SYS_VM_RELEASE, (long)addr, 0, 0, 0);
    return (r == SYSCALL_ERROR) ? -1 : 0;
}

/* pthread_mutex_* e pthread_cond_* reais sobre as syscalls de mutex/condvar
 * do kernel - ver change os-javaos-platform-sync, design.md. Convencao POSIX:
 * devolvem 0 em sucesso, ou um numero de erro positivo (nunca -1/errno, ao
 * contrario das funcoes POSIX "tipo syscall" acima). `pthread_mutex_t`/
 * `pthread_cond_t` ja sao so `uint32_t` opacos no newlib vendorizado -
 * guardam o handle do kernel diretamente, sem indirecao.
 *
 * Inicializacao preguicosa, deliberadamente sem protecao contra corrida:
 * PTHREAD_MUTEX_INITIALIZER/PTHREAD_COND_INITIALIZER (0xFFFFFFFF) sao
 * resolvidos para um handle real na primeira chamada. Seguro hoje porque
 * nenhum processo de usuario pode ter mais de uma thread (nenhuma syscall de
 * criacao de thread existe ainda) - uma mudanca futura que adicione uma
 * precisa revisar isto antes de ser considerada completa (ver design.md,
 * Risks/Trade-offs). */

static int pthread_mutex_resolve(pthread_mutex_t *mutex) {
    if (*mutex == (pthread_mutex_t)PTHREAD_MUTEX_INITIALIZER) {
        long h = javaos_syscall(SYS_MUTEX_CREATE, 0, 0, 0, 0);
        if (h == SYSCALL_ERROR) {
            return EAGAIN;
        }
        *mutex = (pthread_mutex_t)h;
    }
    return 0;
}

int pthread_mutex_init(pthread_mutex_t *mutex, const pthread_mutexattr_t *attr) {
    (void)attr; /* sem suporte a atributos (recursivo, compartilhado entre processos, etc.) */
    long h = javaos_syscall(SYS_MUTEX_CREATE, 0, 0, 0, 0);
    if (h == SYSCALL_ERROR) {
        return EAGAIN;
    }
    *mutex = (pthread_mutex_t)h;
    return 0;
}

int pthread_mutex_destroy(pthread_mutex_t *mutex) {
    /* Sucesso sem liberar o objeto no kernel - nao existe
     * SYSCALL_MUTEX_DESTROY, e a tabela de handles do kernel ja nao suporta
     * remocao (decisao anterior a esta change) - ver design.md. */
    (void)mutex;
    return 0;
}

int pthread_mutex_lock(pthread_mutex_t *mutex) {
    int err = pthread_mutex_resolve(mutex);
    if (err != 0) {
        return err;
    }
    long r = javaos_syscall(SYS_MUTEX_LOCK, (long)*mutex, 0, 0, 0);
    return (r == SYSCALL_ERROR) ? EINVAL : 0;
}

int pthread_mutex_trylock(pthread_mutex_t *mutex) {
    int err = pthread_mutex_resolve(mutex);
    if (err != 0) {
        return err;
    }
    long r = javaos_syscall(SYS_MUTEX_TRYLOCK, (long)*mutex, 0, 0, 0);
    return (r == SYSCALL_ERROR) ? EBUSY : 0;
}

int pthread_mutex_unlock(pthread_mutex_t *mutex) {
    long r = javaos_syscall(SYS_MUTEX_UNLOCK, (long)*mutex, 0, 0, 0);
    return (r == SYSCALL_ERROR) ? EINVAL : 0;
}

static int pthread_cond_resolve(pthread_cond_t *cond) {
    if (*cond == (pthread_cond_t)PTHREAD_COND_INITIALIZER) {
        long h = javaos_syscall(SYS_COND_CREATE, 0, 0, 0, 0);
        if (h == SYSCALL_ERROR) {
            return EAGAIN;
        }
        *cond = (pthread_cond_t)h;
    }
    return 0;
}

int pthread_cond_init(pthread_cond_t *cond, const pthread_condattr_t *attr) {
    (void)attr; /* sem suporte a atributos (clock, compartilhado entre processos) */
    long h = javaos_syscall(SYS_COND_CREATE, 0, 0, 0, 0);
    if (h == SYSCALL_ERROR) {
        return EAGAIN;
    }
    *cond = (pthread_cond_t)h;
    return 0;
}

int pthread_cond_destroy(pthread_cond_t *cond) {
    /* Mesma decisao de pthread_mutex_destroy - ver design.md. */
    (void)cond;
    return 0;
}

int pthread_cond_signal(pthread_cond_t *cond) {
    int err = pthread_cond_resolve(cond);
    if (err != 0) {
        return err;
    }
    long r = javaos_syscall(SYS_COND_NOTIFY_ONE, (long)*cond, 0, 0, 0);
    return (r == SYSCALL_ERROR) ? EINVAL : 0;
}

int pthread_cond_broadcast(pthread_cond_t *cond) {
    int err = pthread_cond_resolve(cond);
    if (err != 0) {
        return err;
    }
    long r = javaos_syscall(SYS_COND_NOTIFY_ALL, (long)*cond, 0, 0, 0);
    return (r == SYSCALL_ERROR) ? EINVAL : 0;
}

int pthread_cond_wait(pthread_cond_t *cond, pthread_mutex_t *mutex) {
    int err = pthread_cond_resolve(cond);
    if (err != 0) {
        return err;
    }
    err = pthread_mutex_resolve(mutex);
    if (err != 0) {
        return err;
    }
    long r = javaos_syscall(SYS_COND_WAIT, (long)*cond, (long)*mutex, 0, 0);
    return (r == SYSCALL_ERROR) ? EINVAL : 0;
}
