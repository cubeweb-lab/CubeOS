// src/cube_core/tss.rs
use core::arch::asm;

// Intel fixes rsp0 at offset 4, which is unaligned. With repr(C) the u64 would be padded to offset 8,
// so the CPU would read a shifted, non-canonical RSP and raise #SS on every ring 3 interrupt.
#[repr(C, packed)]
pub struct TSS {
    pub reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    pub reserved1: u64,
    pub ist1: u64, pub ist2: u64, pub ist3: u64, pub ist4: u64,
    pub ist5: u64, pub ist6: u64, pub ist7: u64,
    pub reserved2: u64,
    pub reserved3: u16,
    pub iomap_base: u16,
}

const _: () = {
    assert!(core::mem::offset_of!(TSS, rsp0) == 4);
    assert!(core::mem::size_of::<TSS>() == 104);
};

pub static mut KERNEL_TSS: TSS = TSS {
    reserved0: 0, rsp0: 0, rsp1: 0, rsp2: 0, reserved1: 0,
    ist1: 0, ist2: 0, ist3: 0, ist4: 0, ist5: 0, ist6: 0, ist7: 0,
    reserved2: 0, reserved3: 0, iomap_base: 104,
};

/// Stack the CPU switches to on an interrupt taken in ring 3. Must be set to the
/// kernel stack of the task that is about to run.
pub fn set_rsp0(kernel_stack_top: u64) {
    unsafe { KERNEL_TSS.rsp0 = kernel_stack_top };
}

pub fn init() {
    unsafe {
        let tss_addr = &raw const KERNEL_TSS as u64;

        let desc = (0x13000u64 + 0x28) as *mut u8;
        *(desc as *mut u16) = 103;
        *(desc.add(2) as *mut u16) = (tss_addr & 0xFFFF) as u16;
        *(desc.add(4)) = ((tss_addr >> 16) & 0xFF) as u8;
        *(desc.add(5)) = 0x89;
        *(desc.add(6)) = 0x00;
        *(desc.add(7)) = ((tss_addr >> 24) & 0xFF) as u8;
        *(desc.add(8) as *mut u32) = (tss_addr >> 32) as u32;
        *(desc.add(12) as *mut u32) = 0;

        asm!("ltr ax", in("ax") 0x28u16);
        crate::serial::send_string("[TSS] Loaded OK\n");
    }
}
