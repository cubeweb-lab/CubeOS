use checks_host::fs_scan::{ext4_label, fat_label, label_eq, mbr_partitions, Partition};

fn mbr(entries: &[(u8, u32)], signature: [u8; 2]) -> [u8; 512] {
    let mut sector = [0u8; 512];
    for (index, (part_type, start_lba)) in entries.iter().enumerate() {
        let offset = 446 + index * 16;
        sector[offset + 4] = *part_type;
        sector[offset + 8..offset + 12].copy_from_slice(&start_lba.to_le_bytes());
    }
    sector[510..512].copy_from_slice(&signature);
    sector
}

#[test]
fn parses_mbr_partitions() {
    let sector = mbr(&[(0x0C, 2048), (0x83, 411648)], [0x55, 0xAA]);
    let entries = mbr_partitions(&sector);
    assert_eq!(entries[0], Partition { part_type: 0x0C, start_lba: 2048 });
    assert_eq!(entries[1], Partition { part_type: 0x83, start_lba: 411648 });
    assert_eq!(entries[2], Partition { part_type: 0, start_lba: 0 });
}

#[test]
fn rejects_mbr_without_signature() {
    let sector = mbr(&[(0x83, 411648)], [0, 0]);
    let entries = mbr_partitions(&sector);
    assert!(entries.iter().all(|entry| entry.start_lba == 0));
}

#[test]
fn reads_ext4_label_and_rejects_bad_magic() {
    let mut sector = [0u8; 512];
    sector[0x38] = 0x53;
    sector[0x39] = 0xEF;
    for (index, byte) in b"CUBEOS".iter().enumerate() {
        sector[0x78 + index] = *byte;
    }
    let label = ext4_label(&sector).unwrap();
    assert_eq!(label[..6], *b"CUBEOS");

    sector[0x38] = 0;
    assert!(ext4_label(&sector).is_none());
}

#[test]
fn reads_fat32_and_fat16_volume_labels() {
    let mut fat32 = [0u8; 512];
    fat32[0x0B] = 0x00;
    fat32[0x0C] = 0x02;
    for (index, byte) in b"CUBEBOOT".iter().enumerate() {
        fat32[0x47 + index] = *byte;
    }
    assert_eq!(fat_label(&fat32).unwrap()[..8], *b"CUBEBOOT");

    let mut fat16 = [0u8; 512];
    fat16[0x0B] = 0x00;
    fat16[0x0C] = 0x02;
    fat16[0x16] = 0x20;
    for (index, byte) in b"CUBEOS  ".iter().enumerate() {
        fat16[0x2B + index] = *byte;
    }
    assert_eq!(fat_label(&fat16).unwrap()[..8], *b"CUBEOS  ");
}

#[test]
fn label_comparison_ignores_case_and_padding() {
    let mut raw = [0u8; 16];
    raw[..6].copy_from_slice(b"cubeos");
    assert!(label_eq(&raw, b"CUBEOS"));
    assert!(!label_eq(&raw, b"NOPE"));
}
