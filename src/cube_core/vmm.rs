use crate::memory::{self, PHYS_MAP_BASE};
use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::page_table::PageTableEntry;
use x86_64::structures::paging::{
    Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

pub const USER_BASE: u64 = 0x0000_0000_0040_0000;
pub const USER_LIMIT: u64 = 0x0000_8000_0000_0000;

const ENTRIES: usize = 512;
const PD_SIZE: u64 = 0x20_0000;
const GIB: u64 = 1 << 30;
const DIRECT_MAP_SLOT: usize = ((PHYS_MAP_BASE >> 39) & 0x1FF) as usize;
/// PML4 slots below this cover user space and stay private per process.
const USER_SLOTS: usize = (USER_LIMIT >> 39) as usize;
/// Number of 2 MiB kernel identity entries kept below `USER_BASE`.
const KERNEL_LOW_PD_ENTRIES: usize = (USER_BASE / PD_SIZE) as usize;

const _: () = assert!(USER_BASE % PD_SIZE == 0);
const _: () = assert!(USER_BASE < USER_LIMIT);

static KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

fn table_flags() -> PageTableFlags {
    PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE
}

fn table_ptr(frame: PhysFrame<Size4KiB>) -> *mut PageTable {
    memory::phys_to_virt(frame.start_address().as_u64()) as *mut PageTable
}

fn read_entry(frame: PhysFrame<Size4KiB>, idx: usize) -> PageTableEntry {
    unsafe { (&*table_ptr(frame))[idx].clone() }
}

fn write_entry(frame: PhysFrame<Size4KiB>, idx: usize, entry: PageTableEntry) {
    unsafe { (&mut *table_ptr(frame))[idx] = entry };
}

pub fn init() {
    let (frame, _) = Cr3::read();
    KERNEL_PML4.store(frame.start_address().as_u64(), Ordering::Relaxed);
}

/// Maps `pages` freshly allocated 4 KiB frames at `virt_base` in the kernel
/// address space. The kernel heap uses this instead of a static array: user
/// address spaces only identity-map the first 2 MiB of the kernel, while the
/// higher half is shared verbatim.
pub fn map_kernel_region(virt_base: u64, pages: usize) -> bool {
    let pml4 = kernel_pml4();
    let mut mapper = unsafe {
        OffsetPageTable::new(&mut *table_ptr(pml4), VirtAddr::new(PHYS_MAP_BASE))
    };
    for i in 0..pages {
        let Some(frame) = memory::allocate_frame() else {
            return false;
        };
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(
            virt_base + i as u64 * memory::PAGE_SIZE as u64,
        ));
        match unsafe {
            mapper.map_to(
                page,
                frame,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                &mut memory::BitmapFrameAllocator,
            )
        } {
            Ok(flush) => flush.flush(),
            Err(_) => {
                memory::deallocate_frame(frame);
                return false;
            }
        }
    }
    true
}

/// Extends the shared direct map from the boot-mapped 4 GiB up to `max_phys`
/// using 2 MiB pages. Runs once from [`crate::memory::init`] while only frames
/// below the current mapping can be allocated.
pub fn extend_direct_map(max_phys: u64) {
    let mapped = memory::mapped_limit();
    if max_phys <= mapped {
        return;
    }

    let (pml4_frame, _) = Cr3::read();
    let pml4_phys = pml4_frame.start_address().as_u64();
    let pml4 = unsafe { &mut *(memory::phys_to_virt(pml4_phys) as *mut PageTable) };
    let mut pdpt = match pml4[DIRECT_MAP_SLOT].frame() {
        Ok(frame) => frame,
        Err(_) => return,
    };

    let first_region = (mapped / GIB) as usize;
    let last_region = ((max_phys + GIB - 1) / GIB) as usize;

    for region in first_region..last_region {
        let block = region / ENTRIES;
        let slot = region % ENTRIES;

        // Each PML4 slot spans 512 GiB; the boot PDPT already covers block 0.
        if slot == 0 {
            let frame = match memory::allocate_frame() {
                Some(frame) => frame,
                None => break,
            };
            let mut entry = PageTableEntry::new();
            entry.set_frame(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
            pml4[DIRECT_MAP_SLOT + block] = entry;
            pdpt = frame;
        }

        let pd_frame = match memory::allocate_frame() {
            Some(frame) => frame,
            None => break,
        };
        let pd = unsafe {
            &mut *(memory::phys_to_virt(pd_frame.start_address().as_u64()) as *mut PageTable)
        };
        let base = region as u64 * GIB;
        for idx in 0..ENTRIES {
            let phys = base + idx as u64 * PD_SIZE;
            let mut entry = PageTableEntry::new();
            entry.set_addr(
                PhysAddr::new(phys),
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::HUGE_PAGE,
            );
            pd[idx] = entry;
        }

        let pdpt_table =
            unsafe { &mut *(memory::phys_to_virt(pdpt.start_address().as_u64()) as *mut PageTable) };
        let mut entry = PageTableEntry::new();
        entry.set_frame(pd_frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
        pdpt_table[slot] = entry;

        x86_64::instructions::tlb::flush_all();
        memory::set_mapped_limit((region as u64 + 1) * GIB);
    }
}

fn kernel_pml4() -> PhysFrame<Size4KiB> {
    memory::frame_containing(KERNEL_PML4.load(Ordering::Relaxed))
}

pub struct AddressSpace {
    pml4: PhysFrame<Size4KiB>,
}

impl AddressSpace {
    pub fn new() -> Option<Self> {
        let pml4 = memory::allocate_frame()?;
        let kernel = kernel_pml4();

        // The upper half holds the shared kernel direct map (and any future
        // higher-half mappings), so copy those entries verbatim.
        for idx in USER_SLOTS..ENTRIES {
            let entry = read_entry(kernel, idx);
            if !entry.is_unused() {
                write_entry(pml4, idx, entry);
            }
        }

        // The lower half is private, but the kernel image must stay identity
        // mapped: interrupts and syscalls enter while this CR3 is still loaded.
        let pdpt = match memory::allocate_frame() {
            Some(frame) => frame,
            None => {
                memory::deallocate_frame(pml4);
                return None;
            }
        };
        let pd = match memory::allocate_frame() {
            Some(frame) => frame,
            None => {
                memory::deallocate_frame(pdpt);
                memory::deallocate_frame(pml4);
                return None;
            }
        };
        let kernel_pdpt = match read_entry(kernel, 0).frame() {
            Ok(frame) => frame,
            Err(_) => {
                memory::deallocate_frame(pd);
                memory::deallocate_frame(pdpt);
                memory::deallocate_frame(pml4);
                return None;
            }
        };
        let kernel_pd = match read_entry(kernel_pdpt, 0).frame() {
            Ok(frame) => frame,
            Err(_) => {
                memory::deallocate_frame(pd);
                memory::deallocate_frame(pdpt);
                memory::deallocate_frame(pml4);
                return None;
            }
        };
        for idx in 0..KERNEL_LOW_PD_ENTRIES {
            write_entry(pd, idx, read_entry(kernel_pd, idx));
        }
        let mut pdpt_entry = PageTableEntry::new();
        pdpt_entry.set_frame(pd, table_flags());
        write_entry(pdpt, 0, pdpt_entry);
        let mut root_entry = PageTableEntry::new();
        root_entry.set_frame(pdpt, table_flags());
        write_entry(pml4, 0, root_entry);

        Some(Self { pml4 })
    }

    pub fn pml4_phys(&self) -> u64 {
        self.pml4.start_address().as_u64()
    }

    fn mapper(&self) -> OffsetPageTable<'static> {
        unsafe { OffsetPageTable::new(&mut *table_ptr(self.pml4), VirtAddr::new(PHYS_MAP_BASE)) }
    }

    /// Maps a fresh zeroed frame. If the page is already mapped, only its flags are updated.
    pub fn map_user_zeroed(&mut self, virt: u64, writable: bool) -> Option<PhysFrame<Size4KiB>> {
        if !in_user_range(virt) || virt % memory::PAGE_SIZE as u64 != 0 {
            return None;
        }

        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(virt));
        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if writable {
            flags |= PageTableFlags::WRITABLE;
        }

        let mut mapper = self.mapper();
        if let Ok(frame) = mapper.translate_page(page) {
            unsafe { mapper.update_flags(page, flags) }.ok()?.flush();
            return Some(frame);
        }

        let frame = memory::allocate_frame()?;
        match unsafe { mapper.map_to(page, frame, flags, &mut memory::BitmapFrameAllocator) } {
            Ok(flush) => {
                flush.flush();
                Some(frame)
            }
            Err(_) => {
                memory::deallocate_frame(frame);
                None
            }
        }
    }
}

/// Frees a process PML4 and every table and frame below it. Only slots that
/// belong to user space are walkable: the shared direct map lives above
/// `USER_LIMIT` and must stay owned by the kernel.
pub fn destroy_pml4(pml4: u64) {
    let root = memory::frame_containing(pml4 & !(memory::PAGE_SIZE as u64 - 1));
    for idx in 0..USER_SLOTS {
        let entry = read_entry(root, idx);
        if entry.flags().contains(PageTableFlags::PRESENT) {
            if let Ok(frame) = entry.frame() {
                free_tree(frame, 3);
            }
        }
    }
    memory::deallocate_frame(root);
}

pub fn in_user_range(virt: u64) -> bool {
    (USER_BASE..USER_LIMIT).contains(&virt)
}

fn free_tree(table: PhysFrame<Size4KiB>, level: u32) {
    for idx in 0..ENTRIES {
        let entry = read_entry(table, idx);
        let flags = entry.flags();
        if !flags.contains(PageTableFlags::PRESENT) {
            continue;
        }
        // A huge entry maps physical memory directly (the kernel low identity
        // region); the frame behind it is not owned by this address space.
        if flags.contains(PageTableFlags::HUGE_PAGE) {
            continue;
        }
        if let Ok(child) = entry.frame() {
            if level > 1 {
                free_tree(child, level - 1);
            } else {
                memory::deallocate_frame(child);
            }
        }
    }
    memory::deallocate_frame(table);
}

/// Prints the paging entries that translate `va` under `cr3`, one line per level, for fault diagnosis.
pub fn debug_walk(cr3: u64, va: u64) {
    let addr = VirtAddr::new(va);
    let indices = [addr.p4_index(), addr.p3_index(), addr.p2_index(), addr.p1_index()];
    let mut table = memory::frame_containing(cr3 & !(memory::PAGE_SIZE as u64 - 1));

    for (level, &idx) in indices.iter().enumerate() {
        let entry = read_entry(table, usize::from(idx));
        let raw = entry.addr().as_u64() | entry.flags().bits();
        crate::serial::send_string("  walk L");
        crate::serial::send_byte(b'4' - level as u8);
        crate::serial::send_string(" entry=0x");
        for j in 0..16 {
            let nib = ((raw >> ((15 - j) * 4)) & 0xF) as u8;
            crate::serial::send_byte(if nib < 10 { b'0' + nib } else { b'A' + nib - 10 });
        }
        crate::serial::send_string("\n");
        if !entry.flags().contains(PageTableFlags::PRESENT) {
            break;
        }
        match entry.frame() {
            Ok(frame) => table = frame,
            Err(_) => break,
        }
    }
}

