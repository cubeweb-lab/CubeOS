use crate::checks::{page_table_index, PAGE_SIZE, PTE_ADDR_MASK, PTE_HUGE, PTE_PRESENT, PTE_USER, PTE_WRITABLE};
use crate::memory;
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

pub const USER_BASE: u64 = 0x0000_0000_0040_0000;
pub const USER_LIMIT: u64 = 0x0000_8000_0000_0000;

const ENTRIES: usize = 512;
const TABLE_FLAGS: u64 = PTE_PRESENT | PTE_WRITABLE | PTE_USER;
const PD_SIZE: u64 = 0x20_0000;
/// PML4 slots below this cover user space and stay private per process.
const USER_SLOTS: usize = (USER_LIMIT >> 39) as usize;
/// Number of 2 MiB kernel identity entries kept below `USER_BASE`.
const KERNEL_LOW_PD_ENTRIES: usize = (USER_BASE / PD_SIZE) as usize;

const _: () = assert!(USER_BASE % PD_SIZE == 0);
const _: () = assert!(USER_BASE < USER_LIMIT);

static KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    KERNEL_PML4.store(read_cr3() & PTE_ADDR_MASK, Ordering::Relaxed);
}

pub fn kernel_cr3() -> u64 {
    KERNEL_PML4.load(Ordering::Relaxed)
}

pub fn load_cr3(cr3: u64) {
    if read_cr3() & PTE_ADDR_MASK != cr3 {
        unsafe { asm!("mov cr3, {}", in(reg) cr3, options(nostack)) };
    }
}

pub struct AddressSpace {
    pml4: u64,
}

impl AddressSpace {
    pub fn new() -> Option<Self> {
        let pml4 = alloc_table()?;
        let kernel = kernel_cr3();

        // The upper half holds the shared kernel direct map (and any future
        // higher-half mappings), so copy those entries verbatim.
        for idx in USER_SLOTS..ENTRIES {
            let entry = read_entry(kernel, idx);
            if entry != 0 {
                write_entry(pml4, idx, entry);
            }
        }

        // The lower half is private, but the kernel image must stay identity
        // mapped: interrupts and syscalls enter while this CR3 is still loaded.
        let pdpt = match alloc_table() {
            Some(table) => table,
            None => {
                memory::free_page(pml4);
                return None;
            }
        };
        let pd = match alloc_table() {
            Some(table) => table,
            None => {
                memory::free_page(pdpt);
                memory::free_page(pml4);
                return None;
            }
        };
        let kernel_pdpt = read_entry(kernel, 0) & PTE_ADDR_MASK;
        let kernel_pd = read_entry(kernel_pdpt, 0) & PTE_ADDR_MASK;
        for idx in 0..KERNEL_LOW_PD_ENTRIES {
            write_entry(pd, idx, read_entry(kernel_pd, idx));
        }
        write_entry(pdpt, 0, pd | TABLE_FLAGS);
        write_entry(pml4, 0, pdpt | TABLE_FLAGS);

        Some(Self { pml4 })
    }

    pub fn pml4_phys(&self) -> u64 { self.pml4 }

    pub fn activate(&self) {
        load_cr3(self.pml4);
    }

    /// Мапит fresh zeroed frame. Если страница уже есть — обновляет флаги.
    pub fn map_user_zeroed(&mut self, virt: u64, writable: bool) -> Option<u64> {
        if !in_user_range(virt) || virt % PAGE_SIZE != 0 {
            return None;
        }

        // Если уже замаплено — обновить writable, вернуть существующий frame
        if let Some(existing) = self.translate(virt) {
            let phys = existing & PTE_ADDR_MASK;
            if !self.map_leaf(virt, phys, writable) {
                return None;
            }
            return Some(phys);
        }

        let frame = memory::alloc_page()?;
        unsafe { core::ptr::write_bytes(memory::phys_to_virt(frame) as *mut u8, 0, PAGE_SIZE as usize) };
        if !self.map_leaf(virt, frame, writable) {
            memory::free_page(frame);
            return None;
        }
        Some(frame)
    }

    pub fn translate(&self, virt: u64) -> Option<u64> {
        let mut table = self.pml4;
        for level in (2..=4).rev() {
            let entry = read_entry(table, page_table_index(virt, level));
            if entry & PTE_PRESENT == 0 {
                return None;
            }
            table = entry & PTE_ADDR_MASK;
        }
        let leaf = read_entry(table, page_table_index(virt, 1));
        if leaf & PTE_PRESENT == 0 {
            return None;
        }
        Some((leaf & PTE_ADDR_MASK) | (virt & (PAGE_SIZE - 1)))
    }


    fn map_leaf(&mut self, virt: u64, frame: u64, writable: bool) -> bool {
        let mut table = self.pml4;
        for level in (2..=4).rev() {
            table = match next_table(table, page_table_index(virt, level)) {
                Some(next) => next,
                None => return false,
            };
        }
        let idx = page_table_index(virt, 1);
        let old = read_entry(table, idx);
        if old & PTE_PRESENT != 0 {
            if writable && (old & PTE_WRITABLE == 0) {
                write_entry(table, idx, old | PTE_WRITABLE);
                invlpg(virt);
            }
            return true;
        }
        let mut flags = PTE_PRESENT | PTE_USER;
        if writable { flags |= PTE_WRITABLE; }
        write_entry(table, idx, (frame & PTE_ADDR_MASK) | flags);
        invlpg(virt);
        true
    }
}

/// Frees a process PML4 and every table and frame below it. Only slots that
/// belong to user space are walkable: the shared direct map lives above
/// `USER_LIMIT` and must stay owned by the kernel.
pub fn destroy_pml4(pml4: u64) {
    for idx in 0..USER_SLOTS {
        let entry = read_entry(pml4, idx);
        if entry & PTE_PRESENT != 0 {
            free_tree(entry & PTE_ADDR_MASK, 3);
        }
    }
    memory::free_page(pml4);
}

pub fn in_user_range(virt: u64) -> bool {
    (USER_BASE..USER_LIMIT).contains(&virt)
}

fn next_table(table: u64, idx: usize) -> Option<u64> {
    let entry = read_entry(table, idx);
    if entry & PTE_PRESENT != 0 {
        return Some(entry & PTE_ADDR_MASK);
    }
    let child = alloc_table()?;
    write_entry(table, idx, child | TABLE_FLAGS);
    Some(child)
}

fn free_tree(table: u64, level: u32) {
    for idx in 0..ENTRIES {
        let entry = read_entry(table, idx);
        if entry & PTE_PRESENT == 0 { continue; }
        // A huge entry maps physical memory directly (the kernel low identity
        // region); the frame behind it is not owned by this address space.
        if entry & PTE_HUGE != 0 { continue; }
        let child = entry & PTE_ADDR_MASK;
        if level > 1 { free_tree(child, level - 1); } else { memory::free_page(child); }
    }
    memory::free_page(table);
}

fn alloc_table() -> Option<u64> {
    let table = memory::alloc_page()?;
    unsafe { core::ptr::write_bytes(memory::phys_to_virt(table) as *mut u8, 0, PAGE_SIZE as usize) };
    Some(table)
}

/// Prints the paging entries that translate `va` under `cr3`, one line per level, for fault diagnosis.
pub fn debug_walk(cr3: u64, va: u64) {
    let indices = [(va >> 39) & 0x1FF, (va >> 30) & 0x1FF, (va >> 21) & 0x1FF, (va >> 12) & 0x1FF];
    let mut table = cr3 & PTE_ADDR_MASK;
    for (level, &idx) in indices.iter().enumerate() {
        let entry = read_entry(table, idx as usize);
        crate::serial::send_string("  walk L");
        crate::serial::send_byte(b'4' - level as u8);
        crate::serial::send_string(" entry=0x");
        for j in 0..16 {
            let nib = ((entry >> ((15 - j) * 4)) & 0xF) as u8;
            crate::serial::send_byte(if nib < 10 { b'0' + nib } else { b'A' + nib - 10 });
        }
        crate::serial::send_string("\n");
        if entry & 1 == 0 {
            break;
        }
        table = entry & PTE_ADDR_MASK;
    }
}

fn read_entry(table: u64, idx: usize) -> u64 {
    unsafe { *((memory::phys_to_virt(table) as *const u64).add(idx)) }
}

fn write_entry(table: u64, idx: usize, value: u64) {
    unsafe { *((memory::phys_to_virt(table) as *mut u64).add(idx)) = value }
}

fn read_cr3() -> u64 {
    let value: u64;
    unsafe { asm!("mov {}, cr3", out(reg) value, options(nomem, nostack)) };
    value
}

fn invlpg(virt: u64) {
    unsafe { asm!("invlpg [{}]", in(reg) virt, options(nostack)) };
}
