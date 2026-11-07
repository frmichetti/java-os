/* Programa C de verificacao de libm e das conversoes string->ponto flutuante
 * (change libc-float-math, design.md). ELF real, compilado pelo wrapper de
 * cross-compilador (agora com SSE habilitado) e carregado via
 * kernel::elf/spawn_from_elf (mesmo mecanismo ja usado para
 * /bin/libctest.elf/cxxtest.elf/javaosvm.elf/jpthread.elf).
 *
 * Exercita, nesta ordem: uma expressao aritmetica com double, sqrt(), uma
 * funcao trigonometrica (sin()), e strtod()/atof() - exatamente o que a
 * capacidade libc-float-math promete. Compara cada resultado com uma
 * tolerancia pequena, sem usar printf("%f", ...) - formatacao de ponto
 * flutuante em texto continua sem suporte (ver design.md, Non-Goals) -
 * reportando so P/F por write(), mesmo padrao dos programas de teste
 * anteriores.
 */

#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define EPSILON 0.0001

static int close_enough(double a, double b) {
    double diff = a - b;
    if (diff < 0.0) {
        diff = -diff;
    }
    return diff < EPSILON;
}

int main(void) {
    char result[5];
    memset(result, '?', sizeof(result));

    /* 1) aritmetica double simples. */
    double sum = 2.5 + 1.5;
    result[0] = close_enough(sum, 4.0) ? 'P' : 'F';

    /* 2) sqrt de um quadrado perfeito. */
    result[1] = close_enough(sqrt(16.0), 4.0) ? 'P' : 'F';

    /* 3) sin em um ponto conhecido. */
    result[2] = close_enough(sin(0.0), 0.0) ? 'P' : 'F';

    /* 4) strtod de uma string numerica simples. */
    double parsed = strtod("3.5", NULL);
    result[3] = close_enough(parsed, 3.5) ? 'P' : 'F';

    /* 5) atof da mesma string. */
    result[4] = close_enough(atof("3.5"), 3.5) ? 'P' : 'F';

    char msg[128];
    int len = 0;
    const char prefix[] = "libm_test_program: resultado ";
    memcpy(msg, prefix, sizeof(prefix) - 1);
    len = (int)(sizeof(prefix) - 1);
    memcpy(msg + len, result, sizeof(result));
    len += (int)sizeof(result);
    const char suffix[] = " (ordem: double_arith,sqrt,sin,strtod,atof)\n";
    memcpy(msg + len, suffix, sizeof(suffix) - 1);
    len += (int)(sizeof(suffix) - 1);
    write(1, msg, (size_t)len);

    return 0;
}
