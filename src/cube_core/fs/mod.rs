// src/cube_core/fs/mod.rs
// File system front end: finds the root FAT volume in the MBR and forwards file operations to the Zig driver.
use crate::ata::AtaDrive;
use crate::serial;
use crate::zigfs;

// MBR partition types of FAT12, FAT16 (<32 MB, >32 MB) and FAT32 (CHS and LBA)
const FAT_PARTITION_TYPES: [u8; 6] = [0x01, 0x04, 0x06, 0x0B, 0x0C, 0x0E];
const MBR_PARTITION_OFFSET: usize = 446;
const MBR_PARTITION_ENTRIES: usize = 4;

pub struct FSManager {
    partition_lba: Option<u32>,
}

impl FSManager {
    pub const fn new() -> Self {
        Self { partition_lba: None }
    }

    /// Mounts the FAT partition whose volume label matches `label` (case-insensitive,
    /// trailing spaces ignored). This makes the root volume independent of MBR order.
    pub fn mount_label(&mut self, ata: &AtaDrive, label: &[u8]) -> bool {
        let lba = match find_partition_by_label(ata, label) {
            Some(lba) => lba,
            None => {
                serial::send_string("[FS] No FAT volume with the requested label\n");
                return false;
            }
        };
        match zigfs::mount(lba) {
            Some(kind) => {
                serial::send_string("[FS] ");
                serial::send_string(kind);
                serial::send_string(" root volume mounted\n");
                self.partition_lba = Some(lba);
                true
            }
            None => false,
        }
    }

    /// Reads a whole file given by an absolute path such as "/SYSTEM/CONFIG/BOOT.CFG".
    pub fn read_file(&self, path: &str, buf: &mut [u8]) -> Option<usize> {
        self.partition_lba?;
        zigfs::read(0, path.as_bytes(), 0, buf)
    }
}

/// Scans the four primary MBR entries for a FAT volume carrying `label`.
fn find_partition_by_label(ata: &AtaDrive, label: &[u8]) -> Option<u32> {
    let mut mbr = [0u8; 512];
    if !ata.read_sector(0, &mut mbr) {
        return None;
    }
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return None;
    }

    for index in 0..MBR_PARTITION_ENTRIES {
        let offset = MBR_PARTITION_OFFSET + index * 16;
        let part_type = mbr[offset + 4];
        if !FAT_PARTITION_TYPES.contains(&part_type) {
            continue;
        }
        let start_lba = u32::from_le_bytes([
            mbr[offset + 8], mbr[offset + 9], mbr[offset + 10], mbr[offset + 11],
        ]);
        if start_lba == 0 {
            continue;
        }
        if partition_label_matches(ata, start_lba, label) {
            return Some(start_lba);
        }
    }
    None
}

fn partition_label_matches(ata: &AtaDrive, start_lba: u32, label: &[u8]) -> bool {
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

fn label_eq(raw: &[u8; 11], want: &[u8]) -> bool {
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
