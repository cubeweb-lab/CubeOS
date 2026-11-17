use crate::multiboot::MemoryMapEntry;

extern "C" {
    static _kernel_end: u8;
}

/// Base of the higher-half direct map. Physical address `p` is reachable at
/// `PHYS_MAP_BASE + p` in every address space. Kept in sync with the PML4[256]
/// entry installed by `boot.asm`.
pub const PHYS_MAP_BASE: u64 = 0xFFFF_8000_0000_0000;

/// Translates a physical address into the shared kernel direct map.
pub fn phys_to_virt(phys: u64) -> u64 {
    PHYS_MAP_BASE.wrapping_add(phys)
}

/// Translates an address from the kernel direct map back to physical memory.
pub fn virt_to_phys(virt: u64) -> u64 {
    virt.wrapping_sub(PHYS_MAP_BASE)
}

const MAX_PAGES: usize = 1024 * 1024;
const PAGE_SIZE: usize = 4096;

static mut BITMAP: [u64; MAX_PAGES / 64] = [0; MAX_PAGES / 64];
static mut TOTAL_PAGES: usize = 0;
static mut FREE_PAGES: usize = 0;

pub fn init(memory_map: &[MemoryMapEntry]) {
    unsafe {
        let mut _total_bytes: u64 = 0;
        let mut free_bytes: u64 = 0;

        for entry in memory_map {
            _total_bytes += entry.length;
            if entry.typ == 1 {
                let start_page = (entry.base_addr / PAGE_SIZE as u64) as usize;
                let end_page = ((entry.base_addr + entry.length) / PAGE_SIZE as u64) as usize;

                for page in start_page..end_page.min(MAX_PAGES) {
                    let idx = page / 64;
                    let bit = page % 64;
                    BITMAP[idx] |= 1 << bit;
                    free_bytes += PAGE_SIZE as u64;
                }
            }
        }

        TOTAL_PAGES = MAX_PAGES;
        FREE_PAGES = (free_bytes / PAGE_SIZE as u64) as usize;

        // Everything below the kernel image end (low memory, page tables, kernel, stack) is never allocatable.
        let kernel_end = &_kernel_end as *const u8 as usize;
        let reserved_pages = ((kernel_end + PAGE_SIZE - 1) / PAGE_SIZE).min(MAX_PAGES);
        for page in 0..reserved_pages {
            let idx = page / 64;
            let bit = page % 64;
            if BITMAP[idx] & (1 << bit) != 0 {
                BITMAP[idx] &= !(1 << bit);
                FREE_PAGES = FREE_PAGES.saturating_sub(1);
            }
        }
    }
}

pub fn total_memory_mb() -> u64 {
    unsafe { (TOTAL_PAGES * PAGE_SIZE / 1024 / 1024) as u64 }
}

pub fn free_memory_mb() -> u64 {
    unsafe { (FREE_PAGES * PAGE_SIZE / 1024 / 1024) as u64 }
}

pub fn free_pages() -> usize {
    unsafe { FREE_PAGES }
}

/// Allocates one zeroed page and returns its physical address. Callers that
/// touch the page through the CPU must use [`phys_to_virt`]; only page tables
/// and `CR3` consume the raw physical address.
pub fn alloc_page() -> Option<u64> {
    unsafe {
        for i in 0..(MAX_PAGES / 64) {
            if BITMAP[i] != 0 {
                let bit = BITMAP[i].trailing_zeros() as usize;
                let page = i * 64 + bit;
                BITMAP[i] &= !(1 << bit);
                FREE_PAGES -= 1;
                return Some((page * PAGE_SIZE) as u64);
            }
        }
        None
    }
}

pub fn free_page(addr: u64) {
    let page = (addr / PAGE_SIZE as u64) as usize;
    if page >= MAX_PAGES { return; }
    unsafe {
        let idx = page / 64;
        let bit = page % 64;
        if BITMAP[idx] & (1 << bit) == 0 {
            BITMAP[idx] |= 1 << bit;
            FREE_PAGES += 1;
        }
    }
}
