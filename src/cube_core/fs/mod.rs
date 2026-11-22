// src/cube_core/fs/mod.rs
// File system front end: finds the root volume in the MBR and forwards file
// operations to either the Zig FAT driver or the ext4_rs backend.
use crate::ata::AtaDrive;
use crate::serial;
use crate::{ext4fs, zigfs};

pub use crate::zigfs::{DirItem, Resolved, Stat, EMPTY_ITEM};

// MBR partition types of FAT12, FAT16 (<32 MB, >32 MB) and FAT32 (CHS and LBA)
const FAT_PARTITION_TYPES: [u8; 6] = [0x01, 0x04, 0x06, 0x0B, 0x0C, 0x0E];
const EXT4_PARTITION_TYPE: u8 = 0x83;
const MBR_PARTITION_OFFSET: usize = 446;
const MBR_PARTITION_ENTRIES: usize = 4;
const EXT4_SUPERBLOCK_MAGIC: u16 = 0xEF53;

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
            if ext4fs::mount(lba) {
                unsafe { KIND = Kind::Ext4 };
                serial::send_string("[FS] ext4 root volume mounted\n");
                return true;
            }
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

/// Scans the four primary MBR entries for a FAT volume carrying `label`.
fn find_fat_by_label(ata: &AtaDrive, label: &[u8]) -> Option<u32> {
    for (part_type, start_lba) in mbr_partitions(ata) {
        if !FAT_PARTITION_TYPES.contains(&part_type) || start_lba == 0 {
            continue;
        }
        if fat_label_matches(ata, start_lba, label) {
            return Some(start_lba);
        }
    }
    None
}

/// Scans the four primary MBR entries for an ext4 volume carrying `label`.
fn find_ext4_by_label(ata: &AtaDrive, label: &[u8]) -> Option<u32> {
    for (part_type, start_lba) in mbr_partitions(ata) {
        if part_type != EXT4_PARTITION_TYPE || start_lba == 0 {
            continue;
        }
        if ext4_label_matches(ata, start_lba, label) {
            return Some(start_lba);
        }
    }
    None
}

fn mbr_partitions(ata: &AtaDrive) -> impl Iterator<Item = (u8, u32)> {
    let mut entries = [(0u8, 0u32); MBR_PARTITION_ENTRIES];
    let mut mbr = [0u8; 512];
    if !ata.read_sector(0, &mut mbr) || mbr[510] != 0x55 || mbr[511] != 0xAA {
        return entries.into_iter();
    }
    for (index, slot) in entries.iter_mut().enumerate() {
        let offset = MBR_PARTITION_OFFSET + index * 16;
        *slot = (
            mbr[offset + 4],
            u32::from_le_bytes([
                mbr[offset + 8],
                mbr[offset + 9],
                mbr[offset + 10],
                mbr[offset + 11],
            ]),
        );
    }
    entries.into_iter()
}

fn fat_label_matches(ata: &AtaDrive, start_lba: u32, label: &[u8]) -> bool {
    let mut boot = [0u8; 512];
    if !ata.read_sector(start_lba, &mut boot) {
        return false;
    }
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return false;
    }
    match volume_label(&boot) {
        Some(raw) => label_eq(&raw, label),
        None => false,
    }
}

fn ext4_label_matches(ata: &AtaDrive, start_lba: u32, label: &[u8]) -> bool {
    let mut superblock = [0u8; 512];
    let Some(sector) = start_lba.checked_add(2) else {
        return false;
    };
    if !ata.read_sector(sector, &mut superblock) {
        return false;
    }
    let magic = u16::from_le_bytes([superblock[0x38], superblock[0x39]]);
    if magic != EXT4_SUPERBLOCK_MAGIC {
        return false;
    }
    label_eq(&superblock[0x78..0x88], label)
}

/// Reads the 11-byte volume label from the BPB: offset 0x2B for FAT12/16,
/// 0x47 for FAT32 (where the FAT16 field is zero).
fn volume_label(boot: &[u8; 512]) -> Option<[u8; 11]> {
    let bytes_per_sector = u16::from_le_bytes([boot[0x0B], boot[0x0C]]);
    if bytes_per_sector != 512 {
        return None;
    }
    let fat16_size = u16::from_le_bytes([boot[0x16], boot[0x17]]);
    let offset = if fat16_size != 0 { 0x2B } else { 0x47 };
    let mut out = [0u8; 11];
    out.copy_from_slice(&boot[offset..offset + 11]);
    Some(out)
}

fn label_eq(raw: &[u8], want: &[u8]) -> bool {
    let raw = trim_padding(raw);
    let want = trim_padding(want);
    raw.len() == want.len() && raw.iter().zip(want).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn trim_padding(bytes: &[u8]) -> &[u8] {
    let mut len = bytes.len();
    while len > 0 && (bytes[len - 1] == b' ' || bytes[len - 1] == 0) {
        len -= 1;
    }
    &bytes[..len]
}
