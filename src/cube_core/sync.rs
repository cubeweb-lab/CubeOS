// NovumOS port: IrqLock follows the interrupt-save/spinlock discipline used by
// NovumOS zig/kernel/memory.zig. Copyright 2025-2026 MinecAnton209.
// Apache-2.0; see NOTICE for provenance.
use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock {
    locked: AtomicBool,
}

impl SpinLock {
    pub const fn new() -> Self {
        Self { locked: AtomicBool::new(false) }
    }

    pub fn lock(&self) {
        while self.locked.swap(true, Ordering::Acquire) {
            unsafe { core::arch::asm!("pause") }
        }
    }

    pub fn try_lock(&self) -> bool {
        !self.locked.swap(true, Ordering::Acquire)
    }

    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

/// Spin lock that masks local interrupts for the critical section. Use this
/// for state shared with interrupt handlers (PMM, heap); a plain [`SpinLock`]
/// can deadlock if an IRQ tries to acquire a lock held by the interrupted code.
pub struct IrqLock {
    locked: AtomicBool,
}

pub struct IrqGuard<'a> {
    lock: &'a IrqLock,
    flags: u64,
}

impl IrqLock {
    pub const fn new() -> Self {
        Self { locked: AtomicBool::new(false) }
    }

    pub fn lock(&self) -> IrqGuard<'_> {
        let flags = irq_save();
        while self.locked.swap(true, Ordering::Acquire) {
            unsafe { core::arch::asm!("pause") }
        }
        IrqGuard { lock: self, flags }
    }
}

impl Drop for IrqGuard<'_> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        irq_restore(self.flags);
    }
}

#[inline]
fn irq_save() -> u64 {
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) flags, options(nostack));
        core::arch::asm!("cli", options(nomem, nostack));
    }
    flags
}

#[inline]
fn irq_restore(flags: u64) {
    if flags & (1 << 9) != 0 {
        unsafe { core::arch::asm!("sti", options(nomem, nostack)) }
    }
}

pub static ATA_LOCK: SpinLock = SpinLock::new();
