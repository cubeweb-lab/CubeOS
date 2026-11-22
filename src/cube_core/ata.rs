// src/cube_core/ata.rs
use crate::sync::ATA_LOCK;
use core::arch::asm;

extern "C" {
    fn ata_init_c() -> i32;
    fn ata_read_sector_c(lba: u32, buf: *mut u8) -> i32;
    fn ata_write_sector_c(lba: u32, buf: *const u8) -> i32;
}

pub struct AtaDrive {
    pub exists: bool,
    pub sectors: u32,
    pub model: [u8; 40],
}

impl AtaDrive {
    pub const fn new() -> Self {
        Self { exists: false, sectors: 0, model: [0; 40] }
    }

    pub fn init(&mut self) {
        ATA_LOCK.lock();
        unsafe { asm!("cli") };
        let ret = unsafe { ata_init_c() };
        unsafe { asm!("sti") };
        ATA_LOCK.unlock();
        self.exists = ret == 1;
    }

    pub fn read_sector(&self, lba: u32, buf: &mut [u8; 512]) -> bool {
        if !self.exists { return false; }
        ATA_LOCK.lock();
        unsafe { asm!("cli") };
        let result = unsafe { ata_read_sector_c(lba, buf.as_mut_ptr()) };
        unsafe { asm!("sti") };
        ATA_LOCK.unlock();
        result != 0
    }

    pub fn write_sector(&self, lba: u32, buf: &[u8; 512]) -> bool {
        if !self.exists { return false; }
        ATA_LOCK.lock();
        unsafe { asm!("cli") };
        let result = unsafe { ata_write_sector_c(lba, buf.as_ptr()) };
        unsafe { asm!("sti") };
        ATA_LOCK.unlock();
        result != 0
    }
}

/// Sector I/O for callers that only know the LBA (the ext4 block device).
/// Unlike the `AtaDrive` methods these preserve the caller's interrupt flag:
/// ext4 runs with interrupts masked and must not be preempted while it owns
/// the ATA lock.
pub fn read_sector_raw(lba: u32, buf: &mut [u8; 512]) -> bool {
    ATA_LOCK.lock();
    let flags: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) flags);
        asm!("cli");
    }
    let result = unsafe { ata_read_sector_c(lba, buf.as_mut_ptr()) };
    ATA_LOCK.unlock();
    if flags & 0x200 != 0 {
        unsafe { asm!("sti") };
    }
    result != 0
}

pub fn write_sector_raw(lba: u32, buf: &[u8; 512]) -> bool {
    ATA_LOCK.lock();
    let flags: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) flags);
        asm!("cli");
    }
    let result = unsafe { ata_write_sector_c(lba, buf.as_ptr()) };
    ATA_LOCK.unlock();
    if flags & 0x200 != 0 {
        unsafe { asm!("sti") };
    }
    result != 0
}
