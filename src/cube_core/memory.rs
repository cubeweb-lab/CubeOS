use crate::multiboot::MemoryMapEntry;
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

extern "C" {
    static _kernel_end: u8;
}

/// Base of the higher-half direct map. Physical address `p` is reachable at
/// `PHYS_MAP_BASE + p` in every address space. Kept in sync with the PML4[256]
/// entry installed by `boot.asm`.
pub const PHYS_MAP_BASE: u64 = 0xFFFF_8000_0000_0000;

pub const PAGE_SIZE: usize = 4096;

/// Translates a physical address into the shared kernel direct map.
pub fn phys_to_virt(phys: u64) -> u64 {
    PHYS_MAP_BASE.wrapping_add(phys)
}

/// Translates an address from the kernel direct map back to physical memory.
pub fn virt_to_phys(virt: u64) -> u64 {
    virt.wrapping_sub(PHYS_MAP_BASE)
}

pub fn frame_containing(phys: u64) -> PhysFrame<Size4KiB> {
    PhysFrame::containing_address(PhysAddr::new(phys))
}

pub fn virt_to_frame(virt: u64) -> PhysFrame<Size4KiB> {
    frame_containing(virt_to_phys(virt))
}

const MAX_PAGES: usize = 1024 * 1024;
const BITMAP_WORDS: usize = MAX_PAGES / 64;

static mut BITMAP: [u64; BITMAP_WORDS] = [0; BITMAP_WORDS];
static mut TOTAL_PAGES: usize = 0;
static mut FREE_PAGES: usize = 0;

fn page_bit(page: usize) -> (usize, u64) {
    (page / 64, 1u64 << (page % 64))
}

pub fn init(memory_map: &[MemoryMapEntry]) {
    unsafe {
        let mut total_pages: usize = 0;
        let mut free_pages: usize = 0;

        for entry in memory_map {
            if entry.typ != 1 {
                continue;
            }
            let start_page = (entry.base_addr / PAGE_SIZE as u64) as usize;
            let end_page = (entry.base_addr.saturating_add(entry.length) / PAGE_SIZE as u64) as usize;
            let end_page = end_page.min(MAX_PAGES);
            total_pages = total_pages.max(end_page);

            for page in start_page..end_page {
                let (word, bit) = page_bit(page);
                if BITMAP[word] & bit == 0 {
                    BITMAP[word] |= bit;
                    free_pages += 1;
                }
            }
        }

        // Everything below the kernel image end (low memory, page tables, kernel, stack) is never allocatable.
        let kernel_end = &_kernel_end as *const u8 as usize;
        let reserved_pages = (kernel_end + PAGE_SIZE - 1) / PAGE_SIZE;
        for page in 0..reserved_pages.min(MAX_PAGES) {
            let (word, bit) = page_bit(page);
            if BITMAP[word] & bit != 0 {
                BITMAP[word] &= !bit;
                free_pages -= 1;
            }
        }

        TOTAL_PAGES = total_pages;
        FREE_PAGES = free_pages;
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

/// Allocates one zeroed 4 KiB frame. The frame is returned as a physical
/// address; callers that touch it through the CPU must use [`phys_to_virt`].
/// Only page tables and `CR3` consume the raw physical address.
pub fn allocate_frame() -> Option<PhysFrame<Size4KiB>> {
    unsafe {
        for word in 0..BITMAP_WORDS {
            let bits = BITMAP[word];
            if bits == 0 {
                continue;
            }
            let bit = bits.trailing_zeros() as usize;
            let page = word * 64 + bit;
            BITMAP[word] &= !(1u64 << bit);
            FREE_PAGES -= 1;

            let addr = PhysAddr::new((page * PAGE_SIZE) as u64);
            core::ptr::write_bytes(phys_to_virt(addr.as_u64()) as *mut u8, 0, PAGE_SIZE);
            return Some(PhysFrame::containing_address(addr));
        }
        None
    }
}

pub fn deallocate_frame(frame: PhysFrame<Size4KiB>) {
    let page = (frame.start_address().as_u64() / PAGE_SIZE as u64) as usize;
    if page >= MAX_PAGES {
        return;
    }
    unsafe {
        let (word, bit) = page_bit(page);
        if BITMAP[word] & bit == 0 {
            BITMAP[word] |= bit;
            FREE_PAGES += 1;
        }
    }
}

/// Zero-sized adapter so `x86_64`'s paging `Mapper` can pull frames from the bitmap PMM.
pub struct BitmapFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        allocate_frame()
    }
}
