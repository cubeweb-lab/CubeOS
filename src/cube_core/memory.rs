use crate::multiboot::MemoryMapEntry;
use core::ptr;
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

extern "C" {
    static _kernel_end: u8;
}

/// Base of the higher-half direct map. Physical address `p` is reachable at
/// `PHYS_MAP_BASE + p` in every address space. `boot.asm` installs the first
/// 4 GiB; [`init`] extends the same PML4/PDPT tree to cover all detected RAM.
pub const PHYS_MAP_BASE: u64 = 0xFFFF_8000_0000_0000;

pub const PAGE_SIZE: usize = 4096;

const GIB: u64 = 1 << 30;
const BOOT_MAPPED: u64 = 4 * GIB;
/// Sanity ceiling for the direct map / bitmap sizing (128 PML4 slots).
const MAX_PHYS: u64 = 64 * 1024 * GIB;

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

/// Number of 64-bit words in the frame bitmap. The bitmap itself lives right
/// after the kernel image and is sized from the detected top of RAM.
static mut BITMAP: *mut u64 = ptr::null_mut();
static mut BITMAP_WORDS: usize = 0;
static mut TOTAL_PAGES: usize = 0;
static mut FREE_PAGES: usize = 0;
/// Highest physical address currently reachable through the direct map.
static mut MAPPED_LIMIT: u64 = BOOT_MAPPED;

fn page_bit(page: usize) -> (usize, u64) {
    (page / 64, 1u64 << (page % 64))
}

/// Current end (exclusive) of the direct map in physical-address space.
pub fn mapped_limit() -> u64 {
    unsafe { MAPPED_LIMIT }
}

pub fn set_mapped_limit(limit: u64) {
    unsafe { MAPPED_LIMIT = limit };
}

pub fn init(memory_map: &[MemoryMapEntry]) {
    unsafe {
        let mut max_phys = BOOT_MAPPED;
        for entry in memory_map {
            if entry.typ != 1 {
                continue;
            }
            max_phys = max_phys.max(entry.base_addr.saturating_add(entry.length));
        }

        let kernel_end = &_kernel_end as *const u8 as usize as u64;
        max_phys = max_phys.max(kernel_end).min(MAX_PHYS);
        let max_pages = ((max_phys + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64) as usize;

        // The boot stack occupies the 0x40000 bytes below `_kernel_end` and
        // grows down, so the bitmap can start exactly there.
        let bitmap_addr = (&_kernel_end as *const u8 as usize + 7) & !7usize;
        let words = (max_pages + 63) / 64;
        let bitmap = bitmap_addr as *mut u64;

        BITMAP = bitmap;
        BITMAP_WORDS = words;
        MAPPED_LIMIT = BOOT_MAPPED;

        ptr::write_bytes(bitmap as *mut u8, 0, words * 8);

        let mut total_pages: usize = 0;
        let mut free_pages: usize = 0;

        for entry in memory_map {
            if entry.typ != 1 {
                continue;
            }
            let start_page = (entry.base_addr / PAGE_SIZE as u64) as usize;
            let end_page =
                ((entry.base_addr.saturating_add(entry.length) / PAGE_SIZE as u64) as usize)
                    .min(max_pages);
            total_pages = total_pages.max(end_page);

            for page in start_page..end_page {
                let (word, bit) = page_bit(page);
                if *BITMAP.add(word) & bit == 0 {
                    *BITMAP.add(word) |= bit;
                    free_pages += 1;
                }
            }
        }

        // Kernel image, boot stack and the PMM bitmap itself are never allocatable.
        let bitmap_end = bitmap_addr + words * 8;
        let reserved_pages = ((bitmap_end + PAGE_SIZE - 1) / PAGE_SIZE).min(max_pages);
        for page in 0..reserved_pages {
            let (word, bit) = page_bit(page);
            if *BITMAP.add(word) & bit != 0 {
                *BITMAP.add(word) &= !bit;
                free_pages -= 1;
            }
        }

        TOTAL_PAGES = total_pages;
        FREE_PAGES = free_pages;

        // Frames above the boot mapping can only be allocated once they are
        // reachable through the direct map.
        crate::vmm::extend_direct_map(max_phys);
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
/// address; callers that touch the page through the CPU must use [`phys_to_virt`].
/// Only page tables and `CR3` consume the raw physical address.
pub fn allocate_frame() -> Option<PhysFrame<Size4KiB>> {
    unsafe {
        for word in 0..BITMAP_WORDS {
            let bits = *BITMAP.add(word);
            if bits == 0 {
                continue;
            }
            let bit = bits.trailing_zeros() as usize;
            let page = word * 64 + bit;
            let phys = page as u64 * PAGE_SIZE as u64;
            // The scan is ascending, so every later free page is also above
            // the direct map during boot-time extension.
            if phys >= MAPPED_LIMIT {
                return None;
            }
            *BITMAP.add(word) &= !(1u64 << bit);
            FREE_PAGES -= 1;

            ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, PAGE_SIZE);
            return Some(PhysFrame::containing_address(PhysAddr::new(phys)));
        }
        None
    }
}

pub fn deallocate_frame(frame: PhysFrame<Size4KiB>) {
    unsafe {
        let page = (frame.start_address().as_u64() / PAGE_SIZE as u64) as usize;
        if page >= BITMAP_WORDS * 64 {
            return;
        }
        let (word, bit) = page_bit(page);
        if *BITMAP.add(word) & bit == 0 {
            *BITMAP.add(word) |= bit;
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
