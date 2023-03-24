
ENTRY(start)
OUTPUT_ARCH(i386:x86-64)
OUTPUT_FORMAT(elf64-x86-64)

KERNEL_OFFSET = 0xffff800000000000;

SECTIONS {
    . = 1M;
    
	.init ALIGN(4K) :
	{
		KEEP(*(.multiboot_header))
		KEEP(*(.init .init.*))
	}

	. += KERNEL_OFFSET;
	
	.text ALIGN(4K) : AT(ADDR(.text) - KERNEL_OFFSET)
	{
		/* KEEP(*(.pre_init_array)) */
		*(.text .text.*)
	}

	.rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_OFFSET) {
        __rodata_start = .;
        *(.rodata*)
        . = ALIGN(4096);
        __rodata_end = .;
    }

	.data ALIGN(4K) : AT(ADDR(.data) - KERNEL_OFFSET) {
        __data_start = .;
        *(.data*)
        . = ALIGN(4096);
        __data_end = .;
        __bss_start = .;
        *(.bss*)
        . = ALIGN(4096);
        __bss_end = .;
    }

    .page_table ALIGN(4K) : AT(ADDR(.page_table) - KERNEL_OFFSET) {
        
        /* . = ALIGN(4096); */
        p4_table = . - KERNEL_OFFSET;
        . += 0x1000;
        p3_table = . - KERNEL_OFFSET;
        . += 0x1000;
        p2_table = . - KERNEL_OFFSET;
        . += 0x1000;
    }

    .stack ALIGN(4K) : AT(ADDR(.stack) - KERNEL_OFFSET) {
        . = ALIGN(2M);
        . += 2M;
        initial_bsp_stack_guard_page = . - KERNEL_OFFSET;
        . += 0x1000;
        initial_bsp_stack_bottom = . - KERNEL_OFFSET;
        . += 0x20000;
        kernel_stack_top = . - KERNEL_OFFSET;
    }

    .got ALIGN(4K) : AT(ADDR(.got) - KERNEL_OFFSET) {
        *(.got*)
    }

	.tdata ALIGN(4K) : AT(ADDR(.tdata) - KERNEL_OFFSET) {
        __tdata_start = .;
        *(.tdata*)
        . = ALIGN(4096);
        __tdata_end = .;
        __tbss_start = .;
        *(.tbss*)
        . += 8;
        . = ALIGN(4096);
        __tbss_end = .;
    }
    
	

	/DISCARD/ :{
		*(.comment*)
        *(.eh_frame*)
        *(.gcc_except_table*)
        *(.note*)
        *(.rel.eh_frame*)
	}
}