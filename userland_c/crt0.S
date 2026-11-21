/* Rotina de inicializacao minima de um programa C de usuario sobre o Java
 * OS - ver change libc-minimal-port, design.md, tarefa 3.7. A stack de
 * usuario ja vem mapeada e com RSP valido antes do ponto de entrada (mesma
 * convencao de elf_test_program/kernel::scheduler::spawn_from_elf - ver
 * kernel-elf-loader): nenhuma configuracao extra de stack e necessaria
 * aqui. Sem argc/argv (nenhum carregador de argumentos de linha de comando
 * existe ainda) - main e chamada sem argumentos.
 *
 * Achado real (change libc-float-math): kernel::process::create_user_thread
 * entrega o RSP inicial de _start ja ajustado para a convencao de "funcao
 * chamada" da ABI System V (RSP % 16 == 8, mesma convencao usada por todo
 * entry point de demo em assembly deste projeto, que SAO efetivamente
 * funcoes). _start, porem, NAO e uma funcao chamada - e o ponto de entrada
 * do processo (equivalente a e_entry do ELF), que a ABI exige comecar com
 * RSP alinhado a 16 bytes (sem offset), ja que nenhum "call" implicito
 * empurrou um endereco de retorno antes dele. Com o RSP herdado (% 16 ==
 * 8), o "call main" abaixo deixava "main" com RSP alinhado a 16 em vez de
 * "% 16 == 8" como a ABI exige na entrada de qualquer funcao chamada -
 * invisivel enquanto nenhum codigo gerado pelo compilador usasse
 * instrucoes SSE alinhadas (ex. movaps), e por isso nunca percebido antes
 * desta change (a primeira a habilitar SSE no toolchain C/C++) - a
 * primeira delas gerada por uma funcao real (libm_test_program.c) causava
 * #GP imediato. Corrigido alinhando explicitamente aqui, mesma convencao
 * de crt0 real (glibc, musl): funciona independente do RSP exato entregue
 * pelo kernel.
 *
 * Achado real (change libc-retargetable-locking): __javaos_init_internal_locks
 * (userland_c/glue.c) precisa rodar antes de "call main" - ponto em que o
 * processo garantidamente so tem uma thread (nenhuma SYSCALL_THREAD_CREATE
 * pode ter acontecido ainda), unica forma de inicializar os 8 mutexes
 * internos do newlib sem precisar de uma secao critica para proteger a
 * propria criacao do mutex - ver design.md daquela change.
 */
.global _start
.extern main
.extern _exit
.extern __javaos_init_internal_locks

.text
_start:
    xor %ebp, %ebp      /* fim da cadeia de frame pointers (convenção comum) */
    and $-16, %rsp      /* garante RSP alinhado a 16 bytes antes do "call" abaixo */
    call __javaos_init_internal_locks
    call main
    mov %eax, %edi      /* valor de retorno de main -> argumento de _exit */
    call _exit
1:
    jmp 1b              /* nunca alcançado: _exit (SYSCALL_EXIT) nunca retorna */
