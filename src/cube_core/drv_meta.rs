// Pure ELF section lookup and .drv metadata validation. Shared by the kernel
// driver loader and host tests; no allocation and no kernel dependencies.

pub const DRV_MAGIC: [u8; 8] = *b"CUBEDRV\0";
pub const DRV_META_VERSION: u32 = 1;
pub const DRV_ABI_VERSION: u32 = 1;
pub const DRV_FLAG_KERNEL: u32 = 1;
pub const DRV_SECTION: &[u8] = b".drvmeta";

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrvMeta {
    pub magic: [u8; 8],
    pub version: u32,
    pub abi: u32,
    pub flags: u32,
    pub name: [u8; 32],
    pub description: [u8; 64],
    pub reserved: [u8; 12],
}

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn u64_at(data: &[u8], offset: usize) -> Option<u64> {
    let bytes: [u8; 8] = data.get(offset..offset + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn section_header(data: &[u8], shoff: usize, entsize: usize, index: usize) -> Option<(u32, usize, usize)> {
    let base = shoff.checked_add(index.checked_mul(entsize)?)?;
    let _ = data.get(base..base + 64)?;
    let name = u32_at(data, base)?;
    let offset = u64_at(data, base + 24)? as usize;
    let size = u64_at(data, base + 32)? as usize;
    let _ = data.get(offset..offset.checked_add(size)?)?;
    Some((name, offset, size))
}

fn section_name(data: &[u8], strtab: (u32, usize, usize), name_offset: u32) -> Option<&[u8]> {
    let start = strtab.1.checked_add(name_offset as usize)?;
    let end = strtab.1.checked_add(strtab.2)?;
    let bytes = data.get(start..end)?;
    let len = bytes.iter().position(|&b| b == 0)?;
    Some(&bytes[..len])
}

/// Returns the file offset and size of the named ELF section.
pub fn section(data: &[u8], wanted: &[u8]) -> Option<(usize, usize)> {
    if data.len() < 64 || data[0..4] != [0x7F, b'E', b'L', b'F'] {
        return None;
    }
    let shoff = u64_at(data, 0x28)? as usize;
    let entsize = u16_at(data, 0x3A)? as usize;
    let shnum = u16_at(data, 0x3C)? as usize;
    let shstrndx = u16_at(data, 0x3E)? as usize;
    if entsize < 64 || shnum == 0 || shstrndx >= shnum {
        return None;
    }
    let strtab = section_header(data, shoff, entsize, shstrndx)?;
    for index in 0..shnum {
        let (name, offset, size) = section_header(data, shoff, entsize, index)?;
        if section_name(data, strtab, name)? == wanted {
            return Some((offset, size));
        }
    }
    None
}

/// Validates a `.drvmeta` section body and returns its parsed header.
pub fn parse(bytes: &[u8]) -> Option<DrvMeta> {
    if bytes.len() < core::mem::size_of::<DrvMeta>() {
        return None;
    }
    let meta = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const DrvMeta) };
    if meta.magic != DRV_MAGIC
        || meta.version != DRV_META_VERSION
        || meta.abi != DRV_ABI_VERSION
        || meta.flags & DRV_FLAG_KERNEL != 0
        || meta.name[0] == 0
        || !meta.name.iter().any(|&b| b == 0)
    {
        return None;
    }
    Some(meta)
}

/// Returns the NUL-terminated driver name from a validated metadata block.
pub fn name(meta: &DrvMeta) -> &str {
    let len = meta.name.iter().position(|&b| b == 0).unwrap_or(meta.name.len());
    core::str::from_utf8(&meta.name[..len]).unwrap_or("")
}
