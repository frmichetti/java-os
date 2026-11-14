/* Programa C de verificacao do isolamento de falha por processo (change
 * kernel-fault-isolation, design.md). ELF real, compilado pelo wrapper de
 * cross-compilador e carregado via kernel::elf/spawn_from_elf (mesmo
 * mecanismo ja usado para os demais programas de teste).
 *
 * Ao contrario de todo programa de teste anterior (que sempre reporta P/F
 * ao final via write()), este programa reporta que chegou ate aqui e entao
 * causa deliberadamente um page fault em modo usuario (escrita num endereco
 * nunca mapeado). Nenhuma instrucao depois dessa escrita deve executar -
 * a prova de que o isolamento funcionou e justamente NAO ver nenhuma
 * mensagem de sucesso depois desta, e ver o boot continuar normalmente com
 * os programas seguintes.
 */

#include <unistd.h>

int main(void) {
    const char msg[] = "fault_test_program: chegou ate aqui, vou causar um page fault de proposito\n";
    write(1, msg, sizeof(msg) - 1);

    volatile int *bad = (volatile int *)0;
    *bad = 1;

    /* Nunca deve chegar aqui - se chegar, o isolamento falhou. */
    const char unreachable[] = "fault_test_program: ISTO NUNCA DEVERIA APARECER\n";
    write(1, unreachable, sizeof(unreachable) - 1);

    return 0;
}
