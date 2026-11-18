// src/cube_core/apic.rs
// Local APIC + I/O APIC + periodic timer.

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

// MSR
const IA32_APIC_BASE: u32 = 0x1B;
const APIC_BASE_ENABLE: u64 = 1 << 11;

// Local APIC регистры (offset от base)
const LAPIC_TPR: u64          = 0x080;
const LAPIC_EOI: u64          = 0x0B0;
const LAPIC_SVR: u64          = 0x0F0;
const LAPIC_LVT_TIMER: u64    = 0x320;
const LAPIC_TIMER_INIT: u64   = 0x380;
const LAPIC_TIMER_DIV: u64    = 0x3E0;

// I/O APIC
const IOAPIC_BASE: u64 = 0xFEC0_0000;

pub const APIC_TIMER_VECTOR: u8 = 0x40;
pub const APIC_SPURIOUS_VECTOR: u8 = 0xFF;

// Храним base, чтобы EOI работал из обработчиков
static mut LAPIC_BASE: u64 = 0;

#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi);
    ((hi as u64) << 32) | lo as u64
}

#[inline]
unsafe fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    asm!("wrmsr", in("ecx") msr, in("eax") lo, in("edx") hi);
}

#[inline]
unsafe fn lapic_read(offset: u64) -> u32 {
    read_volatile((LAPIC_BASE + offset) as *const u32)
}

#[inline]
unsafe fn lapic_write(offset: u64, val: u32) {
    write_volatile((LAPIC_BASE + offset) as *mut u32, val);
}

/// Включает Local APIC через MSR. Возвращает физ. адрес.
pub fn enable() -> u64 {
    unsafe {
        let mut base = rdmsr(IA32_APIC_BASE);
        if base & APIC_BASE_ENABLE == 0 {
            base |= APIC_BASE_ENABLE;
            wrmsr(IA32_APIC_BASE, base);
        }
        let addr = base & 0xFFFF_F000;
        LAPIC_BASE = crate::memory::phys_to_virt(addr);
        addr
    }
}

/// Инициализация Local APIC: SVR, TPR.
pub fn init() {
    unsafe {
        // Spurious vector 0xFF + APIC enable (bit 8)
        lapic_write(LAPIC_SVR, (APIC_SPURIOUS_VECTOR as u32) | (1 << 8));
        // TPR = 0 — принимать все приоритеты
        lapic_write(LAPIC_TPR, 0);
    }
}

/// Periodic timer. hz — желаемая частота (например, 100).
pub fn timer_init_periodic(hz: u32) {
    unsafe {
        // Делитель /16 → 1 MHz (QEMU: bus 16 MHz)
        lapic_write(LAPIC_TIMER_DIV, 0b0011);

        // LVT_TIMER: vector | periodic (bit 17), не маскирован (bit 16 = 0)
        lapic_write(
            LAPIC_LVT_TIMER,
            (APIC_TIMER_VECTOR as u32) | (1 << 17),
        );

        // initial count = 1 MHz / hz
        let init = 1_000_000u32 / hz.max(1);
        lapic_write(LAPIC_TIMER_INIT, init);
    }
}

/// EOI в Local APIC. Вызывать из всех обработчиков, которые идут через APIC.
pub fn eoi() {
    unsafe {
        if LAPIC_BASE != 0 {
            lapic_write(LAPIC_EOI, 0);
        }
    }
}

// ===== I/O APIC =====

unsafe fn ioapic_write(reg: u32, val: u32) {
    let base = crate::memory::phys_to_virt(IOAPIC_BASE);
    let sel = base as *mut u32;
    let win = (base + 0x10) as *mut u32;
    write_volatile(sel, reg);
    write_volatile(win, val);
}

/// Настроить IRQ в I/O APIC: vector, dest_cpu.
/// low: vector | active-high (bit13) | edge (bit15=0), unmasked.
/// high: dest_cpu << 24.
pub fn ioapic_set_irq(irq: u8, vector: u8, dest_cpu: u8) {
    let reg = 0x10 + 2 * irq as u32;
    let low = (vector as u32) | (1 << 13);
    let high = (dest_cpu as u32) << 24;
    unsafe {
        ioapic_write(reg, low);
        ioapic_write(reg + 1, high);
    }
}

/// Замаскировать все линии 8259 PIC. Вызывать после ioapic_set_irq.
pub fn pic_disable() {
    unsafe {
        asm!("out 0x21, al", in("al") 0xFFu8);
        asm!("out 0xA1, al", in("al") 0xFFu8);
    }
}
