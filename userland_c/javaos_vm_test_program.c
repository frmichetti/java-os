/* Programa C de verificacao da API javaos_vm.h/glue.c (change
 * os-javaos-memory-pd, design.md, "Verificacao em boot real via um programa
 * C real"). Nao Rust, nao embutido no kernel: um arquivo ELF real, compilado
 * pelo wrapper de cross-compilador e carregado via kernel::elf/spawn_from_elf
 * (mesmo mecanismo ja usado para /bin/libctest.elf/cxxtest.elf).
 *
 * Exercita, nesta ordem, a mesma sequencia de chamadas que os corpos reais
 * de pd_* em os_javaos.cpp (vendor/jdk-27-ga/.../os_javaos.cpp) passam a
 * fazer: reserva, compromete, escreve/rele, protege como somente leitura,
 * descompromete, libera, e por fim duas falhas esperadas: liberar um
 * endereco no meio (nao na base) de uma reserva viva, e comprometer um
 * endereco fora de qualquer reserva.
 *
 * Deliberadamente NAO escreve na pagina depois de protege-la como somente
 * leitura: isso provocaria um #PF real em modo usuario que, hoje, derruba o
 * kernel inteiro (nenhum isolamento de falha por processo ainda existe — ver
 * ROADMAP.md). Mesma cautela ja adotada por kernel::user_posix_demo
 * (kernel-jvm-posix-syscalls), que tambem nunca escreve apos proteger como
 * somente leitura. O comportamento de permissao em si (escrita apos
 * protect(somente leitura) falha) ja e responsabilidade de
 * kernel-virtual-memory, verificado la; esta change so confirma que a API em
 * C chega ate a mesma syscall e recebe sucesso.
 *
 * Mesmo padrao de relato de 'P'/'F' por caractere dos demos em assembly do
 * kernel e dos outros programas C de teste (libc_test_program.c) — so
 * write() de uma unica mensagem com o resultado.
 */

#include <string.h>
#include <unistd.h>

#include "include/javaos_vm.h"

#define PAGE_SIZE 0x1000UL

int main(void) {
    char result[8];
    memset(result, '?', sizeof(result));

    /* 1) reservar duas paginas (a segunda serve so para o teste de
     * liberacao fora da base, item 7). */
    result[0] = 'F';
    void *addr = javaos_vm_reserve(2 * PAGE_SIZE);
    if (addr != NULL) {
        result[0] = 'P';
    }

    /* 2) comprometer a primeira pagina, gravavel. */
    result[1] = 'F';
    if (addr != NULL && javaos_vm_commit(addr, PAGE_SIZE, JAVAOS_VM_PROT_WRITE) == 0) {
        result[1] = 'P';
    }

    /* 3) escrever um padrao conhecido e confirmar a leitura. */
    result[2] = 'F';
    if (result[1] == 'P') {
        volatile unsigned long *p = (volatile unsigned long *)addr;
        *p = 0xdeadbeefcafebabeUL;
        if (*p == 0xdeadbeefcafebabeUL) {
            result[2] = 'P';
        }
    }

    /* 4) proteger como somente leitura (sem escrever depois — ver nota no
     * topo do arquivo). */
    result[3] = 'F';
    if (result[2] == 'P' && javaos_vm_protect(addr, PAGE_SIZE, 0) == 0) {
        result[3] = 'P';
    }

    /* 5) descomprometer. */
    result[4] = 'F';
    if (result[3] == 'P' && javaos_vm_decommit(addr, PAGE_SIZE) == 0) {
        result[4] = 'P';
    }

    /* 6) liberar a reserva inteira (base exata). */
    result[5] = 'F';
    if (result[4] == 'P' && javaos_vm_release(addr) == 0) {
        result[5] = 'P';
    }

    /* 7) (nova reserva, ja que a anterior foi liberada acima) liberar um
     * endereco no meio — nao na base — de uma reserva viva deve falhar. */
    result[6] = 'F';
    void *addr2 = javaos_vm_reserve(2 * PAGE_SIZE);
    if (addr2 != NULL) {
        char *mid = (char *)addr2 + PAGE_SIZE;
        if (javaos_vm_release(mid) != 0) {
            result[6] = 'P';
        }
        javaos_vm_release(addr2); /* limpeza; resultado nao afeta o relato */
    }

    /* 8) comprometer um endereco fora de qualquer reserva deve falhar. */
    result[7] = 'F';
    if (javaos_vm_commit((void *)0, PAGE_SIZE, JAVAOS_VM_PROT_WRITE) != 0) {
        result[7] = 'P';
    }

    char msg[160];
    int len = 0;
    const char prefix[] = "javaos_vm_test_program: resultado ";
    memcpy(msg, prefix, sizeof(prefix) - 1);
    len = (int)(sizeof(prefix) - 1);
    memcpy(msg + len, result, sizeof(result));
    len += (int)sizeof(result);
    const char suffix[] =
        " (ordem: reserve,commit,pattern,protect,decommit,release,release_mid_err,commit_err)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += (int)(sizeof(suffix) - 1);
    write(1, msg, (size_t)len);

    return 0;
}
