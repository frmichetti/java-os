/* Rotina de inicializacao minima de um programa C de usuario sobre o Java
 * OS - ver change libc-minimal-port, design.md, tarefa 3.7. A stack de
 * usuario ja vem mapeada e com RSP valido antes do ponto de entrada (mesma
 * convencao de elf_test_program/kernel::scheduler::spawn_from_elf - ver
 * kernel-elf-loader): nenhuma configuracao extra de stack e necessaria
 * aqui. Sem argc/argv (nenhum carregador de argumentos de linha de comando
 * existe ainda) - main e chamada sem argumentos.
 */
.global _start
.extern main
.extern _exit

.text
_start:
    xor %ebp, %ebp      /* fim da cadeia de frame pointers (convenção comum) */
    call main
    mov %eax, %edi      /* valor de retorno de main -> argumento de _exit */
    call _exit
1:
    jmp 1b              /* nunca alcançado: _exit (SYSCALL_EXIT) nunca retorna */
