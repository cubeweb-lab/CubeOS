ENTRY(_start)

SECTIONS {
    . = 0x400000;

    .text ALIGN(4K) : { *(.text .text.*) }
    .rodata ALIGN(4K) : { *(.rodata .rodata.*) }
    .drvmeta ALIGN(4) : { KEEP(*(.drvmeta)) }
    .data ALIGN(4K) : { *(.data .data.*) }
    .bss ALIGN(4K) : { *(COMMON) *(.bss .bss.*) }
}
