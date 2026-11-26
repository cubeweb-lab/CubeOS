// RAM disk backed by the first Multiboot2 module. Used when the boot media is
// USB or optical and no legacy ATA controller is available.
use crate::memory;

static mut BASE: usize = 0;
static mut LEN: usize = 0;

pub fn init(start: u32, end: u32) -> bool {
    if end <= start {
        return false;
    }
    unsafe {
        BASE = memory::phys_to_virt(start as u64) as usize;
        LEN = (end - start) as usize;
    }
    true
}

pub fn is_present() -> bool {
    unsafe { LEN > 0 }
}

pub fn len() -> usize {
    unsafe { LEN }
}

pub fn read(offset: usize, out: &mut [u8]) -> usize {
    unsafe {
        if LEN == 0 || offset >= LEN {
            return 0;
        }
        let count = out.len().min(LEN - offset);
        core::ptr::copy_nonoverlapping((BASE + offset) as *const u8, out.as_mut_ptr(), count);
        count
    }
}

pub fn write(offset: usize, data: &[u8]) -> usize {
    unsafe {
        if LEN == 0 || offset >= LEN {
            return 0;
        }
        let count = data.len().min(LEN - offset);
        core::ptr::copy_nonoverlapping(data.as_ptr(), (BASE + offset) as *mut u8, count);
        count
    }
}
