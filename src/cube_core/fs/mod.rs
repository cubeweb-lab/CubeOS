// src/cube_core/fs/mod.rs
// File system front end: finds the root volume in the MBR and forwards file
// operations to either the Zig FAT driver or the ext4_rs backend.
use crate::ata::AtaDrive;
use crate::serial;
use crate::{ext4fs, zigfs};

mod scan;

pub use crate::zigfs::{DirItem, Resolved, Stat, EMPTY_ITEM};

// MBR partition types of FAT12, FAT16 (<32 MB, >32 MB) and FAT32 (CHS and LBA)
const FAT_PARTITION_TYPES: [u8; 6] = [0x01, 0x04, 0x06, 0x0B, 0x0C, 0x0E];

#[derive(Copy, Clone, PartialEq, Eq)]
enum Kind {
    None,
    Fat,
    Ext4,
}

// TODO: replace the single global KIND with a mount table plus a FileOps/VfsNode trait so FAT and ext4 can be mounted together and paths route across mount points instead of selecting one root filesystem at boot.
static mut KIND: Kind = Kind::None;

fn kind() -> Kind {
    unsafe { KIND }
}

pub struct FSManager;

impl FSManager {
    pub const fn new() -> Self {
        Self
    }

    /// Mounts the volume whose label matches `label` (case-insensitive,
    /// trailing padding ignored). ext4 is preferred over FAT so an image can
    /// carry both without changing the root selection.
    pub fn mount_label(&mut self, ata: &AtaDrive, label: &[u8]) -> bool {
        if let Some(lba) = find_ext4_by_label(ata, label) {
            serial::send_string("[FS] trying ext4 root\n");
            if ext4fs::mount(lba) {
                unsafe { KIND = Kind::Ext4 };
                serial::send_string("[FS] ext4 root volume mounted\n");
                return true;
            }
            serial::send_string("[FS] ext4 mount failed, falling back to FAT\n");
        }

        let lba = match find_fat_by_label(ata, label) {
            Some(lba) => lba,
            None => {
                serial::send_string("[FS] No volume with the requested label\n");
                return false;
            }
        };
        match zigfs::mount(lba) {
            Some(fs_kind) => {
                unsafe { KIND = Kind::Fat };
                serial::send_string("[FS] ");
                serial::send_string(fs_kind);
                serial::send_string(" root volume mounted\n");
                true
            }
            None => false,
        }
    }

    /// Reads a whole file given by an absolute path such as "/SYSTEM/CONFIG/BOOT.CFG".
    pub fn read_file(&self, path: &str, buf: &mut [u8]) -> Option<usize> {
        read(0, path.as_bytes(), 0, buf)
    }
}

pub fn resolve(start_dir: u32, cwd: &[u8], input: &[u8]) -> Option<Resolved> {
    match kind() {
        Kind::Fat => zigfs::resolve(start_dir, cwd, input),
        Kind::Ext4 => ext4fs::resolve(cwd, input),
        Kind::None => None,
    }
}

pub fn stat(start_dir: u32, path: &[u8]) -> Option<Stat> {
    match kind() {
        Kind::Fat => zigfs::stat(start_dir, path),
        Kind::Ext4 => ext4fs::stat(path),
        Kind::None => None,
    }
}

pub fn read(start_dir: u32, path: &[u8], offset: u32, out: &mut [u8]) -> Option<usize> {
    match kind() {
        Kind::Fat => zigfs::read(start_dir, path, offset, out),
        Kind::Ext4 => ext4fs::read(path, offset, out),
        Kind::None => None,
    }
}

pub fn list(start_dir: u32, path: &[u8], items: &mut [DirItem]) -> Option<usize> {
    match kind() {
        Kind::Fat => zigfs::list(start_dir, path, items),
        Kind::Ext4 => ext4fs::list(path, items),
        Kind::None => None,
    }
}

pub fn write_file(start_dir: u32, path: &[u8], data: &[u8]) -> bool {
    match kind() {
        Kind::Fat => zigfs::write_file(start_dir, path, data),
        Kind::Ext4 => ext4fs::write_file(path, data),
        Kind::None => false,
    }
}

pub fn mkdir(start_dir: u32, path: &[u8]) -> bool {
    match kind() {
        Kind::Fat => zigfs::mkdir(start_dir, path),
        Kind::Ext4 => ext4fs::mkdir(path),
        Kind::None => false,
    }
}

pub fn unlink(start_dir: u32, path: &[u8]) -> bool {
    match kind() {
        Kind::Fat => zigfs::unlink(start_dir, path),
        Kind::Ext4 => ext4fs::unlink(path),
        Kind::None => false,
    }
}

pub fn rename(start_dir: u32, old: &[u8], new: &[u8]) -> bool {
    match kind() {
        Kind::Fat => zigfs::rename(start_dir, old, new),
        Kind::Ext4 => ext4fs::rename(old, new),
        Kind::None => false,
    }
}

#[allow(dead_code)] // used by the upcoming directory commands
pub fn rmdir(start_dir: u32, path: &[u8], recursive: bool) -> bool {
    match kind() {
        Kind::Fat => zigfs::rmdir(start_dir, path, recursive),
        Kind::Ext4 => {
            if recursive {
                false
            } else {
                ext4fs::rmdir(path)
            }
        }
        Kind::None => false,
    }
}

/// Scans MBR entries for a FAT volume carrying `label`.
fn find_fat_by_label(ata: &AtaDrive, label: &[u8]) -> Option<u32> {
    let partitions = read_partitions(ata)?;
    for part in partitions {
        if !FAT_PARTITION_TYPES.contains(&part.part_type) || part.start_lba == 0 {
            continue;
        }
        let mut boot = [0u8; 512];
        if !ata.read_sector(part.start_lba, &mut boot) {
            continue;
        }
        if boot[510] != 0x55 || boot[511] != 0xAA {
            continue;
        }
        if scan::fat_label(&boot).is_some_and(|raw| scan::label_eq(&raw, label)) {
            return Some(part.start_lba);
        }
    }
    None
}

/// Scans MBR entries for an ext4 volume carrying `label`. Any partition with
/// a valid superblock is a fallback: the root is selected by filesystem, not
/// by the MBR type byte, so a GRUB-installed image with a nonstandard type
/// still boots.
fn find_ext4_by_label(ata: &AtaDrive, label: &[u8]) -> Option<u32> {
    let partitions = read_partitions(ata)?;
    let mut fallback = None;
    for part in partitions {
        if part.start_lba == 0 {
            continue;
        }
        let Some(sector) = part.start_lba.checked_add(2) else {
            continue;
        };
        let mut superblock = [0u8; 512];
        if !ata.read_sector(sector, &mut superblock) {
            continue;
        }
        let Some(raw) = scan::ext4_label(&superblock) else {
            continue;
        };
        if fallback.is_none() {
            fallback = Some(part.start_lba);
        }
        if scan::label_eq(&raw, label) {
            return Some(part.start_lba);
        }
    }
    fallback
}

fn read_partitions(ata: &AtaDrive) -> Option<[scan::Partition; scan::MBR_PARTITION_ENTRIES]> {
    let mut mbr = [0u8; 512];
    if !ata.read_sector(0, &mut mbr) || mbr[510] != 0x55 || mbr[511] != 0xAA {
        return None;
    }
    Some(scan::mbr_partitions(&mbr))
}
