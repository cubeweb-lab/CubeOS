// Pure MBR/superblock parsing shared by the kernel FS front end and host tests.

pub const MBR_PARTITION_OFFSET: usize = 446;
pub const MBR_PARTITION_ENTRIES: usize = 4;
pub const EXT4_SUPERBLOCK_MAGIC: u16 = 0xEF53;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    pub part_type: u8,
    pub start_lba: u32,
}

/// Parses the four primary MBR entries. An invalid signature yields empty entries.
pub fn mbr_partitions(mbr: &[u8; 512]) -> [Partition; MBR_PARTITION_ENTRIES] {
    let mut entries = [Partition { part_type: 0, start_lba: 0 }; MBR_PARTITION_ENTRIES];
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return entries;
    }
    for (index, slot) in entries.iter_mut().enumerate() {
        let offset = MBR_PARTITION_OFFSET + index * 16;
        *slot = Partition {
            part_type: mbr[offset + 4],
            start_lba: u32::from_le_bytes([
                mbr[offset + 8],
                mbr[offset + 9],
                mbr[offset + 10],
                mbr[offset + 11],
            ]),
        };
    }
    entries
}

/// Returns the 16-byte volume label from an ext4 superblock sector, or `None`
/// when the superblock magic is missing.
pub fn ext4_label(sector: &[u8; 512]) -> Option<[u8; 16]> {
    let magic = u16::from_le_bytes([sector[0x38], sector[0x39]]);
    if magic != EXT4_SUPERBLOCK_MAGIC {
        return None;
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&sector[0x78..0x88]);
    Some(out)
}

/// Returns the 11-byte volume label from a FAT boot sector.
pub fn fat_label(boot: &[u8; 512]) -> Option<[u8; 11]> {
    if u16::from_le_bytes([boot[0x0B], boot[0x0C]]) != 512 {
        return None;
    }
    let fat16_size = u16::from_le_bytes([boot[0x16], boot[0x17]]);
    let offset = if fat16_size != 0 { 0x2B } else { 0x47 };
    let mut out = [0u8; 11];
    out.copy_from_slice(&boot[offset..offset + 11]);
    Some(out)
}

/// Case-insensitive comparison ignoring trailing spaces and NUL padding.
pub fn label_eq(raw: &[u8], want: &[u8]) -> bool {
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
