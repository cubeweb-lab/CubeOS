// src/cube_core/zigfs.rs
// Bridge to the Zig FAT driver in zig/src: C exports that Zig calls, and wrappers that call Zig.
use crate::serial;
use crate::sync::ATA_LOCK;
use core::arch::asm;

extern "C" {
    fn ata_read_sector_c(lba: u32, buf: *mut u8) -> i32;
    fn ata_write_sector_c(lba: u32, buf: *const u8) -> i32;

    fn zig_fat_mount(partition_lba: u32) -> bool;
    fn zig_fat_type() -> u32;
    fn zig_fat_read_file(path: *const u8, path_len: usize, out: *mut u8, max_len: u32) -> i32;
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

/// Mounts the FAT volume that starts at `partition_lba` through the Zig driver.
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

/// Reads `path` with the Zig driver. Returns the number of bytes copied into `out`.
pub fn read_file(path: &str, out: &mut [u8]) -> Option<usize> {
    let n = unsafe { zig_fat_read_file(path.as_ptr(), path.len(), out.as_mut_ptr(), out.len() as u32) };
    if n < 0 { None } else { Some(n as usize) }
}

/// Cross-checks a file that the Rust FAT32 driver already read: the Zig driver must return the same bytes.
pub fn verify_file(partition_lba: u32, path: &str, expected: &[u8]) -> bool {
    let kind = match mount(partition_lba) {
        Some(kind) => kind,
        None => {
            serial::log_error("Zig FAT driver failed to mount the system partition");
            return false;
        }
    };

    let mut buf = [0u8; 4096];
    let matches = match read_file(path, &mut buf) {
        Some(n) => n == expected.len() && buf[..n] == *expected,
        None => false,
    };

    serial::send_string("[ ZIG  ] ");
    serial::send_string(kind);
    serial::send_string(if matches {
        " driver: file matches the Rust FAT32 read\n"
    } else {
        " driver: file differs from the Rust FAT32 read\n"
    });
    matches
}
