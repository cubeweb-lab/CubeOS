; ============================================
; Multiboot2 Header
; ============================================
section .multiboot
align 8
multiboot_header:
    dd 0xE85250D6
    dd 0
    dd multiboot_header_end - multiboot_header
    dd -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header))

    align 8
    dw 5
    dw 1
    dd 24
    dd 800
    dd 600
    dd 32

    align 8
    dw 0
    dw 0
    dd 8
multiboot_header_end:

; ============================================
; Boot Code (32-bit)
; ============================================
section .boot
bits 32
global start
extern rust_main
extern _stack_top

PML4_TABLE_ADDR   equ 0x10000
PDPT_TABLE_ADDR   equ 0x11000
PDT_TABLE_ADDR    equ 0x12000
PDT1_TABLE_ADDR   equ 0x14000
PDT2_TABLE_ADDR   equ 0x15000
PDT3_TABLE_ADDR   equ 0x16000
GDT64_ADDR        equ 0x13000

start:
    cli
    cld

    mov dword [0x500], eax
    mov dword [0x504], ebx

    ; Проверка Long Mode
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb .no_lm

    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29
    jz .no_lm

    call init_paging
    call init_gdt64
    lgdt [gdt64_ptr]

    ; Загружаем TSS (дескриптор уже валиден)
    mov ax, 0x28
    ltr ax

    ; Включаем paging
    mov eax, PML4_TABLE_ADDR
    mov cr3, eax

    mov eax, cr4
    or eax, 0x20      ; PAE
    mov cr4, eax

    ; Включаем Long Mode
    mov ecx, 0xC0000080
    rdmsr
    or eax, 0x100     ; LME
    wrmsr

    mov eax, cr0
    or eax, 0x80000000
    mov cr0, eax

    jmp 0x08:long_mode_start

.no_lm:
    mov dword [0xB8000], 0x4F4C4F4C
    hlt
    jmp $

; ============================================
; Инициализация Paging
; ============================================
init_paging:
    pushad
    mov edi, PML4_TABLE_ADDR
    mov ecx, (4096 * 6) / 4
    xor eax, eax
    rep stosd

    mov eax, PDPT_TABLE_ADDR
    or eax, 0x03
    mov [PML4_TABLE_ADDR], eax

    mov eax, PDT_TABLE_ADDR
    or eax, 0x03
    mov [PDPT_TABLE_ADDR], eax

    mov eax, PDT1_TABLE_ADDR
    or eax, 0x03
    mov [PDPT_TABLE_ADDR + 8], eax

    mov eax, PDT2_TABLE_ADDR
    or eax, 0x03
    mov [PDPT_TABLE_ADDR + 16], eax

    mov eax, PDT3_TABLE_ADDR
    or eax, 0x03
    mov [PDPT_TABLE_ADDR + 24], eax

    %macro fill_pdt 2
        mov edi, %1
        mov ebx, %2
        mov ecx, 512
    %%loop:
        mov [edi], ebx
        mov dword [edi + 4], 0
        add ebx, 0x200000
        add edi, 8
        loop %%loop
    %endmacro

    ; 0x83 = Present + Writable + Page Size 2MB, supervisor-only; user pages are mapped per process by vmm.rs
    fill_pdt PDT_TABLE_ADDR,  0x00000083
    fill_pdt PDT1_TABLE_ADDR, 0x40000083
    fill_pdt PDT2_TABLE_ADDR, 0x80000083
    fill_pdt PDT3_TABLE_ADDR, 0xC0000083

    popad
    ret

; ============================================
; Инициализация GDT64 (с поддержкой Ring 3)
; ============================================
init_gdt64:
    pushad

    mov edi, GDT64_ADDR
    mov ecx, 30
    xor eax, eax
    rep stosd

    ; [0x00] Null
    ; [0x08] Kernel Code (Ring 0)
    mov dword [GDT64_ADDR + 8], 0x0000FFFF
    mov dword [GDT64_ADDR + 12], 0x00209A00

    ; [0x10] Kernel Data (Ring 0)
    mov dword [GDT64_ADDR + 16], 0x0000FFFF
    mov dword [GDT64_ADDR + 20], 0x00209200

    ; [0x18] User Code (Ring 3)
    mov dword [GDT64_ADDR + 24], 0x0000FFFF
    mov dword [GDT64_ADDR + 28], 0x0020FA00

    ; [0x20] User Data (Ring 3)
    mov dword [GDT64_ADDR + 32], 0x0000FFFF
    mov dword [GDT64_ADDR + 36], 0x0020F200

    ; [0x28] TSS (16 байт для 64-битного режима)
    ; ВАЖНО: base указывает на валидный TSS в .bss (заполним в tss.rs)
    ; Здесь ставим base = GDT64_ADDR + 56 (временный), rsp0=0 — обновится в tss.rs
    mov eax, GDT64_ADDR + 56
    mov word [GDT64_ADDR + 40], 103
    mov word [GDT64_ADDR + 42], ax
    shr eax, 16
    mov byte [GDT64_ADDR + 44], al
    mov byte [GDT64_ADDR + 45], 0x89
    mov byte [GDT64_ADDR + 46], 0x00
    mov byte [GDT64_ADDR + 47], ah
    mov dword [GDT64_ADDR + 48], 0
    mov dword [GDT64_ADDR + 52], 0

    ; GDT pointer
    mov word [gdt64_ptr], (8 * 8 - 1)
    mov dword [gdt64_ptr + 2], GDT64_ADDR
    mov dword [gdt64_ptr + 6], 0

    popad
    ret

; ============================================
; Long Mode (64-bit)
; ============================================
bits 64
long_mode_start:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov rsp, _stack_top
    and rsp, ~0xF

    mov edi, dword [0x500]
    mov esi, dword [0x504]
    call rust_main
    cli
    hlt
    jmp $

; ============================================
; Данные
; ============================================
section .data
align 8
gdt64_ptr:
    dw 0
    dq 0
