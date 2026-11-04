// src/cube_core/uptime.rs
// ACPI/APIC tick counter + uptime formatting

use core::sync::atomic::{AtomicU64, Ordering};
use crate::zigfs;

/// Частота APIC-таймера в Гц (совпадает с timer_init_periodic).
pub const TICK_HZ: u64 = 100;

/// Тиков с момента запуска. Увеличивается в APIC-обработчике.
static TICKS: AtomicU64 = AtomicU64::new(0);

/// Вызывается из APIC timer handler (vector 0x40) до EOI.
#[inline]
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Текущее число тиков.
#[inline]
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Секунд с момента запуска.
#[inline]
pub fn seconds() -> u64 {
    ticks() / TICK_HZ
}

#[derive(Copy, Clone)]
pub struct Uptime {
    pub days: u64,
    pub hours: u64,
    pub mins: u64,
    pub secs: u64,
}

impl Uptime {
    pub fn now() -> Self {
        let total = seconds();
        Self {
            days:  total / 86_400,
            hours: (total / 3_600) % 24,
            mins:  (total / 60)   % 60,
            secs:  total % 60,
        }
    }
}

/// Пишет uptime в /system/info/uptime в формате
/// "X days, X hours, X mins, X sec\n".
/// Возвращает true при успехе.
pub fn write_uptime_file(ata: &crate::ata::AtaDrive, fs: &crate::fs::FSManager) -> bool {
    let u = Uptime::now();

    // Форматируем в фиксированный буфер без аллокаций.
    let mut buf = [0u8; 128];
    let mut n = 0usize;

    n += write_num(&mut buf[n..], u.days);
    n += write_str(&mut buf[n..], if u.days == 1 { " day, " } else { " days, " });

    n += write_num(&mut buf[n..], u.hours);
    n += write_str(&mut buf[n..], if u.hours == 1 { " hour, " } else { " hours, " });

    n += write_num(&mut buf[n..], u.mins);
    n += write_str(&mut buf[n..], if u.mins == 1 { " min, " } else { " mins, " });

    n += write_num(&mut buf[n..], u.secs);
    n += write_str(&mut buf[n..], if u.secs == 1 { " sec\n" } else { " sec\n" });

    zigfs::write_file(0, b"/system/info/uptime", &buf[..n])
}

fn write_str(dst: &mut [u8], s: &str) -> usize {
    let b = s.as_bytes();
    let n = b.len().min(dst.len());
    dst[..n].copy_from_slice(&b[..n]);
    n
}

fn write_num(dst: &mut [u8], mut n: u64) -> usize {
    if n == 0 {
        if !dst.is_empty() { dst[0] = b'0'; }
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while n > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let len = 20 - i;
    let copy = len.min(dst.len());
    dst[..copy].copy_from_slice(&tmp[i..i + copy]);
    copy
}
