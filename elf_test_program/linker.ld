/* Endereco de carga fixo (nao-PIE) do executavel ELF64 de teste - ver
 * design.md da change `kernel-elf-loader`, "Programa de teste: binario ELF
 * real, fora da arvore do kernel". Qualquer endereco na metade inferior
 * (nao-canonica para o kernel) serve: o carregador (`kernel::elf`) mapeia
 * cada segmento PT_LOAD no `p_vaddr` que o proprio executavel declarar, sem
 * exigir nenhum endereco especifico.
 */
ENTRY(_start)

SECTIONS
{
    . = 0x400000;

    .text : {
        *(.text*)
    }

    /* Alinhamento explicito de pagina antes de qualquer secao gravavel:
     * sem isso, o linker empacota `.bss` na mesma pagina de `.text` quando o
     * conteudo e pequeno, e o carregador (`kernel::elf`) rejeita dois
     * segmentos PT_LOAD com flags diferentes que compartilham uma pagina
     * como sobreposicao (ver design.md da change `kernel-elf-loader`,
     * "Achados durante a implementacao"). Mesmo problema que motiva linkers
     * de proposito geral a sempre separarem segmentos de permissao
     * diferente por pagina inteira.
     */
    . = ALIGN(0x1000);

    .rodata : {
        *(.rodata*)
    }

    .data : {
        *(.data*)
    }

    .bss : {
        *(.bss*)
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame*)
        *(.note*)
    }
}
