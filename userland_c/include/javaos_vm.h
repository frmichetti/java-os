/* API minima em C sobre as cinco syscalls de memoria virtual anonima do
 * kernel (SYSCALL_VM_RESERVE/COMMIT/DECOMMIT/PROTECT/RELEASE) - ver change
 * os-javaos-memory-pd, design.md, "Uma API em C dedicada (javaos_vm.h)".
 *
 * Existe para dois chamadores: os corpos pd_* de
 * vendor/jdk-27-ga/src/hotspot/os/javaos/os_javaos.cpp (que so precisam da
 * declaracao - sao verificados por g++ -fsyntax-only contra os headers reais
 * do HotSpot, nunca linkados a esta implementacao nesta change) e
 * userland_c/javaos_vm_test_program.c (que linka de verdade contra a
 * implementacao em glue.c e verifica o comportamento em boot real).
 *
 * `prot` usa a mesma convencao de kernel::syscall::prot_to_page_flags: bit 0
 * = gravavel, bit 1 = executavel.
 */

#ifndef JAVAOS_VM_H
#define JAVAOS_VM_H

#include <stddef.h>

#define JAVAOS_VM_PROT_WRITE 0x1
#define JAVAOS_VM_PROT_EXEC 0x2

#ifdef __cplusplus
extern "C" {
#endif

/* Reserva `bytes` de memoria virtual anonima. Devolve o endereco base da
 * reserva, ou NULL se o kernel rejeitar o pedido (sem espaco virtual livre
 * suficiente). */
void *javaos_vm_reserve(size_t bytes);

/* Compromete `bytes` a partir de `addr` (deve cair dentro de uma reserva
 * viva) com a mascara de permissoes `prot`. Devolve 0 em sucesso, -1 em
 * falha (addr fora de qualquer reserva viva, entre outras condicoes). */
int javaos_vm_commit(void *addr, size_t bytes, int prot);

/* Descompromete `bytes` a partir de `addr`. Devolve 0 em sucesso, -1 em
 * falha. */
int javaos_vm_decommit(void *addr, size_t bytes);

/* Altera as permissoes de `bytes` a partir de `addr` para a mascara `prot`.
 * Devolve 0 em sucesso, -1 em falha. */
int javaos_vm_protect(void *addr, size_t bytes, int prot);

/* Libera por completo a reserva cuja base e exatamente `addr`. Devolve 0 em
 * sucesso, -1 em falha (ex.: addr nao e a base exata de nenhuma reserva
 * viva). */
int javaos_vm_release(void *addr);

#ifdef __cplusplus
}
#endif

#endif /* JAVAOS_VM_H */
