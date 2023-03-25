.global start
.global long_mode_start

.extern kernel_main

.section .multiboot_header
.align 8
header_start:
    .long 0xe85250d6
    .long 0
    .long header_end - header_start
    .long -(0xe85250d6 + 0 + (header_end - header_start))

info_request:
    .short 1
    .short 0
    .long info_request_end - info_request
    .long 6
info_request_end:

.align 8
    .short 0
    .short 0
    .long 8
header_end:

.section .init.text32, "ax", @progbits
.code32
start:
    
    mov esp, offset kernel_stack_top
    
    push 0
    push eax
    push 0
    push ebx

    call check_multiboot
    call check_cpuid
    call check_long_mode

    mov eax, 24
    push eax
    lea edx, [protected_mode]
    push edx
    lgdt [gdt_pointer]

    retf

    mov dword ptr [0xb8000], 0x2f4b2f4f
    hlt


check_multiboot:
    cmp eax, 0x36d76289
    jne .no_multiboot
    ret
.no_multiboot:
    mov al, 0x30
    jmp error

check_cpuid:
    pushfd
    pop eax
    mov ecx, eax
    xor eax, 0x200000
    push eax
    popfd
    pushfd
    pop eax
    push ecx
    popfd
    cmp eax, ecx
    je .no_cpuid
    ret
.no_cpuid:
    mov al, 0x31
    jmp error

check_long_mode:
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb .no_long_mode
    mov eax, 0x80000001
    cpuid
    test edx, 0x20000000
    je .no_long_mode
    ret
.no_long_mode:
    mov al, 0x32
    jmp error

protected_mode:
    mov ax, 16
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

set_up_page_tables:
    //; map the first p4 entry to the p3 table
    lea edi, [p4_table]
    lea eax, [p3_table + 0x3]
    mov dword ptr [edi], eax
    mov dword ptr [edi + 4], 0

    //; gotta do it for high addresses (p4_table[256+]) too!
    lea edi, [p4_table + 256 * 8]
    lea eax, [p3_table + 0x3]
    mov dword ptr [edi], eax
    mov dword ptr [edi + 4], 0

    lea edi, [p3_table]
    lea eax, [p2_table + 0x3]
    mov ecx, 8

write_p3_entry:
    mov dword ptr [edi], eax
    mov dword ptr [edi + 4], 0
    add eax, 0x1000
    add edi, 8
    loop write_p3_entry

    lea edi, [p2_table]
    mov eax, 0x0000083
    mov ecx, 8 * 512

write_p2_entry:
    mov dword ptr [edi], eax
    mov dword ptr [edi + 4], 0
    add eax, 0x200000
    add edi, 8
    loop write_p2_entry

    jmp enable_long_mode

enable_long_mode:
    mov eax, cr4
    or eax, (1 << 7) | (1 << 5) | (1 << 1)
    mov cr4, eax

    lea eax, [p4_table]
    mov cr3, eax

    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 11) | (1 << 8)
    wrmsr

    mov eax, 8
    push eax
    lea edx, [long_mode_start]
    push edx

    mov eax, cr0
    //; and ax, 0xfffb
    or eax, (1 << 31) | (1 << 16)
    mov cr0, eax

    retf

error:
    mov dword ptr [0xb8000], 0x4f524f45
    mov dword ptr [0xb8004], 0x4f3a4f52
    mov dword ptr [0xb8008], 0x4f204f20
    mov byte  ptr [0xb800a], al
    hlt


.align 16
gdt_pointer:
    .word gdt_end - gdt - 1
    .quad gdt

.align 16
gdt:
    // 0: null
    .quad 0

    // 8: 64-bit code
    .quad 0x00af9a000000ffff

    // 16: 64-bit data
    .quad 0x00cf92000000ffff

    // 24: 32-bit code
    .quad 0x00cf9a000000ffff
gdt_end:

//; .section .init.text64
.code64
long_mode_start:
    mov ax, 16
    mov ds, ax
    mov ss, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    mov rbx, 0xffff800000000000
    or rsp, rbx
    lea rax, [start_high - 0xffff800000000000]
    or rax, rbx
    
    jmp rax
    

.code64
.text
start_high:
    pop rsi
    pop rdi

    xor rbp, rbp
    call kernel_main

    mov rax, 0x2f592f412f4b2f4f
    mov qword ptr [0xb8000], rax

halt:
    cli
    hlt
    jmp halt
