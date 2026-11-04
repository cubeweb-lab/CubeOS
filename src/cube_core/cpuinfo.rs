// src/cube_core/cpuinfo.rs
// Определение точной модели CPU через CPUID + запись в /system/info/cpu-m

use core::arch::asm;
use crate::zigfs;

// ===== CPUID =====

fn cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    unsafe {
        asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
             inout("ecx") subleaf => ecx,
             out("edx") edx,
             ebx_out = out(reg) ebx,
        );
    }
    (eax, ebx, ecx, edx)
}

fn cpuid_max_leaf() -> u32 {
    cpuid(0, 0).0
}

fn cpuid_max_extended_leaf() -> u32 {
    cpuid(0x8000_0000, 0).0
}

/// Бренд-строка CPU (48 байт, "AMD Athlon(tm) 64 X2 ...")
pub fn brand_string() -> [u8; 48] {
    let mut brand = [0u8; 48];
    if cpuid_max_extended_leaf() < 0x8000_0004 {
        return brand;
    }
    for (i, leaf) in [0x8000_0002u32, 0x8000_0003, 0x8000_0004].iter().enumerate() {
        let (eax, ebx, ecx, edx) = cpuid(*leaf, 0);
        let base = i * 16;
        brand[base..base + 4].copy_from_slice(&eax.to_le_bytes());
        brand[base + 4..base + 8].copy_from_slice(&ebx.to_le_bytes());
        brand[base + 8..base + 12].copy_from_slice(&ecx.to_le_bytes());
        brand[base + 12..base + 16].copy_from_slice(&edx.to_le_bytes());
    }
    brand
}

/// Число логических процессоров (из leaf 0x1, EDX bit 16-23)
pub fn logical_cores() -> u8 {
    let (_, ebx, _, _) = cpuid(1, 0);
    ((ebx >> 16) & 0xFF) as u8
}

/// Базовая частота (МГц) — для Intel/AMD через leaf 0x16 (если есть),
/// иначе fallback через leaf 0x15.
pub fn base_freq_mhz() -> Option<u32> {
    if cpuid_max_leaf() >= 0x16 {
        let (eax, ebx, _, _) = cpuid(0x16, 0);
        let base = eax & 0xFFFF;
        if base != 0 {
            return Some(base);
        }
    }
    None
}

/// Сколько физических ядер (topology leaf 0x1F или 0x0B).
pub fn physical_cores() -> u8 {
    let max_leaf = cpuid_max_leaf();
    let leaf = if max_leaf >= 0x1F { 0x1F } else if max_leaf >= 0x0B { 0x0B } else { return 1 };

    // Считаем ядра: каждый уровень topology даёт shift
    let mut cores: u8 = 0;
    for subleaf in 0..8 {
        let (eax, ebx, ecx, _) = cpuid(leaf, subleaf);
        let level_type = (ecx >> 8) & 0xFF;
        if level_type == 0 { break; }
        // level_type 2 = Core
        if level_type == 2 {
            cores = (ebx & 0xFFFF) as u8;
        }
    }
    if cores == 0 { 1 } else { cores }
}

// ===== Форматирование =====

fn write_str(dst: &mut [u8], pos: usize, s: &str) -> usize {
    let b = s.as_bytes();
    let n = b.len().min(dst.len() - pos);
    dst[pos..pos + n].copy_from_slice(&b[..n]);
    pos + n
}

fn write_num(dst: &mut [u8], pos: usize, mut n: u64) -> usize {
    if n == 0 {
        dst[pos] = b'0';
        return pos + 1;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while n > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let len = 20 - i;
    let copy = len.min(dst.len() - pos);
    dst[pos..pos + copy].copy_from_slice(&tmp[i..i + copy]);
    pos + copy
}

/// Форматирует модель CPU в строку вида:
/// "AMD Athlon(tm) 64 X2 Dual Core Processor 3800+ (2) @ 2.0 GHz"
pub fn format_cpu_model() -> ([u8; 128], usize) {
    let brand = brand_string();
    let mut out = [0u8; 128];
    let mut pos = 0;

    // Бренд-строка (обрезаем по NUL и пробелы)
    let end = brand.iter().position(|&b| b == 0).unwrap_or(48);
    let mut start = 0;
    while start < end && brand[start] == b' ' { start += 1; }
    let mut stop = end;
    while stop > start && brand[stop - 1] == b' ' { stop -= 1; }

    pos = write_str(&mut out, pos, core::str::from_utf8(&brand[start..stop]).unwrap_or("Unknown CPU"));

    // (N) — число логических ядер
    let cores = logical_cores();
    if cores > 0 {
        pos = write_str(&mut out, pos, " (");
        pos = write_num(&mut out, pos, cores as u64);
        pos = write_str(&mut out, pos, ")");
    }

    // @ X.X GHz — из base_freq_mhz
    if let Some(mhz) = base_freq_mhz() {
        pos = write_str(&mut out, pos, " @ ");
        let ghz_int = mhz / 1000;
        let ghz_frac = (mhz % 1000) / 100;
        pos = write_num(&mut out, pos, ghz_int as u64);
        pos = write_str(&mut out, pos, ".");
        pos = write_num(&mut out, pos, ghz_frac as u64);
        pos = write_str(&mut out, pos, " GHz");
    }

    (out, pos)
}

/// Записывает модель CPU в /system/info/cpu-m
/// Записывает модель CPU в /system/info/cpu-m
pub fn write_cpu_file(start_dir: u32) -> bool {
    let (buf, len) = format_cpu_model();
    let mut line = [0u8; 130];
    line[..len].copy_from_slice(&buf[..len]);
    line[len] = b'\n';

    crate::zigfs::write_file(start_dir, b"/system/info/cpu-m", &line[..len + 1])
}
