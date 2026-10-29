ENTRY(start)

SECTIONS {
    . = 1M;

    .multiboot ALIGN(8) : {
        KEEP(*(.multiboot))
    }

    .boot ALIGN(4K) : {
        *(.boot)
    }

    .text ALIGN(4K) : {
        *(.text .text.*)
    }

    .rodata ALIGN(4K) : {
        *(.rodata .rodata.*)
    }

    .data ALIGN(4K) : {
        *(.data .data.*)
    }

    .bss ALIGN(4K) : {
        *(COMMON)
        *(.bss .bss.*)
    }

    .program_buf ALIGN(4K) : {
        KEEP(*(.program_buf))
    }

    /* Запас под RSP ядра */
    . = ALIGN(16);
    . += 0x40000;
    _kernel_end = .;

    /* Символ для boot.asm */
    _stack_top = _kernel_end;
}
