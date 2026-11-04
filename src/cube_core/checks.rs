// Pure validation helpers with no hardware access, so they can be unit tested on the host.

pub fn range_fits(offset: u64, len: u64, limit: u64) -> bool {
    match offset.checked_add(len) {
        Some(end) => end <= limit,
        None => false,
    }
}

pub fn in_segment(addr: u64, start: u64, len: u64) -> bool {
    match addr.checked_sub(start) {
        Some(off) => off < len,
        None => false,
    }
}

// File bytes must lie inside the file, and the whole memory image must lie inside the load region.
pub fn elf_segment_fits(
    vaddr: u64,
    file_offset: u64,
    filesz: u64,
    memsz: u64,
    file_len: u64,
    region_start: u64,
    region_end: u64,
) -> bool {
    filesz <= memsz
        && range_fits(file_offset, filesz, file_len)
        && vaddr >= region_start
        && range_fits(vaddr, memsz, region_end)
}

pub fn cluster_in_range(cluster: u32, cluster_count: u32) -> bool {
    cluster >= 2 && cluster - 2 < cluster_count
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FatLayout {
    pub data_start_rel: u32,
    pub cluster_count: u32,
}

const FAT32_MAX_CLUSTERS: u64 = 0x0FFF_FFF5;
const MAX_CLUSTER_BYTES: u64 = 32768;

// Rejects geometry that would make the FAT or data region point outside the volume.
pub fn fat_layout(
    total_sectors: u32,
    reserved: u16,
    num_fats: u8,
    fat_size: u32,
    spc: u8,
    root_cluster: u32,
) -> Option<FatLayout> {
    if spc == 0 || !spc.is_power_of_two() || spc as u64 * 512 > MAX_CLUSTER_BYTES {
        return None;
    }
    if reserved == 0 || num_fats == 0 || fat_size == 0 {
        return None;
    }
    let data_start = reserved as u64 + num_fats as u64 * fat_size as u64;
    let total = total_sectors as u64;
    if data_start >= total {
        return None;
    }
    let cluster_count = (total - data_start) / spc as u64;
    if cluster_count < 2 || cluster_count > FAT32_MAX_CLUSTERS {
        return None;
    }
    let cluster_count = cluster_count as u32;
    if !cluster_in_range(root_cluster, cluster_count) {
        return None;
    }
    Some(FatLayout {
        data_start_rel: data_start as u32,
        cluster_count,
    })
}

pub fn mmap_entry_count(tag_size: usize, header_len: usize, entry_size: usize, min_entry_size: usize) -> usize {
    if entry_size < min_entry_size || tag_size < header_len {
        return 0;
    }
    (tag_size - header_len) / entry_size
}

// Lines are split with lines(), which strips a trailing '\r', so CRLF files never produce a slice with start > end.
pub fn config_cmdline(text: &str) -> Option<&str> {
    let mut cmdline = None;
    for raw in text.lines() {
        let line = raw.trim();
        if !line.starts_with("cmdline") {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let value = line[eq + 1..].trim().trim_matches('"');
            if !value.is_empty() {
                cmdline = Some(value);
            }
        }
    }
    cmdline
}

pub fn config_apic_logs(text: &str) -> Option<&str> {
    let mut apic_logs = None;
    for raw in text.lines() {
        let line = raw.trim();
        if !line.starts_with("apic_logs") {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let value = line[eq + 1..].trim().trim_matches('"');
            if !value.is_empty() {
                apic_logs = Some(value);
            }
        }
    }
    apic_logs
}

pub const PAGE_SIZE: u64 = 4096;
pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;
pub const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// Level 4 is the PML4; each level indexes 512 entries of 8 bytes.
pub fn page_table_index(virt: u64, level: u32) -> usize {
    ((virt >> (12 + 9 * (level - 1))) & 0x1FF) as usize
}

// Returns the page-aligned [first, end) range that covers `len` bytes starting at `start`.
pub fn page_span(start: u64, len: u64) -> Option<(u64, u64)> {
    let end = start.checked_add(len)?;
    let first = start & !(PAGE_SIZE - 1);
    let last = end.checked_add(PAGE_SIZE - 1)? & !(PAGE_SIZE - 1);
    Some((first, last))
}
