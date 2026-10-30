#![no_std]
#![feature(abi_x86_interrupt)]
#![allow(static_mut_refs)]

mod serial;
mod vga;
mod multiboot;
mod idt;
mod interrupts;
mod apic;
mod memory;
mod sync;
mod panic;
mod fs;
mod vesa_log;
mod ata;
mod zigfs;
mod checks;
mod elf;
mod loader;
mod tss;
mod ring3;
mod syscall;
mod sched;
mod vmm;

use multiboot::MultibootInfo;
use core::arch::asm;
use core::sync::atomic::Ordering;
use fs::FSManager;
use ata::AtaDrive;
use sched::Qos;

fn get_cpu_info() -> ([u8; 12], u32) {
    let mut vendor = [0u8; 12];
    let mut eax: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov [{vendor}], ebx",
            "mov [{vendor} + 4], edx",
            "mov [{vendor} + 8], ecx",
            "pop rbx",
            inout("eax") 0u32 => eax,
                         out("ecx") _,
                         out("edx") _,
                         vendor = in(reg) vendor.as_mut_ptr(),
        );
        core::arch::asm!(
            "push rbx", "cpuid", "pop rbx",
            inout("eax") 1u32 => eax,
                         out("ecx") _, out("edx") _,
        );
    }
    (vendor, eax)
}

fn write_num(buf: &mut [u8], mut n: usize) -> &str {
    let mut i = buf.len();
    if n == 0 { i -= 1; buf[i] = b'0'; }
    else { while n > 0 && i > 0 { i -= 1; buf[i] = (n % 10) as u8 + b'0'; n /= 10; } }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}

fn write_hex(buf: &mut [u8], n: u64) -> &str {
    for i in 0..buf.len() {
        let nibble = ((n >> ((buf.len() - 1 - i) * 4)) & 0xF) as u8;
        buf[i] = if nibble < 10 { b'0' + nibble } else { b'A' + nibble - 10 };
    }
    core::str::from_utf8(buf).unwrap_or("????????")
}

pub fn log_both(vesa_log: &mut Option<vesa_log::VesaLogger>, level: &str, msg: &str) {
    match level {
        "OK"    => serial::log_ok(msg),
        "INFO"  => serial::log_info(msg),
        "WARN"  => serial::log_warn(msg),
        "ERROR" => serial::log_error(msg),
        _       => serial::send_string(msg),
    }
    if let Some(ref mut logger) = vesa_log {
        logger.log(level, msg);
    }
}

#[no_mangle]
pub extern "C" fn task_a() -> ! {
    loop {
        crate::serial::send_string("A");
        for _ in 0..1_000_000 { unsafe { core::arch::asm!("nop") }; }
    }
}

#[no_mangle]
pub extern "C" fn task_b() -> ! {
    loop {
        crate::serial::send_string("B");
        for _ in 0..1_000_000 { unsafe { core::arch::asm!("nop") }; }
    }
}

#[no_mangle]
pub extern "C" fn rust_main(magic: u32, info_addr: u32) -> ! {
    // 1. Multiboot
    let mb_info = unsafe { MultibootInfo::parse(info_addr) };
    let mut buf = [0u8; 20];

    // 2. Serial + VGA
    serial::init();
    serial::send_string("\n========================================\n");
    serial::send_string("  CubeCORE v1.0 — Kernel Boot Sequence\n");
    serial::send_string("========================================\n");
    vga::init();

    // 3. Magic
    if magic != 0x36d76289 {
        serial::log_error("Invalid Multiboot2 magic!");
        serial::send_string("  Got: 0x");
        serial::send_string(write_hex(&mut buf, magic as u64));
        serial::send_string("\n");
        loop { unsafe { core::arch::asm!("cli; hlt") } }
    }
    serial::log_ok("Multiboot2 magic valid");

    // 4. VESA logger
    let mut vesa_log = if let Some(fb) = mb_info.framebuffer {
        if fb.addr != 0 && fb.width > 0 && fb.height > 0 {
            let mut logger = vesa_log::VesaLogger::new(fb.addr, fb.width, fb.height, fb.pitch);
            vesa_log::console_init(fb.addr, fb.width, fb.height, fb.pitch);
            logger.write_string("========================================\n");
            logger.write_string("  CubeCORE v1.0 — Kernel Boot Sequence\n");
            logger.write_string("========================================\n\n");
            logger.log("OK", "Framebuffer initialized");
            Some(logger)
        } else { None }
    } else { None };

    // 5. CPU info
    let (vendor, eax) = get_cpu_info();
    let family = ((eax >> 8) & 0xF) + ((eax >> 20) & 0xFF);
    let model = ((eax >> 4) & 0xF) + (((eax >> 16) & 0xF) << 4);
    let stepping = eax & 0xF;

    serial::send_string("[INFO ] CPU: ");
    serial::send_string(core::str::from_utf8(&vendor).unwrap_or("Unknown"));
    serial::send_string(", Family "); serial::send_string(write_num(&mut buf, family as usize));
    serial::send_string(", Model "); serial::send_string(write_num(&mut buf, model as usize));
    serial::send_string(", Stepping "); serial::send_string(write_num(&mut buf, stepping as usize));
    serial::send_string("\n");

    if let Some(ref mut logger) = vesa_log {
        logger.write_string("[INFO ] CPU: ");
        logger.write_string(core::str::from_utf8(&vendor).unwrap_or("Unknown"));
        logger.write_string(", Family "); logger.write_string(write_num(&mut buf, family as usize));
        logger.write_string(", Model "); logger.write_string(write_num(&mut buf, model as usize));
        logger.write_string(", Stepping "); logger.write_string(write_num(&mut buf, stepping as usize));
        logger.write_string("\n");
    }

    // 6. Multiboot info
    serial::send_string("[INFO ] Multiboot2 info at 0x");
    serial::send_string(write_hex(&mut buf, info_addr as u64));
    serial::send_string(", size ");
    serial::send_string(write_num(&mut buf, mb_info.total_size as usize));
    serial::send_string(" bytes\n");

    if let Some(ref mut logger) = vesa_log {
        logger.write_string("[INFO ] Multiboot2 info at 0x");
        logger.write_string(write_hex(&mut buf, info_addr as u64));
        logger.write_string(", size ");
        logger.write_string(write_num(&mut buf, mb_info.total_size as usize));
        logger.write_string(" bytes\n");
    }

    // 7. Memory map
    serial::send_string("[INFO ] Memory map: ");
    serial::send_string(write_num(&mut buf, mb_info.memory_map.len()));
    serial::send_string(" entries\n");

    for (idx, entry) in mb_info.memory_map.iter().enumerate() {
        serial::send_string("  [");
        serial::send_string(write_num(&mut buf, idx));
        serial::send_string("] ");
        let type_str = match entry.typ {
            1 => "Available ", 2 => "Reserved  ", 3 => "ACPI Rec  ",
            4 => "ACPI NVS  ", 5 => "Bad Mem   ", _ => "Unknown   ",
        };
        serial::send_string(type_str);
        serial::send_string(" 0x");
        serial::send_string(write_hex(&mut buf, entry.base_addr));
        serial::send_string(" (");
        serial::send_string(write_num(&mut buf, (entry.length / 1024) as usize));
        serial::send_string(" KB)\n");
    }

    let total_mb = mb_info.total_memory() / 1024 / 1024;
    let avail_mb = mb_info.available_memory() / 1024 / 1024;
    serial::send_string("[INFO ] Total RAM: ");
    serial::send_string(write_num(&mut buf, total_mb as usize));
    serial::send_string(" MB, Available: ");
    serial::send_string(write_num(&mut buf, avail_mb as usize));
    serial::send_string(" MB\n");

    // 8. PMM
    memory::init(mb_info.memory_map);
    serial::send_string("[INFO ] PMM: ");
    serial::send_string(write_num(&mut buf, memory::free_pages()));
    serial::send_string(" free pages (");
    serial::send_string(write_num(&mut buf, memory::free_memory_mb() as usize));
    serial::send_string(" MB)\n");
    serial::log_ok("Physical Memory Manager initialized");

    vmm::init();
    serial::log_ok("Virtual memory manager initialized");

    // 9. IDT
    unsafe { idt::init_idt(); }
    serial::log_ok("IDT initialized (256 entries)");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "IDT initialized"); }

    // 10. PIC remap — для совместимости, потом отключим
    interrupts::remap_pic();
    serial::log_ok("PIC remapped");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "PIC remapped"); }

    // 11. Local APIC
    let lapic_base = apic::enable();
    apic::init();
    serial::send_string("[APIC] Local APIC base = 0x");
    serial::send_string(write_hex(&mut buf, lapic_base));
    serial::send_string("\n");
    serial::log_ok("Local APIC initialized");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "Local APIC initialized"); }

    // 12. APIC timer 100 Hz
    apic::timer_init_periodic(100);
    serial::log_ok("APIC timer 100 Hz");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "APIC timer 100 Hz"); }

    //13. Scheduler init
    sched::init();
    serial::log_ok("Scheduler initialized");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "Scheduler initialized"); }

    // 14. I/O APIC: клавиатура IRQ1 → вектор 0x21
    apic::ioapic_set_irq(1, 0x21, 0);
    serial::log_ok("I/O APIC IRQ1 → 0x21");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "I/O APIC IRQ1"); }

    // 15. Отключить PIC
    apic::pic_disable();
    serial::log_ok("PIC disabled");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "PIC disabled"); }

    // 16. Прерывания
    unsafe { core::arch::asm!("sti"); }
    serial::log_ok("Interrupts enabled (IF=1)");
    if let Some(ref mut logger) = vesa_log { logger.log("OK", "Interrupts enabled"); }

    // 17. ATA + FAT32
    unsafe { asm!("cli"); }
    let mut ata = AtaDrive::new();
    let mut fs = FSManager::new();
    ata.init();

    log_both(&mut vesa_log, "INFO", "Initializing ATA controller...");

    if ata.exists {
        log_both(&mut vesa_log, "OK", "ATA controller detected");

        if fs.mount_fat32(&ata, 1) {
            log_both(&mut vesa_log, "OK", "FAT32 filesystem mounted");

            let mut config_buf = [0u8; 4096];
            if let Some(size) = fs.read_file(&ata, "/SYSTEM/CONFIG/BOOT.CFG", &mut config_buf) {
                if let Ok(text) = core::str::from_utf8(&config_buf[..size]) {
                    log_both(&mut vesa_log, "OK", "Boot config loaded");

                    if let Some(partition_lba) = fs.fat32.as_ref().map(|f| f.partition_start) {
                        zigfs::verify_file(partition_lba, "/SYSTEM/CONFIG/BOOT.CFG", &config_buf[..size]);
                    }

                    let cmdline = checks::config_cmdline(text);

                    if let Some(cmd) = cmdline {
                        log_both(&mut vesa_log, "INFO", "Launching program from cmdline...");
                        match loader::exec_user(&ata, &fs, cmd, sched::Qos::Interactive) {
                            Some(pid) => {
                                log_both(&mut vesa_log, "OK", "Program launched (user task)");
                                let mut nb = [0u8; 20];
                                serial::send_string("[SCHED] pid=");
                                serial::send_string(write_num(&mut nb, pid as usize));
                                serial::send_string("\n");
                            }
                            None => {
                                log_both(&mut vesa_log, "ERROR", "Failed to launch program");
                            }
                        }
                    } else {
                        log_both(&mut vesa_log, "INFO", "No cmdline, skipping launch");
                    }

                    kernel_main_loop()
                }
                if let Ok(text) = core::str::from_utf8(&config_buf[..size]) {
                    for line in text.lines() {
                        log_both(&mut vesa_log, "CFG", line);
                    }
                }
            } else {
                log_both(&mut vesa_log, "WARN", "boot.cfg not found");
            }
        } else {
            log_both(&mut vesa_log, "ERROR", "Failed to mount FAT32");
        }
    } else {
        log_both(&mut vesa_log, "WARN", "No ATA drive detected");
    }

    serial::send_string("\n[  OK ] CubeCORE v1.0 boot complete!\n");
    serial::send_string("========================================\n\n");

    if let Some(ref mut logger) = vesa_log {
        logger.write_string("\n[  OK ] CubeCORE v1.0 boot complete!\n");
        logger.write_string("========================================\n\n");
    }

    // === Тест планировщика: две задачи ===

    let stack_a = memory::alloc_page().expect("stack A");
    let stack_b = memory::alloc_page().expect("stack B");
    unsafe {
        core::ptr::write_bytes(stack_a as *mut u8, 0, 4096);
        core::ptr::write_bytes(stack_b as *mut u8, 0, 4096);
    }
    extern "C" { static _kernel_end: u8; }

    let stack_a = memory::alloc_page().expect("stack A");
    let stack_b = memory::alloc_page().expect("stack B");
    let kend = unsafe { &_kernel_end as *const u8 as u64 };
    serial::send_string("[SCHED] _kernel_end = 0x");
    serial::send_string(write_hex(&mut buf, kend));
    serial::send_string("[SCHED] task_a = 0x");
    serial::send_string(write_hex(&mut buf, task_a as u64));
    serial::send_string("\n");
    serial::send_string("[SCHED] task_b = 0x");
    serial::send_string(write_hex(&mut buf, task_b as u64));
    serial::send_string("\n");

    if stack_a < kend || stack_b < kend {
        serial::log_error("PMM returned stack below kernel end!");
    }
    sched::spawn(task_a, stack_a, 1, sched::Qos::Interactive, b"task_a");
    sched::spawn(task_b, stack_b, 1, sched::Qos::Utility, b"task_b");
    serial::log_ok("Test tasks A/B spawned");

    kernel_main_loop()
}

#[no_mangle]
pub extern "C" fn kernel_main_loop() -> ! {
    crate::serial::send_string("\n[KERNEL] Back in main loop\n");

    let mut last_ticks = 0u64;
    let mut buf = [0u8; 20];

    loop {
        // Проверка APIC тиков раз в ~секунду (100 Hz → +100)
        let t = idt::TICKS.load(Ordering::Relaxed);
        if t >= last_ticks + 100 {
            last_ticks = t;
            crate::serial::send_string("[APIC] ticks=");
            crate::serial::send_string(write_num(&mut buf, t as usize));
            crate::serial::send_string("\n");
        }

        if let Some(sc) = interrupts::keyboard_pop() {
            crate::serial::send_string("[INPUT] scancode=0x");
            let mut h = [0u8; 2];
            h[0] = if (sc >> 4) < 10 { b'0' + (sc >> 4) } else { b'A' + (sc >> 4) - 10 };
            h[1] = if (sc & 0xF) < 10 { b'0' + (sc & 0xF) } else { b'A' + (sc & 0xF) - 10 };
            crate::serial::send_string(core::str::from_utf8(&h).unwrap_or("?"));
            crate::serial::send_string("\n");
        }

        unsafe { core::arch::asm!("hlt") }
    }
}
