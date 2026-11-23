/* Memory of the Arm MPS2 board with the Cortex-M7 image (AN500), as QEMU
   models it: 4 MB for code and 4 MB of RAM. One test frame sits beside the
   code, the other is loaded into RAM, where a camera would deliver it. */
MEMORY
{
    CODE   (rx)  : ORIGIN = 0x00000000, LENGTH = 4M
    RAM    (rw)  : ORIGIN = 0x20000000, LENGTH = 4M
}

ENTRY(Reset)
__stack_top = ORIGIN(RAM) + LENGTH(RAM);
__stack_limit = __stack_top - 64K;

SECTIONS
{
    .vectors : { KEEP(*(.vectors)) } > CODE
    .text : { *(.text .text.*) } > CODE
    .rodata : { *(.rodata .rodata.*) } > CODE
    .frames : { KEEP(*(.frames .frames.*)) } > CODE
    .ramframes : { KEEP(*(.ramframes .ramframes.*)) } > RAM
    .data : {
        __data_start = .;
        *(.data .data.*)
        . = ALIGN(4);
        __data_end = .;
    } > RAM AT > CODE
    __data_load = LOADADDR(.data);
    .bss (NOLOAD) : {
        __bss_start = .;
        *(.bss .bss.* COMMON)
        . = ALIGN(4);
        __bss_end = .;
    } > RAM
    ASSERT(__bss_end <= __stack_limit, "data and heap run into the stack")
    /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab*) }
}
