// src/cube_core/idt.rs
use core::mem::size_of;
use core::sync::atomic::{AtomicU64, Ordering};

const IDT_ENTRIES: usize = 256;

pub static TICKS: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
pub struct InterruptStackFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    flags: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn new() -> Self {
        Self { offset_low: 0, selector: 0, ist: 0, flags: 0,
            offset_mid: 0, offset_high: 0, reserved: 0 }
    }

    pub fn set_handler_addr(&mut self, handler_addr: u64) {
        self.offset_low = (handler_addr & 0xFFFF) as u16;
        self.selector = 0x08;
        self.ist = 0;
        self.flags = 0x8E;
        self.offset_mid = ((handler_addr >> 16) & 0xFFFF) as u16;
        self.offset_high = (handler_addr >> 32) as u32;
        self.reserved = 0;
    }
}

#[repr(C, align(16))]
struct IdtArray([IdtEntry; IDT_ENTRIES]);

static mut IDT: IdtArray = IdtArray([IdtEntry::new(); IDT_ENTRIES]);

#[repr(C, packed)]
struct IdtPtr {
    limit: u16,
    base: u64,
}

// ===== Исключения (без error code) =====
// Stubs align %rsp before calling Rust: the interrupted code's stack alignment is arbitrary,
// and an SSE spill in the handler on a misaligned stack raises #GP. %rbp keeps the frame reachable.
macro_rules! gen_stub {
    ($name:ident, $num:expr) => {
        #[unsafe(naked)]
        extern "x86-interrupt" fn $name(_sf: &InterruptStackFrame) {
            core::arch::naked_asm!(
                "push rbp",
                "mov rbp, rsp",
                "and rsp, -16",
                "push rax", "push rcx", "push rdx", "push rsi", "push rdi",
                "push r8", "push r9", "push r10", "push r11",
                "sub rsp, 8",
                "mov rdi, {num}",
                "mov rsi, [rbp + 16]",
                "mov rdx, [rbp + 8]",
                "call {handler}",
                "add rsp, 8",
                "pop r11", "pop r10", "pop r9", "pop r8",
                "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rax",
                "mov rsp, rbp",
                "pop rbp",
                "iretq",
                num = const $num,
                handler = sym exception_handler,
            );
        }
    };
}

// ===== Исключения (с error code) =====
macro_rules! gen_stub_error {
    ($name:ident, $num:expr) => {
        #[unsafe(naked)]
        extern "x86-interrupt" fn $name(_sf: &InterruptStackFrame, _error_code: u64) {
            core::arch::naked_asm!(
                "push rbp",
                "mov rbp, rsp",
                "and rsp, -16",
                "push rax", "push rcx", "push rdx", "push rsi", "push rdi",
                "push r8", "push r9", "push r10", "push r11",
                "sub rsp, 8",
                "mov rdi, {num}",
                "mov rsi, [rbp + 8]",
                "mov rdx, [rbp + 24]",
                "mov rcx, [rbp + 16]",
                "mov r8, [rbp + 40]",
                "call {handler}",
                "add rsp, 8",
                "pop r11", "pop r10", "pop r9", "pop r8",
                "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rax",
                "mov rsp, rbp",
                "pop rbp",
                "add rsp, 8",
                "iretq",
                num = const $num,
                handler = sym exception_handler_with_code,
            );
        }
    };
}

gen_stub!(isr0, 0);   gen_stub!(isr1, 1);   gen_stub!(isr2, 2);   gen_stub!(isr3, 3);
gen_stub!(isr4, 4);   gen_stub!(isr5, 5);   gen_stub!(isr6, 6);   gen_stub!(isr7, 7);
gen_stub_error!(isr8, 8);
gen_stub!(isr9, 9);
gen_stub_error!(isr10, 10); gen_stub_error!(isr11, 11); gen_stub_error!(isr12, 12);
gen_stub_error!(isr13, 13); gen_stub_error!(isr14, 14);
gen_stub!(isr15, 15); gen_stub!(isr16, 16);
gen_stub_error!(isr17, 17);
gen_stub!(isr18, 18); gen_stub!(isr19, 19); gen_stub!(isr20, 20); gen_stub!(isr21, 21);
gen_stub!(isr22, 22); gen_stub!(isr23, 23); gen_stub!(isr24, 24); gen_stub!(isr25, 25);
gen_stub!(isr26, 26); gen_stub!(isr27, 27); gen_stub!(isr28, 28); gen_stub!(isr29, 29);
gen_stub!(isr30, 30); gen_stub!(isr31, 31);

// ===== APIC timer (вектор 0x40) =====
#[unsafe(naked)]
extern "x86-interrupt" fn timer_handler(_sf: &InterruptStackFrame) {
    core::arch::naked_asm!(
        "push rbp",
        "mov rbp, rsp",
        "and rsp, -16",
        "push rax", "push rcx", "push rdx", "push rsi", "push rdi",
        "push r8", "push r9", "push r10", "push r11",
        "sub rsp, 8",
        "call {body}",
        "add rsp, 8",
        "pop r11", "pop r10", "pop r9", "pop r8",
        "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rax",
        "mov rsp, rbp",
        "pop rbp",
        "iretq",
        body = sym timer_body,
    );
}

#[no_mangle]
extern "C" fn timer_body() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::uptime::tick();
    crate::apic::eoi();
    crate::sched::tick();
}

// ===== IRQ клавиатуры (0x21) через APIC =====
#[unsafe(naked)]
extern "x86-interrupt" fn keyboard_handler(_sf: &InterruptStackFrame) {
    core::arch::naked_asm!(
        "push rbp",
        "mov rbp, rsp",
        "and rsp, -16",
        "push rax", "push rcx", "push rdx", "push rsi", "push rdi",
        "push r8", "push r9", "push r10", "push r11",
        "sub rsp, 8",
        "call {body}",
        "add rsp, 8",
        "pop r11", "pop r10", "pop r9", "pop r8",
        "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rax",
        "mov rsp, rbp",
        "pop rbp",
        "iretq",
        body = sym keyboard_body,
    );
}

// One IRQ can stand for several bytes: a key press and its release often arrive together,
// and the controller keeps the second one until the output buffer is read again.
// Drain the buffer (status bit 0 on port 0x64) so no scancode waits for the next key.
#[no_mangle]
extern "C" fn keyboard_body() {
    loop {
        let status: u8;
        unsafe { core::arch::asm!("in al, 0x64", out("al") status); }
        if status & 0x01 == 0 {
            break;
        }
        let sc: u8;
        unsafe { core::arch::asm!("in al, 0x60", out("al") sc); }
        crate::interrupts::keyboard_push(sc);
    }
    crate::apic::eoi();
}

// ===== Заглушка для прочих IRQ =====
#[unsafe(naked)]
extern "x86-interrupt" fn irq_stub(_sf: &InterruptStackFrame) {
    core::arch::naked_asm!(
        "push rbp",
        "mov rbp, rsp",
        "and rsp, -16",
        "push rax", "push rcx", "push rdx", "push rsi", "push rdi",
        "push r8", "push r9", "push r10", "push r11",
        "sub rsp, 8",
        "call {body}",
        "add rsp, 8",
        "pop r11", "pop r10", "pop r9", "pop r8",
        "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rax",
        "mov rsp, rbp",
        "pop rbp",
        "iretq",
        body = sym irq_body,
    );
}

#[no_mangle]
extern "C" fn irq_body() {
    crate::apic::eoi();
}

// ===== Обработчики исключений =====
fn print_rip(rip: u64) {
    crate::serial::send_string("  rip=0x");
    let mut buf = [0u8; 16];
    for j in 0..16 {
        let nib = ((rip >> ((15 - j) * 4)) & 0xF) as u8;
        buf[j] = if nib < 10 { b'0' + nib } else { b'A' + nib - 10 };
    }
    crate::serial::send_string(core::str::from_utf8(&buf).unwrap_or("?"));
    crate::serial::send_string("\n");
}

#[no_mangle]
pub extern "C" fn exception_handler(int_num: u64, cs: u64, rip: u64) {
    print_rip(rip);
    crate::serial::log_error("EXCEPTION");
    crate::serial::send_string("  Vector: ");
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut n = int_num;
    if n == 0 { i -= 1; buf[i] = b'0'; }
    else { while n > 0 { i -= 1; buf[i] = (n % 10) as u8 + b'0'; n /= 10; } }
    crate::serial::send_string(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
    crate::serial::send_string("\n");
    terminate_if_user(cs);
    loop { unsafe { core::arch::asm!("cli; hlt") } }
}

// CPL lives in the low two bits of the interrupted CS; ring 3 means the fault belongs to a process, not the kernel.
fn terminate_if_user(cs: u64) {
    if cs & 3 == 3 {
        crate::serial::log_error("Faulting task is user mode, terminating it");
        crate::sched::exit_current();
    }
}

#[no_mangle]
pub extern "C" fn exception_handler_with_code(int_num: u64, error_code: u64, cs: u64, rip: u64, fault_rsp: u64) {
    print_rip(rip);
    // The five words at the faulting %rsp are the frame that iretq was about to pop.
    for i in 0..5u64 {
        let word = unsafe { *((fault_rsp + i * 8) as *const u64) };
        crate::serial::send_string("  iret frame[");
        crate::serial::send_byte(b'0' + i as u8);
        crate::serial::send_string("]=0x");
        let mut wbuf = [0u8; 16];
        for j in 0..16 {
            let nib = ((word >> ((15 - j) * 4)) & 0xF) as u8;
            wbuf[j] = if nib < 10 { b'0' + nib } else { b'A' + nib - 10 };
        }
        crate::serial::send_string(core::str::from_utf8(&wbuf).unwrap_or("?"));
        crate::serial::send_string("\n");
    }
    crate::serial::send_string("  frame cs=0x");
    let mut cbuf = [0u8; 16];
    for j in 0..16 {
        let nib = ((cs >> ((15 - j) * 4)) & 0xF) as u8;
        cbuf[j] = if nib < 10 { b'0' + nib } else { b'A' + nib - 10 };
    }
    crate::serial::send_string(core::str::from_utf8(&cbuf).unwrap_or("?"));
    crate::serial::send_string("\n");
    crate::serial::log_error("EXCEPTION");
    crate::serial::send_string("  Vector: ");
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut n = int_num;
    if n == 0 { i -= 1; buf[i] = b'0'; }
    else { while n > 0 { i -= 1; buf[i] = (n % 10) as u8 + b'0'; n /= 10; } }
    crate::serial::send_string(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
    crate::serial::send_string(", err=0x");
    for j in 0..16 {
        let nib = ((error_code >> ((15 - j) * 4)) & 0xF) as u8;
        buf[j] = if nib < 10 { b'0' + nib } else { b'A' + nib - 10 };
    }
    crate::serial::send_string(core::str::from_utf8(&buf[..16]).unwrap_or("?"));
    crate::serial::send_string("\n");

    if int_num == 14 {
        let cr2: u64;
        unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2); }
        crate::serial::send_string("  CR2=0x");
        for j in 0..16 {
            let nib = ((cr2 >> ((15 - j) * 4)) & 0xF) as u8;
            buf[j] = if nib < 10 { b'0' + nib } else { b'A' + nib - 10 };
        }
        crate::serial::send_string(core::str::from_utf8(&buf[..16]).unwrap_or("?"));
        crate::serial::send_string("\n");
        let cr3: u64;
        unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3); }
        crate::serial::send_string("  CR3=0x");
        for j in 0..16 {
            let nib = ((cr3 >> ((15 - j) * 4)) & 0xF) as u8;
            buf[j] = if nib < 10 { b'0' + nib } else { b'A' + nib - 10 };
        }
        crate::serial::send_string(core::str::from_utf8(&buf[..16]).unwrap_or("?"));
        crate::serial::send_string("\n");
        crate::vmm::debug_walk(cr3, cr2);
    }
    terminate_if_user(cs);
    loop { unsafe { core::arch::asm!("cli; hlt") } }
}

pub unsafe fn init_idt() {
    IDT.0[0].set_handler_addr(isr0 as u64);
    IDT.0[1].set_handler_addr(isr1 as u64);
    IDT.0[2].set_handler_addr(isr2 as u64);
    IDT.0[3].set_handler_addr(isr3 as u64);
    IDT.0[4].set_handler_addr(isr4 as u64);
    IDT.0[5].set_handler_addr(isr5 as u64);
    IDT.0[6].set_handler_addr(isr6 as u64);
    IDT.0[7].set_handler_addr(isr7 as u64);
    IDT.0[8].set_handler_addr(isr8 as u64);
    IDT.0[9].set_handler_addr(isr9 as u64);
    IDT.0[10].set_handler_addr(isr10 as u64);
    IDT.0[11].set_handler_addr(isr11 as u64);
    IDT.0[12].set_handler_addr(isr12 as u64);
    IDT.0[13].set_handler_addr(isr13 as u64);
    IDT.0[14].set_handler_addr(isr14 as u64);
    IDT.0[15].set_handler_addr(isr15 as u64);
    IDT.0[16].set_handler_addr(isr16 as u64);
    IDT.0[17].set_handler_addr(isr17 as u64);
    IDT.0[18].set_handler_addr(isr18 as u64);
    IDT.0[19].set_handler_addr(isr19 as u64);
    IDT.0[20].set_handler_addr(isr20 as u64);
    IDT.0[21].set_handler_addr(isr21 as u64);
    IDT.0[22].set_handler_addr(isr22 as u64);
    IDT.0[23].set_handler_addr(isr23 as u64);
    IDT.0[24].set_handler_addr(isr24 as u64);
    IDT.0[25].set_handler_addr(isr25 as u64);
    IDT.0[26].set_handler_addr(isr26 as u64);
    IDT.0[27].set_handler_addr(isr27 as u64);
    IDT.0[28].set_handler_addr(isr28 as u64);
    IDT.0[29].set_handler_addr(isr29 as u64);
    IDT.0[30].set_handler_addr(isr30 as u64);
    IDT.0[31].set_handler_addr(isr31 as u64);

    // IRQ-заглушки 0x20..0x2F — на случай, если что-то придёт
    for i in 0..16 {
        IDT.0[0x20 + i].set_handler_addr(irq_stub as u64);
    }

    // Переопределяем конкретные
    IDT.0[0x21].set_handler_addr(keyboard_handler as u64);
    IDT.0[0x40].set_handler_addr(timer_handler as u64);

    let idt_ptr = IdtPtr {
        limit: (size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
        base: &raw const IDT as *const _ as u64,
    };
    core::arch::asm!("lidt [{}]", in(reg) &idt_ptr);
}

pub fn set_interrupt_handler(vector: usize, handler: u64) {
    unsafe { IDT.0[vector].set_handler_addr(handler); }
}
