// src/cube_core/zigfs.rs
// Rust side of the Zig FAT driver (zig/src): C exports that Zig calls, and wrappers that call Zig.
// Paths are relative to `start_dir` (a cluster, 0 = root) unless they start with '/'.
use crate::serial;
use crate::sync::ATA_LOCK;
use core::arch::asm;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stat {
    pub is_dir: u32,
    pub size: u32,
    pub cluster: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Resolved {
    pub cluster: u32,
    pub is_dir: u32,
    pub path: [u8; 256],
    pub path_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirItem {
    pub name: [u8; 256],
    pub name_len: u32,
    pub attr: u32,
    pub size: u32,
    pub cluster: u32,
}

pub const EMPTY_ITEM: DirItem = DirItem { name: [0; 256], name_len: 0, attr: 0, size: 0, cluster: 0 };

extern "C" {
    fn ata_read_sector_c(lba: u32, buf: *mut u8) -> i32;
    fn ata_write_sector_c(lba: u32, buf: *const u8) -> i32;

    fn zig_fat_mount(partition_lba: u32) -> bool;
    fn zig_fat_type() -> u32;
    fn zig_fat_stat(start_dir: u32, path: *const u8, path_len: usize, out: *mut Stat) -> bool;
    fn zig_fat_resolve(
        start_dir: u32,
        cwd: *const u8,
        cwd_len: usize,
        input: *const u8,
        input_len: usize,
        out: *mut Resolved,
    ) -> bool;
    fn zig_fat_read(start_dir: u32, path: *const u8, path_len: usize, offset: u32, out: *mut u8, max_len: u32) -> i32;
    fn zig_fat_list(start_dir: u32, path: *const u8, path_len: usize, items: *mut DirItem, max_items: u32) -> i32;
    fn zig_fat_write_file(start_dir: u32, path: *const u8, path_len: usize, data: *const u8, data_len: usize) -> bool;
    fn zig_fat_mkdir(start_dir: u32, path: *const u8, path_len: usize) -> bool;
    fn zig_fat_unlink(start_dir: u32, path: *const u8, path_len: usize) -> bool;
    fn zig_fat_rmdir(start_dir: u32, path: *const u8, path_len: usize, recursive: bool) -> bool;
    fn zig_fat_rename(start_dir: u32, old: *const u8, old_len: usize, new: *const u8, new_len: usize) -> bool;
}

#[no_mangle]
pub extern "C" fn cube_ata_read_sector(lba: u32, buf: *mut u8) -> bool {
    ATA_LOCK.lock();
    unsafe { asm!("cli") };
    let ok = unsafe { ata_read_sector_c(lba, buf) } != 0;
    unsafe { asm!("sti") };
    ATA_LOCK.unlock();
    ok
}

#[no_mangle]
pub extern "C" fn cube_ata_write_sector(lba: u32, buf: *const u8) -> bool {
    ATA_LOCK.lock();
    unsafe { asm!("cli") };
    let ok = unsafe { ata_write_sector_c(lba, buf) } != 0;
    unsafe { asm!("sti") };
    ATA_LOCK.unlock();
    ok
}

#[no_mangle]
pub extern "C" fn cube_console_putc(c: u8) {
    serial::send_byte(c);
}

/// Mounts the FAT volume that starts at `partition_lba`. Returns the FAT variant name.
pub fn mount(partition_lba: u32) -> Option<&'static str> {
    if !unsafe { zig_fat_mount(partition_lba) } {
        return None;
    }
    match unsafe { zig_fat_type() } {
        1 => Some("FAT12"),
        2 => Some("FAT16"),
        3 => Some("FAT32"),
        _ => None,
    }
}

pub fn stat(start_dir: u32, path: &[u8]) -> Option<Stat> {
    let mut out = Stat { is_dir: 0, size: 0, cluster: 0 };
    if unsafe { zig_fat_stat(start_dir, path.as_ptr(), path.len(), &mut out) } {
        Some(out)
    } else {
        None
    }
}

/// Normalizes `input` against the working directory `cwd` into an absolute path.
pub fn resolve(start_dir: u32, cwd: &[u8], input: &[u8]) -> Option<Resolved> {
    let mut out = Resolved { cluster: 0, is_dir: 0, path: [0; 256], path_len: 0 };
    let ok = unsafe {
        zig_fat_resolve(start_dir, cwd.as_ptr(), cwd.len(), input.as_ptr(), input.len(), &mut out)
    };
    if ok { Some(out) } else { None }
}

/// Reads up to `out.len()` bytes starting at `offset`. Returns the number of bytes read.
pub fn read(start_dir: u32, path: &[u8], offset: u32, out: &mut [u8]) -> Option<usize> {
    let n = unsafe { zig_fat_read(start_dir, path.as_ptr(), path.len(), offset, out.as_mut_ptr(), out.len() as u32) };
    if n < 0 { None } else { Some(n as usize) }
}

/// Fills `items` with the entries of the directory at `path`. Returns the entry count.
pub fn list(start_dir: u32, path: &[u8], items: &mut [DirItem]) -> Option<usize> {
    let n = unsafe { zig_fat_list(start_dir, path.as_ptr(), path.len(), items.as_mut_ptr(), items.len() as u32) };
    if n < 0 { None } else { Some(n as usize) }
}

pub fn write_file(start_dir: u32, path: &[u8], data: &[u8]) -> bool {
    unsafe { zig_fat_write_file(start_dir, path.as_ptr(), path.len(), data.as_ptr(), data.len()) }
}

pub fn mkdir(start_dir: u32, path: &[u8]) -> bool {
    unsafe { zig_fat_mkdir(start_dir, path.as_ptr(), path.len()) }
}

pub fn unlink(start_dir: u32, path: &[u8]) -> bool {
    unsafe { zig_fat_unlink(start_dir, path.as_ptr(), path.len()) }
}

pub fn rename(start_dir: u32, old: &[u8], new: &[u8]) -> bool {
    unsafe { zig_fat_rename(start_dir, old.as_ptr(), old.len(), new.as_ptr(), new.len()) }
}

#[allow(dead_code)] // used by the upcoming directory commands
pub fn rmdir(start_dir: u32, path: &[u8], recursive: bool) -> bool {
    unsafe { zig_fat_rmdir(start_dir, path.as_ptr(), path.len(), recursive) }
}

/// Writes, reads back and deletes a scratch file on the system volume. Called only with the `kernel-debug` feature.
pub fn self_test() -> bool {
    const PATH: &[u8] = b"/TMP/SELFTST.TXT";
    const DATA: &[u8] = b"zig fat self-test\n";

    let mut buf = [0u8; 64];
    let written = write_file(0, PATH, DATA);
    let read_ok = read(0, PATH, 0, &mut buf) == Some(DATA.len()) && buf[..DATA.len()] == *DATA;
    let removed = unlink(0, PATH);

    let ok = written && read_ok && removed;
    serial::send_string(if ok { "[  OK ] FAT self-test: write, read and delete passed\n" } else { "[ERROR] FAT self-test failed\n" });
    ok
}
