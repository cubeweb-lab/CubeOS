use linked_list_allocator::LockedHeap;

/// Higher-half address used for the kernel heap. Must stay above
/// `vmm::USER_LIMIT` so process address spaces copy the mapping.
pub const HEAP_BASE: u64 = 0xFFFF_A000_0000_0000;
const HEAP_PAGES: usize = 4096;
const HEAP_SIZE: usize = HEAP_PAGES * 4096;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn init() -> bool {
    if !crate::vmm::map_kernel_region(HEAP_BASE, HEAP_PAGES) {
        return false;
    }
    unsafe {
        ALLOCATOR.lock().init(HEAP_BASE as *mut u8, HEAP_SIZE);
    }
    true
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
