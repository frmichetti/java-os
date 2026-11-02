OUTPUT_FORMAT(elf64-x86-64)

ENTRY(kernel_main)

PHDRS
{
    text    PT_LOAD    FLAGS((1 << 0) | (1 << 2)) ;
    rodata  PT_LOAD    FLAGS((1 << 2)) ;
    data    PT_LOAD    FLAGS((1 << 1) | (1 << 2)) ;
}

SECTIONS
{
    /* Higher-half kernel: topo dos 2 GiB superiores, exigido pelo code-model=kernel. */
    . = 0xffffffff80000000;

    .text : {
        *(.text .text.*)
    } :text

    . += CONSTANT(MAXPAGESIZE);

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    .requests_start_marker : {
        KEEP(*(.requests_start_marker))
    } :rodata
    .requests : {
        KEEP(*(.requests))
    } :rodata
    .requests_end_marker : {
        KEEP(*(.requests_end_marker))
    } :rodata

    . += CONSTANT(MAXPAGESIZE);

    .data : {
        *(.data .data.*)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
        *(.comment)
    }
}
