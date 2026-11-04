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
pub const SYS_RENAME:   u64 = 82;
pub const SYS_DEBUG:    u64 = 78;
pub const SYS_EXEC:     u64 = 99;

// open() flags
const O_WRONLY:    u64 = 0o1;
const O_RDWR:      u64 = 0o2;
const O_CREAT:     u64 = 0o100;
const O_TRUNC:     u64 = 0o1000;
const O_APPEND:    u64 = 0o2000;
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

// The kernel stack of a user task is a single 4 KiB page, far too small for this array (~18 KiB),
// so it lives in static memory. Only getdents uses it, and the kernel runs one syscall at a time here.
static mut DIR_ITEMS: [DirItem; MAX_DIR_ITEMS] = [EMPTY_ITEM; MAX_DIR_ITEMS];

// Files open for writing are edited in memory and written back on close. Each fd owns one buffer.
const WRITE_LIMIT: usize = 4096;
static mut WRITE_BUF: [[u8; WRITE_LIMIT]; MAX_FD] = [[0; WRITE_LIMIT]; MAX_FD];

#[derive(Copy, Clone)]
enum FdKind {
    Free,
    // Путь абсолютный, нормализованный через Zig
    File { path: [u8; MAX_PATH], path_len: usize, size: u32, offset: u32 },
    // pos — индекс следующей записи, которую вернёт getdents
    Dir  { path: [u8; MAX_PATH], path_len: usize, pos: usize },
    // size — длина буфера, pos — текущая позиция записи
    Write { path: [u8; MAX_PATH], path_len: usize, size: usize, pos: usize },
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
        // IF stays set in syscalls so the keyboard and timer IRQs run while the shell waits in stdin reads.
        wrmsr(IA32_FMASK, 0x500);

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

// Opens a file for writing. O_CREAT creates it when missing; without O_TRUNC the existing contents are kept.
unsafe fn open_for_write(flags: u64, input: &[u8]) -> u64 {
    let (cwd_cluster, cwd_path, cwd_len) = cwd_snapshot();
    if zigfs::resolve(cwd_cluster, &cwd_path[..cwd_len], input).is_none() {
        if flags & O_CREAT == 0 { return u64::MAX; }
        if !zigfs::write_file(cwd_cluster, input, &[]) { return u64::MAX; }
    }
    let resolved = match zigfs::resolve(cwd_cluster, &cwd_path[..cwd_len], input) {
        Some(r) => r,
        None => return u64::MAX,
    };
    let abs_len = resolved.path_len as usize;
    let stat = match zigfs::stat(0, &resolved.path[..abs_len]) {
        Some(s) => s,
        None => return u64::MAX,
    };
    if stat.is_dir != 0 || stat.size as usize > WRITE_LIMIT { return u64::MAX; }
    let fd = match fd_alloc() {
        Some(fd) => fd,
        None => return u64::MAX,
    };

    let mut size = 0usize;
    if flags & O_TRUNC == 0 && stat.size > 0 {
        let write_bufs = &mut *core::ptr::addr_of_mut!(WRITE_BUF);
        let buf = &mut write_bufs[fd][..stat.size as usize];
        match zigfs::read(0, &resolved.path[..abs_len], 0, buf) {
            Some(n) => size = n,
            None => return u64::MAX,
        }
    }
    let pos = if flags & O_APPEND != 0 { size } else { 0 };
    FDS[fd].kind = FdKind::Write { path: resolved.path, path_len: abs_len, size, pos };
    fd as u64
}

// Copies user bytes into the fd's buffer at its position and advances the position.
unsafe fn write_file_fd(fd: usize, ptr: u64, len: u64) -> u64 {
    if fd >= MAX_FD || len == 0 || !user_range_ok(ptr, len as usize) { return u64::MAX; }
    let (pos, size) = match FDS[fd].kind {
        FdKind::Write { pos, size, .. } => (pos, size),
        _ => return u64::MAX,
    };
    let n = len as usize;
    if pos + n > WRITE_LIMIT { return u64::MAX; }
    let src = core::slice::from_raw_parts(ptr as *const u8, n);
    let write_bufs = &mut *core::ptr::addr_of_mut!(WRITE_BUF);
    write_bufs[fd][pos..pos + n].copy_from_slice(src);
    let new_pos = pos + n;
    if let FdKind::Write { ref mut pos, ref mut size, .. } = FDS[fd].kind {
        *pos = new_pos;
        *size = (*size).max(new_pos);
    }
    n as u64
}

// Writes the buffer of a file opened for writing back to the volume. Other fds have nothing to flush.
unsafe fn flush_write_fd(fd: usize) -> u64 {
    let (path, path_len, size) = match FDS[fd].kind {
        FdKind::Write { path, path_len, size, .. } => (path, path_len, size),
        _ => return 0,
    };
    let write_bufs = &*core::ptr::addr_of!(WRITE_BUF);
    let data = &write_bufs[fd][..size];
    if zigfs::write_file(0, &path[..path_len], data) { 0 } else { u64::MAX }
}

// Shift state from the make/break codes of the two shift keys.
static mut SHIFT_DOWN: bool = false;

// Like scancode_to_ascii, but tracks Shift so capital letters and symbols such as '>' come out right.
fn key_to_ascii(sc: u8) -> Option<u8> {
    match sc {
        0x2A | 0x36 => {
            unsafe { SHIFT_DOWN = true };
            return None;
        }
        0xAA | 0xB6 => {
            unsafe { SHIFT_DOWN = false };
            return None;
        }
        _ => {}
    }
    let c = scancode_to_ascii(sc)?;
    if unsafe { SHIFT_DOWN } { Some(shifted(c)) } else { Some(c) }
}

fn shifted(c: u8) -> u8 {
    match c {
        b'a'..=b'z' => c - 32,
        b'1' => b'!', b'2' => b'@', b'3' => b'#', b'4' => b'$', b'5' => b'%',
        b'6' => b'^', b'7' => b'&', b'8' => b'*', b'9' => b'(', b'0' => b')',
        b'-' => b'_', b'=' => b'+', b'[' => b'{', b']' => b'}', b'\\' => b'|',
        b';' => b':', b'\'' => b'"', b'`' => b'~', b',' => b'<', b'.' => b'>', b'/' => b'?',
        _ => c,
    }
}

// === Главный dispatcher ===

#[no_mangle]
extern "C" fn syscall_dispatch(num: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    match num {
        SYS_WRITE => {
            if a1 >= 3 {
                return unsafe { write_file_fd(a1 as usize, a2, a3) };
            }
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
                        if let Some(ch) = key_to_ascii(sc) {
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
                    FdKind::Write { .. } => u64::MAX,
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
            if a1 & (O_WRONLY | O_RDWR) != 0 {
                return unsafe { open_for_write(a1, &pathbuf[..pathlen]) };
            }
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
                let fd = a1 as usize;
                let result = unsafe { flush_write_fd(fd) };
                unsafe { FDS[fd].kind = FdKind::Free };
                return result;
            }
            u64::MAX
        }
        SYS_RENAME => {
            // a1 = ptr to "old\0new", a2 = total length (matches cube_syscall::rename)
            if a1 == 0 || a2 == 0 { return u64::MAX; }
            let mut buf = [0u8; 512];
            let len = match unsafe { copy_user_str(a1, a2, &mut buf) } {
                Some(n) => n, None => return u64::MAX,
            };
            let sep = match buf[..len].iter().position(|&b| b == 0) {
                Some(i) => i, None => return u64::MAX,
            };
            let old = &buf[..sep];
            let new = &buf[sep + 1..len];
            unsafe {
                if zigfs::rename(CWD_CLUSTER, old, new) { 0 } else { u64::MAX }
            }
        }
        SYS_GETDENTS => {
            let fd = a1 as usize;
            if fd >= MAX_FD || !user_range_ok(a2, a3 as usize) { return u64::MAX; }
            unsafe {
                let (path, path_len, pos) = match FDS[fd].kind {
                    FdKind::Dir { path, path_len, pos } => (path, path_len, pos),
                    _ => return u64::MAX,
                };
                let items = &mut *core::ptr::addr_of_mut!(DIR_ITEMS);
                let count = match zigfs::list(0, &path[..path_len], items) {
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
        SYS_EXEC => {
            // a1 = ptr to "path\\0", a2 = total length (включая \\0)
            if a1 == 0 || a2 == 0 || a2 > 512 {
                return u64::MAX;
            }

            // Копируем путь из user space
            let mut path_buf = [0u8; 512];
            let len = match unsafe { copy_user_str(a1, a2, &mut path_buf) } {
                Some(n) => n,
                None => return u64::MAX,
            };

            // Проверка: строка должна содержать нулевой терминатор
            if path_buf[..len].iter().all(|&b| b != 0) {
                return u64::MAX;
            }

            let path = match core::str::from_utf8(&path_buf[..len]) {
                Ok(s) => s.trim_end_matches("\\0"),
                Err(_) => return u64::MAX,
            };

            // Буфер для программы — локальный, не static mut
            const MAX_PROGRAM_SIZE: usize = 65536;
            let mut program_buf = [0u8; MAX_PROGRAM_SIZE];

            // Читаем файл через zigfs
            const START_DIR: u32 = 0; // корень
            let size = match crate::zigfs::read(START_DIR, path.as_bytes(), 0, &mut program_buf) {
                Some(n) => n,
                None => {
                    crate::serial::send_string("[SYSCALL] EXEC: file not found or read error: ");
                    crate::serial::send_string(path);
                    crate::serial::send_string("\\n");
                    return u64::MAX;
                }
            };

            // Проверка ELF-магии
            if size < 4 || program_buf[0..4] != *b"\\x7fELF" {
                crate::serial::send_string("[SYSCALL] EXEC: invalid ELF: ");
                crate::serial::send_string(path);
                crate::serial::send_string("\\n");
                return u64::MAX;
            }

            // Создаём новое адресное пространство
            let mut aspace = match crate::vmm::AddressSpace::new() {
                Some(a) => a,
                None => {
                    crate::serial::send_string("[SYSCALL] EXEC: failed to create AddressSpace\\n");
                    return u64::MAX;
                }
            };

            // Загружаем и запускаем программу
            match crate::loader::spawn_program(&mut aspace, &program_buf[..size], path, crate::sched::Qos::Interactive) {
                Some(pid) => {
                    crate::serial::send_string("[SYSCALL] EXEC: launched ");
                    crate::serial::send_string(path);
                    crate::serial::send_string(" as pid=");

                    // Вывод PID (без внешних утилит)
                    let mut temp = [0u8; 20];
                    let mut i = 19;
                    let mut n = pid as usize;
                    if n == 0 {
                        temp[i] = b'0';
                        i -= 1;
                    } else {
                        while n > 0 && i > 0 {
                            temp[i] = b'0' + (n % 10) as u8;
                            n /= 10;
                            i -= 1;
                        }
                    }
                    crate::serial::send_string(core::str::from_utf8(&temp[i+1..]).unwrap_or("?"));
                    crate::serial::send_string("\\n");

                    pid as u64
                }
                None => {
                    // Ошибка загрузки — освобождаем ресурсы
                    crate::vmm::destroy_pml4(aspace.pml4_phys());
                    u64::MAX
                }
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
