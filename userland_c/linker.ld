/* Mesmo layout de elf_test_program/linker.ld (change kernel-elf-loader):
 * endereco de carga fixo (nao-PIE) em 0x400000, e alinhamento de pagina
 * explicito antes de qualquer secao gravavel — sem isso, o linker pode
 * empacotar .bss na mesma pagina de .text quando o conteudo e pequeno, e o
 * carregador (`kernel::elf`) rejeita dois segmentos PT_LOAD com flags
 * diferentes que compartilham uma pagina.
 */
ENTRY(_start)

SECTIONS
{
    . = 0x400000;

    .text : {
        *(.text*)
    }

    . = ALIGN(0x1000);

    .rodata : {
        *(.rodata*)
    }

    .data : {
        *(.data*)
    }

    .bss : {
        *(.bss*)
        *(COMMON)
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame*)
        *(.note*)
    }
}
