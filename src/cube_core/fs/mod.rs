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

    /// Selects the root volume by probing every filesystem in the MBR for
    /// `/system/config/boot.cfg`. No label or partition type is required.
    pub fn mount_root(&mut self, ata: &AtaDrive) -> bool {
        let mut probe = [0u8; 64];
        if crate::ramdisk::is_present() && ext4fs::mount(0) {
            unsafe { KIND = Kind::Ext4 };
            if read(0, b"/system/config/boot.cfg", 0, &mut probe).is_some() {
                serial::send_string("[FS] root volume selected: RAM ext4\n");
                return true;
            }
        }
        let Some(candidates) = candidates(ata) else {
            serial::send_string("[FS] no readable MBR\n");
            return false;
        };
        for candidate in candidates.iter().take_while(|c| c.kind != Kind::None) {
            let mounted = match candidate.kind {
                Kind::Ext4 => ext4fs::mount(candidate.lba),
                Kind::Fat => zigfs::mount(candidate.lba).is_some(),
                Kind::None => false,
            };
            if !mounted {
                continue;
            }
            unsafe { KIND = candidate.kind };
            if read(0, b"/system/config/boot.cfg", 0, &mut probe).is_some() {
                serial::send_string("[FS] root volume selected: ");
                serial::send_string(match candidate.kind {
                    Kind::Ext4 => "ext4",
                    Kind::Fat => "FAT",
                    Kind::None => "none",
                });
                serial::send_string("\n");
                return true;
            }
        }
        unsafe { KIND = Kind::None };
        serial::send_string("[FS] no volume contains /system/config/boot.cfg\n");
        false
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

#[derive(Clone, Copy)]
struct Candidate {
    lba: u32,
    kind: Kind,
}

/// Collects every mountable volume from the MBR, ext4 first, FAT second.
fn candidates(ata: &AtaDrive) -> Option<[Candidate; scan::MBR_PARTITION_ENTRIES]> {
    let partitions = read_partitions(ata)?;
    let mut out = [Candidate { lba: 0, kind: Kind::None }; scan::MBR_PARTITION_ENTRIES];
    let mut count = 0;
    for part in partitions {
        if part.start_lba == 0 || count == out.len() {
            continue;
        }
        let Some(sb_lba) = part.start_lba.checked_add(2) else {
            continue;
        };
        let mut sector = [0u8; 512];
        if ata.read_sector(sb_lba, &mut sector) && scan::ext4_label(&sector).is_some() {
            out[count] = Candidate { lba: part.start_lba, kind: Kind::Ext4 };
            count += 1;
            continue;
        }
        if !FAT_PARTITION_TYPES.contains(&part.part_type) {
            continue;
        }
        if ata.read_sector(part.start_lba, &mut sector)
            && sector[510] == 0x55
            && sector[511] == 0xAA
            && scan::fat_label(&sector).is_some()
        {
            out[count] = Candidate { lba: part.start_lba, kind: Kind::Fat };
            count += 1;
        }
    }
    Some(out)
}

fn read_partitions(ata: &AtaDrive) -> Option<[scan::Partition; scan::MBR_PARTITION_ENTRIES]> {
    let mut mbr = [0u8; 512];
    if !ata.read_sector(0, &mut mbr) || mbr[510] != 0x55 || mbr[511] != 0xAA {
        return None;
    }
    Some(scan::mbr_partitions(&mbr))
}
