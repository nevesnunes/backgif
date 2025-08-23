TARGET(binary)
INPUT(./a.out)
OUTPUT_FORMAT(elf64-x86-64)

MEMORY
{
    ram (rwx) : ORIGIN = 0x1000,   LENGTH = 0x100000
    rom (rx)  : ORIGIN = 0x400000, LENGTH = 0x200000
}

SECTIONS
{
    .data              : { ./a.out } > ram
    .note.gnu.build-id : { *(.note.gnu.build-id) } > ram
    .text 0x401000     : { *(.text) } > rom

    /DISCARD/ : { *(.comment) *(.eh_frame*) *(.gnu*) }
}
