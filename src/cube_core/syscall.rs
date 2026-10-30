// src/cube_core/syscall.rs
use core::arch::asm;
use crate::zigfs::{self, DirItem, EMPTY_ITEM};

const IA32_EFER:  u32 = 0xC0000080;
const IA32_STAR:  u32 = 0xC0000081;
const IA32_LSTAR: u32 = 0xC0000082;
const IA32_FMASK: u32 = 0xC0000084;

// Номера syscall (должны совпадать с cube_syscall)
pub const SYS_READ:     u64 = 0;
pub const SYS_WRITE:    u64 = 1;
pub const SYS_OPEN:     u64 = 2;
pub const SYS_CLOSE:    u64 = 3;
pub const SYS_EXIT:     u64 = 5;
pub const SYS_GETDENTS: u64 = 11;
pub const SYS_GETCWD:   u64 = 12;
pub const SYS_CHDIR:    u64 = 13;
pub const SYS_MKDIR:    u64 = 14;
pub const SYS_UNLINK:   u64 = 34;
pub const SYS_DEBUG:    u64 = 78;

// open() flags
const O_DIRECTORY: u64 = 0o200000;

// file types
const DT_REG: u8 = 8;
const DT_DIR: u8 = 4;

static mut KERNEL_RSP: u64 = 0;
static mut USER_RSP:   u64 = 0;

// Таблица fd
const MAX_FD: usize = 16;
const MAX_PATH: usize = 256;
const MAX_DIR_ITEMS: usize = 64;

#[derive(Copy, Clone)]
enum FdKind {
    Free,
    // Путь абсолютный, нормализованный через Zig
    File { path: [u8; MAX_PATH], path_len: usize, size: u32, offset: u32 },
    // pos — индекс следующей записи, которую вернёт getdents
    Dir  { path: [u8; MAX_PATH], path_len: usize, pos: usize },
}

#[derive(Copy, Clone)]
struct Fd { kind: FdKind }

static mut FDS: [Fd; MAX_FD] = [Fd { kind: FdKind::Free }; MAX_FD];

// Текущая директория: кластер (0 = корень) и абсолютный путь
static mut CWD_CLUSTER: u32 = 0;
static mut CWD_PATH:    [u8; MAX_PATH] = {
    let mut path = [0u8; MAX_PATH];
    path[0] = b'/';
    path
};
static mut CWD_PATH_LEN: usize = 1;

unsafe fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    asm!("wrmsr", in("ecx") msr, in("eax") lo, in("edx") hi);
}

unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi);
    ((hi as u64) << 32) | lo as u64
}

/// Stack the syscall entry switches to. The scheduler updates it on every switch to a user task.
pub fn set_kernel_rsp(kernel_rsp: u64) {
    unsafe { KERNEL_RSP = kernel_rsp };
}

pub fn init_syscall(kernel_rsp: u64) {
    unsafe {
        KERNEL_RSP = kernel_rsp;

        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | 1);

        // syscall loads CS = STAR[47:32] (kernel code 0x08) and SS = that + 8 (kernel data 0x10).
        // Using the user-code selector 0x18 here made every interrupt taken inside a syscall push CS=0x18,
        // and the iretq that returned from it raised #GP.
        wrmsr(IA32_STAR, (0x10u64 << 48) | (0x08u64 << 32));
        wrmsr(IA32_LSTAR, syscall_entry as u64);
        wrmsr(IA32_FMASK, 0x700);

        crate::serial::send_string("[SYSCALL] syscall/sysret configured\n");
        crate::serial::send_string("[TSS] rsp0=");
        print_u64(kernel_rsp);
        crate::serial::send_string("\n");
    }
}

#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        // сохранить user RSP, переключиться на kernel RSP
        "mov [rip + {user_rsp}], rsp",
        "mov rsp, [rip + {kernel_rsp}]",

        // user RIP/RFLAGS → на стек (для iretq)
        "push rcx",
        "push r11",

        // сохранить аргументы и caller-saved
        "push rdi", "push rsi", "push rdx",
        "push r8", "push r9", "push r10",

        // kernel DS/ES
        "push rax",
        "mov ax, 0x10",
        "mov ds, ax",
        "mov es, ax",
        "pop rax",

        // System V: (num=rdi, a1=rsi, a2=rdx, a3=rcx)
        "mov rcx, rdx",
        "mov rdx, rsi",
        "mov rsi, rdi",
        "mov rdi, rax",
        "call {handler}",

        // восстановить user DS/ES
        "push rax",
        "mov ax, 0x23",
        "mov ds, ax",
        "mov es, ax",
        "pop rax",

        // восстановить регистры
        "pop r10", "pop r9", "pop r8",
        "pop rdx", "pop rsi", "pop rdi",
        "pop r11",   // user RFLAGS
        "pop rcx",   // user RIP

        // iretq-фрейм: SS, RSP, RFLAGS, CS, RIP
        "push 0x23",
        "push qword ptr [rip + {user_rsp}]",
        "push r11",
        "push 0x1B",
        "push rcx",
        "iretq",

        user_rsp   = sym USER_RSP,
        kernel_rsp = sym KERNEL_RSP,
        handler    = sym syscall_dispatch,
    );
}

// === Вспомогательные ===

// Проверка, что указатель находится в пользовательском пространстве (canonical user addresses)
// Для x86_64 canonical user addresses: 0x0000_7FFF_FFFF_FFFF и ниже
fn is_user_address(ptr: u64) -> bool {
    ptr <= 0x00007FFFFFFFFFFF
}

// Пользовательский буфер [ptr, ptr+len) целиком лежит в user-пространстве
fn user_range_ok(ptr: u64, len: usize) -> bool {
    len > 0 && ptr != 0 && is_user_address(ptr) && is_user_address(ptr + len as u64 - 1)
}

unsafe fn copy_user_str(ptr: u64, len: u64, out: &mut [u8]) -> Option<usize> {
    if ptr == 0 || len == 0 || len as usize > out.len() {
        return None;
    }
    // Защита от обращения к памяти ядра из Ring 3
    if !user_range_ok(ptr, len as usize) {
        return None;
    }
    let src = ptr as *const u8;
    for i in 0..len as usize {
        out[i] = *src.add(i);
    }
    Some(len as usize)
}

unsafe fn fd_alloc() -> Option<usize> {
    for i in 3..MAX_FD {
        if let FdKind::Free = FDS[i].kind {
            return Some(i);
        }
    }
    None
}

fn print_u64(mut n: u64) {
    let mut b = [0u8; 20];
    let mut i = 20;
    if n == 0 { i -= 1; b[i] = b'0'; }
    else { while n > 0 { i -= 1; b[i] = (n % 10) as u8 + b'0'; n /= 10; } }
    crate::serial::send_string(core::str::from_utf8(&b[i..]).unwrap_or("?"));
}

fn scancode_to_ascii(sc: u8) -> Option<u8> {
    if sc & 0x80 != 0 { return None; }
    const MAP: [u8; 0x3A] = [
        0, 0, b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0',
        b'-', b'=', 0, 0,
        b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p',
        b'[', b']', b'\n', 0,
        b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l',
        b';', b'\'', b'`', 0,
        b'\\', b'z', b'x', b'c', b'v', b'b', b'n', b'm',
        b',', b'.', b'/', 0,
        b'*', 0, b' ',
    ];
    if (sc as usize) < MAP.len() {
        let c = MAP[sc as usize];
        if c != 0 { Some(c) } else { None }
    } else { None }
}

// Копия текущего рабочего каталога (кластер и путь) для разрешения относительных путей
unsafe fn cwd_snapshot() -> (u32, [u8; MAX_PATH], usize) {
    (CWD_CLUSTER, CWD_PATH, CWD_PATH_LEN)
}

// === Главный dispatcher ===

#[no_mangle]
extern "C" fn syscall_dispatch(num: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    match num {
        SYS_WRITE => {
            if a1 <= 2 && a2 != 0 && a3 > 0 && a3 <= 0x10000 {
                let ptr = a2 as *const u8;
                for i in 0..a3 as usize {
                    unsafe { crate::serial::send_byte(*ptr.add(i)); }
                }
                a3
            } else { 0 }
        }
        SYS_READ => {
            let fd = a1 as usize;
            if a2 == 0 || a3 == 0 { return u64::MAX; }

            // stdin — клавиатура (без sti/hlt!)
            if fd == 0 {
                let buf = a2 as *mut u8;
                let max = a3 as usize;
                let mut n = 0usize;
                // Block until the first key arrives. IF is cleared on syscall entry, so it is enabled
                // only here, after the kernel stack is in use: IRQs never land on the user stack.
                while n < max {
                    if let Some(sc) = crate::interrupts::keyboard_pop() {
                        if let Some(ch) = scancode_to_ascii(sc) {
                            unsafe { *buf.add(n) = ch; }
                            n += 1;
                            unsafe { crate::serial::send_byte(ch); }
                            if ch == b'\n' { break; }
                        }
                    } else if n == 0 {
                        unsafe { core::arch::asm!("sti", "hlt", "cli") };
                    } else {
                        break;
                    }
                }
                return n as u64;
            }

            // Чтение из файлов
            if fd >= MAX_FD { return u64::MAX; }
            if !user_range_ok(a2, a3 as usize) { return u64::MAX; }

            unsafe {
                match FDS[fd].kind {
                    FdKind::File { path, path_len, size, offset } => {
                        let remaining = size.saturating_sub(offset) as usize;
                        let to_read = (a3 as usize).min(remaining);
                        if to_read == 0 { return 0; }
                        let out = core::slice::from_raw_parts_mut(a2 as *mut u8, to_read);
                        match zigfs::read(0, &path[..path_len], offset, out) {
                            Some(n) => {
                                if let FdKind::File { ref mut offset, .. } = FDS[fd].kind {
                                    *offset += n as u32;
                                }
                                n as u64
                            }
                            None => u64::MAX,
                        }
                    }
                    FdKind::Dir { .. } => 0,
                    FdKind::Free => u64::MAX,
                }
            }
        }
        SYS_OPEN => {
            // a1 = flags, a2 = path_ptr, a3 = path_len
            if a2 == 0 || a3 == 0 { return u64::MAX; }
            let mut pathbuf = [0u8; MAX_PATH];
            let pathlen = match unsafe { copy_user_str(a2, a3, &mut pathbuf) } {
                Some(n) => n, None => return u64::MAX,
            };
            unsafe {
                let (cwd_cluster, cwd_path, cwd_len) = cwd_snapshot();
                let resolved = match zigfs::resolve(cwd_cluster, &cwd_path[..cwd_len], &pathbuf[..pathlen]) {
                    Some(r) => r,
                    None => return u64::MAX,
                };
                let abs_len = resolved.path_len as usize;
                let stat = match zigfs::stat(0, &resolved.path[..abs_len]) {
                    Some(s) => s,
                    None => return u64::MAX,
                };
                let fd = match fd_alloc() {
                    Some(fd) => fd,
                    None => return u64::MAX,
                };

                if stat.is_dir != 0 {
                    FDS[fd].kind = FdKind::Dir { path: resolved.path, path_len: abs_len, pos: 0 };
                } else {
                    if a1 & O_DIRECTORY != 0 { return u64::MAX; }
                    FDS[fd].kind = FdKind::File { path: resolved.path, path_len: abs_len, size: stat.size, offset: 0 };
                }
                fd as u64
            }
        }
        SYS_CLOSE => {
            if a1 < MAX_FD as u64 {
                unsafe { FDS[a1 as usize].kind = FdKind::Free; return 0; }
            }
            u64::MAX
        }
        SYS_GETDENTS => {
            let fd = a1 as usize;
            if fd >= MAX_FD || !user_range_ok(a2, a3 as usize) { return u64::MAX; }
            unsafe {
                let (path, path_len, pos) = match FDS[fd].kind {
                    FdKind::Dir { path, path_len, pos } => (path, path_len, pos),
                    _ => return u64::MAX,
                };
                let mut items = [EMPTY_ITEM; MAX_DIR_ITEMS];
                let count = match zigfs::list(0, &path[..path_len], &mut items) {
                    Some(n) => n,
                    None => return u64::MAX,
                };
                let dst = a2 as *mut u8;
                let max = a3 as usize;
                let mut written = 0usize;
                let mut next = pos;
                for j in pos..count {
                    let e: &DirItem = &items[j];
                    let nlen = e.name_len as usize;
                    let reclen = (19 + nlen + 1 + 7) & !7;
                    if written + reclen > max { break; }

                    let ent = dst.add(written);
                    *(ent as *mut u64) = e.cluster as u64;
                    *((ent.add(8)) as *mut u64) = j as u64;
                    *((ent.add(16)) as *mut u16) = reclen as u16;
                    *(ent.add(18)) = if e.attr & 0x10 != 0 { DT_DIR } else { DT_REG };
                    for i in 0..nlen { *(ent.add(19 + i)) = e.name[i]; }
                    *(ent.add(19 + nlen)) = 0;
                    written += reclen;
                    next = j + 1;
                }
                if let FdKind::Dir { ref mut pos, .. } = FDS[fd].kind {
                    *pos = next;
                }
                written as u64
            }
        }
        SYS_GETCWD => {
            // a1 = buf_ptr, a2 = len (matches cube_syscall::getcwd)
            if !user_range_ok(a1, a2 as usize) { return u64::MAX; }
            unsafe {
                let dst = a1 as *mut u8;
                let len = a2 as usize;
                let to_copy = CWD_PATH_LEN.min(len);
                for i in 0..to_copy { *dst.add(i) = CWD_PATH[i]; }
                to_copy as u64
            }
        }
        SYS_CHDIR => {
            // a1 = path_ptr, a2 = len (matches cube_syscall::chdir)
            if a1 == 0 || a2 == 0 { return u64::MAX; }
            let mut pathbuf = [0u8; MAX_PATH];
            let pathlen = match unsafe { copy_user_str(a1, a2, &mut pathbuf) } {
                Some(n) => n, None => return u64::MAX,
            };
            unsafe {
                let (cwd_cluster, cwd_path, cwd_len) = cwd_snapshot();
                let resolved = match zigfs::resolve(cwd_cluster, &cwd_path[..cwd_len], &pathbuf[..pathlen]) {
                    Some(r) => r,
                    None => return u64::MAX,
                };
                if resolved.is_dir == 0 { return u64::MAX; }
                CWD_CLUSTER = resolved.cluster;
                CWD_PATH = resolved.path;
                CWD_PATH_LEN = resolved.path_len as usize;
                0
            }
        }
        SYS_MKDIR => {
            // a1 = path_ptr, a2 = len, a3 = mode
            if a1 == 0 || a2 == 0 { return u64::MAX; }
            let mut pathbuf = [0u8; MAX_PATH];
            let pathlen = match unsafe { copy_user_str(a1, a2, &mut pathbuf) } {
                Some(n) => n, None => return u64::MAX,
            };
            unsafe {
                if zigfs::mkdir(CWD_CLUSTER, &pathbuf[..pathlen]) { 0 } else { u64::MAX }
            }
        }
        SYS_UNLINK => {
            if a1 == 0 || a2 == 0 { return u64::MAX; }
            let mut pathbuf = [0u8; MAX_PATH];
            let pathlen = match unsafe { copy_user_str(a1, a2, &mut pathbuf) } {
                Some(n) => n, None => return u64::MAX,
            };
            unsafe {
                // unlink removes only files; directories go through rmdir semantics
                if let Some(st) = zigfs::stat(CWD_CLUSTER, &pathbuf[..pathlen]) {
                    if st.is_dir != 0 { return u64::MAX; }
                }
                if zigfs::unlink(CWD_CLUSTER, &pathbuf[..pathlen]) { 0 } else { u64::MAX }
            }
        }
        SYS_DEBUG => {
            if a2 != 0 && a3 > 0 && a3 <= 0x10000 {
                let ptr = a2 as *const u8;
                crate::serial::send_string("[USER] ");
                for i in 0..a3 as usize {
                    unsafe { crate::serial::send_byte(*ptr.add(i)); }
                }
                crate::serial::send_string("\n");
                a3
            } else { 0 }
        }
        SYS_EXIT => {
            crate::serial::send_string("\n[SYSCALL] EXIT, code=");
            print_u64(a1);
            crate::serial::send_string("\n");
            unsafe {
                asm!(
                    "mov rsp, {rsp}",
                     "jmp {rip}",
                     rsp = in(reg) KERNEL_RSP,
                     rip = in(reg) crate::kernel_main_loop as u64,
                     options(noreturn),
                );
            }
        }
        _ => {
            crate::serial::send_string("[SYSCALL] unknown num=");
            print_u64(num);
            crate::serial::send_string("\n");
            0
        }
    }
}
