// src/cube_core/fs/mod.rs
// File system front end: finds the FAT partition in the MBR and forwards file operations to the Zig driver.
use crate::ata::AtaDrive;
use crate::serial;
use crate::zigfs;

// MBR partition types of FAT12, FAT16 (<32 MB, >32 MB) and FAT32 (CHS and LBA)
const FAT_PARTITION_TYPES: [u8; 6] = [0x01, 0x04, 0x06, 0x0B, 0x0C, 0x0E];

pub struct FSManager {
    partition_lba: Option<u32>,
}

impl FSManager {
    pub const fn new() -> Self {
        Self { partition_lba: None }
    }

    /// Mounts the FAT partition with index `partition_index` (0 = first MBR entry).
    pub fn mount(&mut self, ata: &AtaDrive, partition_index: usize) -> bool {
        let lba = match partition_start(ata, partition_index) {
            Some(lba) => lba,
            None => return false,
        };
        match zigfs::mount(lba) {
            Some(kind) => {
                serial::send_string("[FS] ");
                serial::send_string(kind);
                serial::send_string(" volume mounted\n");
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

/// Returns the first LBA of the FAT partition at `partition_index` in the MBR.
fn partition_start(ata: &AtaDrive, partition_index: usize) -> Option<u32> {
    let mut mbr = [0u8; 512];
    if !ata.read_sector(0, &mut mbr) {
        return None;
    }
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return None;
    }

    let offset = 446 + partition_index * 16;
    if offset + 16 > 512 {
        return None;
    }
    let part_type = mbr[offset + 4];
    if !FAT_PARTITION_TYPES.contains(&part_type) {
        return None;
    }
    let start_lba = u32::from_le_bytes([mbr[offset + 8], mbr[offset + 9], mbr[offset + 10], mbr[offset + 11]]);
    if start_lba == 0 {
        return None;
    }
    Some(start_lba)
}
