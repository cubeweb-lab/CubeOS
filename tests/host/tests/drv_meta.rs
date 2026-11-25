use checks_host::drv_meta::{
    name, parse, section, DrvMeta, DRV_ABI_VERSION, DRV_FLAG_KERNEL, DRV_MAGIC, DRV_META_VERSION,
    DRV_SECTION,
};

fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn padded<const N: usize>(text: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out[..text.len()].copy_from_slice(text);
    out
}

fn driver_elf(flags: u32) -> [u8; 576] {
    let mut data = [0u8; 576];
    data[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    write_u64(&mut data, 0x28, 256);
    write_u16(&mut data, 0x3A, 64);
    write_u16(&mut data, 0x3C, 3);
    write_u16(&mut data, 0x3E, 2);

    let meta = DrvMeta {
        magic: DRV_MAGIC,
        version: DRV_META_VERSION,
        abi: DRV_ABI_VERSION,
        flags,
        name: padded(b"demo"),
        description: padded(b"demo driver"),
        reserved: [0; 12],
    };
    unsafe {
        core::ptr::write_unaligned(data.as_mut_ptr().add(64) as *mut DrvMeta, meta);
    }

    let strings = b"\0.drvmeta\0.shstrtab\0";
    data[192..192 + strings.len()].copy_from_slice(strings);
    write_u32(&mut data, 256 + 64, 1);
    write_u64(&mut data, 256 + 64 + 24, 64);
    write_u64(&mut data, 256 + 64 + 32, 128);
    write_u32(&mut data, 256 + 128, 10);
    write_u64(&mut data, 256 + 128 + 24, 192);
    write_u64(&mut data, 256 + 128 + 32, strings.len() as u64);
    data
}

#[test]
fn finds_drvmeta_section() {
    let data = driver_elf(0);
    assert_eq!(section(&data, DRV_SECTION), Some((64, 128)));
    assert_eq!(section(&data, b".missing"), None);
}

#[test]
fn parses_driver_metadata() {
    let data = driver_elf(0);
    let meta = parse(&data[64..192]).unwrap();
    assert_eq!(name(&meta), "demo");
    assert_eq!(meta.version, DRV_META_VERSION);
    assert_eq!(meta.abi, DRV_ABI_VERSION);
}

#[test]
fn rejects_bad_metadata() {
    let mut data = driver_elf(0);
    data[64] = 0;
    assert!(parse(&data[64..192]).is_none());

    let data = driver_elf(DRV_FLAG_KERNEL);
    assert!(parse(&data[64..192]).is_none());
}

#[test]
fn rejects_elf_without_sections() {
    let data = [0u8; 64];
    assert!(section(&data, DRV_SECTION).is_none());
}
