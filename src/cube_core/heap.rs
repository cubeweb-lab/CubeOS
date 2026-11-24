// NovumOS port: heap allocator adapted from NovumOS zig/kernel/memory.zig.
// Copyright 2025-2026 MinecAnton209. Apache-2.0; see NOTICE for provenance.
use crate::sync::IrqLock;
use core::alloc::{GlobalAlloc, Layout};
use core::mem::size_of;
use core::ptr;

/// Higher-half address used for the kernel heap. Must stay above
/// `vmm::USER_LIMIT` so process address spaces copy the mapping.
pub const HEAP_BASE: u64 = 0xFFFF_A000_0000_0000;
const PAGE_SIZE: usize = 4096;

const HEAP_MAGIC: u32 = 0x4845_4150;
const END_CANARY: u32 = 0xCAFE_BABE;
const CANARY_BYTE: u8 = 0xCB;
const POISON_ALLOC: u8 = 0xAA;
const POISON_FREE: u8 = 0xDF;
const CANARY_SIZE: usize = 4;
const MAX_ALLOC_SIZE: usize = 16 * 1024 * 1024;
const MIN_BIN_SHIFT: u32 = 5;
const BIN_COUNT: usize = 27;
const DATA_ALIGN: usize = 16;

#[repr(C)]
struct BlockHeader {
    magic: u32,
    size: u32,
    is_free: u32,
    requested: u32,
}

#[repr(C)]
struct BlockFooter {
    size: u32,
}

const HEADER_SIZE: usize = size_of::<BlockHeader>();
const FOOTER_SIZE: usize = size_of::<BlockFooter>();

const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

const MIN_BLOCK_SIZE: usize = align_up(
    HEADER_SIZE + 2 * size_of::<*mut BlockHeader>() + FOOTER_SIZE,
    DATA_ALIGN,
);

struct HeapState {
    bins: [*mut BlockHeader; BIN_COUNT],
    base: usize,
    end: usize,
    initialized: bool,
}

impl HeapState {
    const fn new() -> Self {
        Self {
            bins: [ptr::null_mut(); BIN_COUNT],
            base: 0,
            end: 0,
            initialized: false,
        }
    }

    fn in_heap(&self, addr: usize, len: usize) -> bool {
        let Some(end) = addr.checked_add(len) else {
            return false;
        };
        addr >= self.base && end <= self.end
    }

    unsafe fn grow(&mut self, need: usize) -> bool {
        if !self.initialized {
            return false;
        }
        let pages = ((need + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
        for _ in 0..pages {
            let addr = self.end;
            let Some(frame) = crate::memory::allocate_frame() else {
                return false;
            };
            if !crate::vmm::map_kernel_frame(addr as u64, frame.start_address().as_u64()) {
                crate::memory::deallocate_frame(frame);
                return false;
            }

            let block = addr as *mut BlockHeader;
            write_header(block, PAGE_SIZE, true, 0);
            self.end = addr + PAGE_SIZE;
            let merged = self.coalesce(block);
            self.bin_push(merged);
        }
        true
    }

    unsafe fn find_fit(&mut self, need: usize) -> Option<*mut BlockHeader> {
        let mut bin = bin_index(need);
        while bin < BIN_COUNT {
            let mut block = self.bins[bin];
            while !block.is_null() {
                let next = next_free(block);
                if (*block).size as usize >= need {
                    self.bin_remove(block);
                    return Some(block);
                }
                block = next;
            }
            bin += 1;
        }
        None
    }

    unsafe fn split_block(&mut self, block: *mut BlockHeader, need: usize) {
        let size = (*block).size as usize;
        let remaining = size - need;
        if remaining >= MIN_BLOCK_SIZE {
            let next = (block as usize + need) as *mut BlockHeader;
            write_header(next, remaining, true, 0);
            (*block).size = need as u32;
            write_footer(block);
            self.bin_push(next);
        }
    }

    unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        if !self.initialized
            || size > MAX_ALLOC_SIZE
            || !align.is_power_of_two()
            || align > DATA_ALIGN
        {
            return ptr::null_mut();
        }

        let aligned = align_up(size, DATA_ALIGN);
        let need = align_up(
            HEADER_SIZE + aligned + CANARY_SIZE + FOOTER_SIZE,
            DATA_ALIGN,
        )
        .max(MIN_BLOCK_SIZE);

        loop {
            let Some(block) = self.find_fit(need) else {
                if !self.grow(need) {
                    return ptr::null_mut();
                }
                continue;
            };

            self.split_block(block, need);
            write_footer(block);
            (*block).is_free = 0;
            (*block).requested = size as u32;

            let data = data_ptr(block);
            ptr::write_bytes(data, POISON_ALLOC, size);

            let canary = data.add(aligned) as *mut u32;
            ptr::write(canary, END_CANARY);
            if aligned > size {
                ptr::write_bytes(data.add(size), CANARY_BYTE, aligned - size);
            }
            return data;
        }
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) -> bool {
        let addr = ptr as usize;
        if addr < self.base.saturating_add(HEADER_SIZE) || addr % DATA_ALIGN != 0 {
            return false;
        }
        let header_addr = addr - HEADER_SIZE;
        if !self.in_heap(header_addr, HEADER_SIZE) {
            return false;
        }

        let header = header_addr as *mut BlockHeader;
        if (*header).magic != HEAP_MAGIC || (*header).is_free != 0 {
            return false;
        }
        let size = (*header).size as usize;
        if size < MIN_BLOCK_SIZE || !self.in_heap(header_addr, size) {
            return false;
        }
        let requested = (*header).requested as usize;
        if requested > size {
            return false;
        }

        let aligned = align_up(requested, DATA_ALIGN);
        let Some(canary_off) = HEADER_SIZE.checked_add(aligned) else {
            return false;
        };
        if canary_off + CANARY_SIZE + FOOTER_SIZE > size {
            return false;
        }
        let stored = ptr::read((header_addr + canary_off) as *const u32);
        if stored != END_CANARY {
            return false;
        }
        if aligned > requested {
            for i in requested..aligned {
                if ptr::read(ptr.add(i)) != CANARY_BYTE {
                    return false;
                }
            }
        }

        let payload_len = size - HEADER_SIZE - FOOTER_SIZE;
        ptr::write_bytes(ptr, POISON_FREE, payload_len);
        (*header).is_free = 1;
        (*header).requested = 0;

        let merged = self.coalesce(header);
        self.bin_push(merged);
        true
    }

    unsafe fn coalesce(&mut self, block: *mut BlockHeader) -> *mut BlockHeader {
        let mut current = block;
        let addr = current as usize;

        if addr >= self.base + FOOTER_SIZE {
            let footer_addr = addr - FOOTER_SIZE;
            if self.in_heap(footer_addr, FOOTER_SIZE) {
                let prev_size = ptr::read(footer_addr as *const u32) as usize;
                if prev_size >= MIN_BLOCK_SIZE && prev_size <= addr - self.base {
                    let prev_addr = addr - prev_size;
                    if self.in_heap(prev_addr, prev_size) {
                        let prev = prev_addr as *mut BlockHeader;
                        if (*prev).magic == HEAP_MAGIC && (*prev).is_free != 0 {
                            self.bin_remove(prev);
                            (*prev).size = (*prev).size + (*current).size;
                            write_footer(prev);
                            current = prev;
                        }
                    }
                }
            }
        }

        let next_addr = current as usize + (*current).size as usize;
        if self.in_heap(next_addr, HEADER_SIZE) {
            let next = next_addr as *mut BlockHeader;
            let next_size = (*next).size as usize;
            if next_size >= MIN_BLOCK_SIZE
                && self.in_heap(next_addr, next_size)
                && (*next).magic == HEAP_MAGIC
                && (*next).is_free != 0
            {
                self.bin_remove(next);
                (*current).size = (*current).size + (*next).size;
                write_footer(current);
            }
        }
        current
    }

    unsafe fn bin_push(&mut self, block: *mut BlockHeader) {
        let bin = bin_index((*block).size as usize);
        let head = self.bins[bin];
        set_next_free(block, head);
        set_prev_free(block, ptr::null_mut());
        if !head.is_null() {
            set_prev_free(head, block);
        }
        self.bins[bin] = block;
    }

    unsafe fn bin_remove(&mut self, block: *mut BlockHeader) {
        let prev = prev_free(block);
        let next = next_free(block);
        if !prev.is_null() {
            set_next_free(prev, next);
        } else {
            let bin = bin_index((*block).size as usize);
            if self.bins[bin] == block {
                self.bins[bin] = next;
            }
        }
        if !next.is_null() {
            set_prev_free(next, prev);
        }
    }
}

fn data_ptr(block: *mut BlockHeader) -> *mut u8 {
    (block as usize + HEADER_SIZE) as *mut u8
}

unsafe fn write_header(block: *mut BlockHeader, size: usize, is_free: bool, requested: usize) {
    (*block).magic = HEAP_MAGIC;
    (*block).size = size as u32;
    (*block).is_free = u32::from(is_free);
    (*block).requested = requested as u32;
    write_footer(block);
}

unsafe fn write_footer(block: *mut BlockHeader) {
    let footer = (block as usize + (*block).size as usize - FOOTER_SIZE) as *mut BlockFooter;
    (*footer).size = (*block).size;
}

unsafe fn next_free(block: *mut BlockHeader) -> *mut BlockHeader {
    ptr::read((block as usize + HEADER_SIZE) as *const *mut BlockHeader)
}

unsafe fn set_next_free(block: *mut BlockHeader, next: *mut BlockHeader) {
    ptr::write((block as usize + HEADER_SIZE) as *mut *mut BlockHeader, next);
}

unsafe fn prev_free(block: *mut BlockHeader) -> *mut BlockHeader {
    ptr::read((block as usize + HEADER_SIZE + size_of::<*mut BlockHeader>()) as *const *mut BlockHeader)
}

unsafe fn set_prev_free(block: *mut BlockHeader, prev: *mut BlockHeader) {
    ptr::write(
        (block as usize + HEADER_SIZE + size_of::<*mut BlockHeader>()) as *mut *mut BlockHeader,
        prev,
    );
}

fn bin_index(size: usize) -> usize {
    if size <= (1usize << MIN_BIN_SHIFT) {
        return 0;
    }
    let bits = (usize::BITS - size.leading_zeros()) as usize;
    (bits - MIN_BIN_SHIFT as usize).min(BIN_COUNT - 1)
}

static HEAP_LOCK: IrqLock = IrqLock::new();
static mut HEAP: HeapState = HeapState::new();

pub fn init() -> bool {
    let _guard = HEAP_LOCK.lock();
    unsafe {
        HEAP = HeapState::new();
        let Some(frame) = crate::memory::allocate_frame() else {
            return false;
        };
        if !crate::vmm::map_kernel_frame(HEAP_BASE, frame.start_address().as_u64()) {
            crate::memory::deallocate_frame(frame);
            return false;
        }

        let base = HEAP_BASE as usize;
        HEAP.base = base;
        HEAP.end = base + PAGE_SIZE;
        HEAP.initialized = true;

        let block = base as *mut BlockHeader;
        write_header(block, PAGE_SIZE, true, 0);
        HEAP.bin_push(block);
    }
    true
}

struct NovaAllocator;

#[global_allocator]
static ALLOCATOR: NovaAllocator = NovaAllocator;

unsafe impl GlobalAlloc for NovaAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard = HEAP_LOCK.lock();
        HEAP.alloc(layout.size(), layout.align())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let ok = {
            let _guard = HEAP_LOCK.lock();
            HEAP.dealloc(ptr)
        };
        if !ok {
            crate::serial::send_string("[HEAP] invalid free rejected\n");
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size <= layout.size() {
            return ptr;
        }
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        let new_ptr = self.alloc(new_layout);
        if !new_ptr.is_null() {
            ptr::copy_nonoverlapping(ptr, new_ptr, layout.size());
            self.dealloc(ptr, layout);
        }
        new_ptr
    }
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    crate::serial::send_string("[HEAP] allocation failed: size=");
    let mut buf = [0u8; 20];
    let mut n = layout.size();
    let mut len = 0;
    if n == 0 {
        buf[0] = b'0';
        len = 1;
    }
    while n > 0 {
        buf[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    for i in (0..len).rev() {
        crate::serial::send_byte(buf[i]);
    }
    crate::serial::send_string("\n");
    loop {
        unsafe { core::arch::asm!("hlt") }
    }
}
