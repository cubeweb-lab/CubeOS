use core::arch::asm;
use crate::consts::*;

#[inline]
pub fn syscall0(a: usize) -> usize {
    unsafe {
        let mut ret = a;
        asm!("syscall", inout("rax") ret, out("rcx") _, out("r11") _, options(nostack));
        ret
    }
}

#[inline]
pub fn syscall1(a: usize, b: usize) -> usize {
    unsafe {
        let mut ret = a;
        asm!("syscall", inout("rax") ret, in("rdi") b, out("rcx") _, out("r11") _, options(nostack));
        ret
    }
}

#[inline]
pub fn syscall2(a: usize, b: usize, c: usize) -> usize {
    unsafe {
        let mut ret = a;
        asm!("syscall", inout("rax") ret, in("rdi") b, in("rsi") c, out("rcx") _, out("r11") _, options(nostack));
        ret
    }
}

#[inline]
pub fn syscall3(a: usize, b: usize, c: usize, d: usize) -> usize {
    unsafe {
        let mut ret = a;
        asm!("syscall", inout("rax") ret, in("rdi") b, in("rsi") c, in("rdx") d, out("rcx") _, out("r11") _, options(nostack));
        ret
    }
}

#[inline]
pub fn syscall4(a: usize, b: usize, c: usize, d: usize, e: usize) -> usize {
    unsafe {
        let mut ret = a;
        asm!("syscall", inout("rax") ret, in("rdi") b, in("rsi") c, in("rdx") d, in("r10") e, out("rcx") _, out("r11") _, options(nostack));
        ret
    }
}

#[inline]
pub fn syscall5(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize {
    unsafe {
        let mut ret = a;
        asm!("syscall", inout("rax") ret, in("rdi") b, in("rsi") c, in("rdx") d, in("r10") e, in("r8") f, out("rcx") _, out("r11") _, options(nostack));
        ret
    }
}

#[inline]
pub fn syscall6(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize {
    unsafe {
        let mut ret = a;
        asm!("syscall", inout("rax") ret, in("rdi") b, in("rsi") c, in("rdx") d, in("r10") e, in("r8") f, in("r9") g, out("rcx") _, out("r11") _, options(nostack));
        ret
    }
}

// === Удобные обёртки ===

pub fn write(fd: usize, buf: &[u8]) -> isize {
    syscall3(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()) as isize
}

pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    syscall3(SYS_READ, fd, buf.as_mut_ptr() as usize, buf.len()) as isize
}

pub fn exit(code: usize) -> ! {
    syscall1(SYS_EXIT, code);
    loop {}
}

pub fn debug(msg: &str) -> isize {
    syscall3(SYS_DEBUG, msg.as_ptr() as usize, msg.len(), 0) as isize
}

pub fn open(path: &[u8], flags: usize) -> isize {
    syscall3(SYS_OPEN, flags, path.as_ptr() as usize, path.len()) as isize
}

pub fn close(fd: usize) -> isize {
    syscall1(SYS_CLOSE, fd) as isize
}

pub fn getdents(fd: usize, buf: &mut [u8]) -> isize {
    syscall3(SYS_GETDENTS, fd, buf.as_mut_ptr() as usize, buf.len()) as isize
}

pub fn chdir(path: &[u8]) -> isize {
    syscall2(SYS_CHDIR, path.as_ptr() as usize, path.len()) as isize
}

pub fn getcwd(buf: &mut [u8]) -> isize {
    syscall2(SYS_GETCWD, buf.as_mut_ptr() as usize, buf.len()) as isize
}

pub fn mkdir(path: &[u8], mode: usize) -> isize {
    syscall3(SYS_MKDIR, path.as_ptr() as usize, path.len(), mode) as isize
}

pub fn unlink(path: &[u8]) -> isize {
    syscall2(SYS_UNLINK, path.as_ptr() as usize, path.len()) as isize
}

// The kernel takes both paths in one buffer, "old\0new", so the call stays within three arguments.
pub fn rename(old: &[u8], new: &[u8]) -> isize {
    let mut buf = [0u8; 512];
    if old.len() + 1 + new.len() > buf.len() {
        return -1;
    }
    buf[..old.len()].copy_from_slice(old);
    buf[old.len() + 1..old.len() + 1 + new.len()].copy_from_slice(new);
    let total = old.len() + 1 + new.len();
    syscall2(SYS_RENAME, buf.as_ptr() as usize, total) as isize
}
