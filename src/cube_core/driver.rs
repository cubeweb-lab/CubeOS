// Driver loader and registry. A .drv file is a static ELF executable carrying
// a `.drvmeta` section; it runs as a user-mode utility task, never in ring 0.
use crate::drv_meta;
use crate::fs::FSManager;
use crate::sched::Qos;
use crate::sync::SpinLock;
use crate::vmm::AddressSpace;

pub const MAX_DRIVER_SIZE: usize = 65536;
pub const MAX_DRIVERS: usize = 16;
pub const DRIVER_INFO_SIZE: usize = 48;

#[repr(C, align(4096))]
struct DriverBuf([u8; MAX_DRIVER_SIZE]);

#[link_section = ".program_buf"]
static mut DRIVER_BUF: DriverBuf = DriverBuf([0; MAX_DRIVER_SIZE]);

#[derive(Clone, Copy)]
struct Entry {
    name: [u8; 32],
    pid: u64,
    abi: u32,
    flags: u32,
}

static mut DRIVERS: [Option<Entry>; MAX_DRIVERS] = [None; MAX_DRIVERS];
static LOCK: SpinLock = SpinLock::new();

/// Loads a .drv ELF and starts it as a utility task. Returns its PID.
pub fn load(fs: &FSManager, path: &str) -> Option<u64> {
    unsafe {
        let size = fs.read_file(path, &mut DRIVER_BUF.0)?;
        let (offset, length) = drv_meta::section(&DRIVER_BUF.0[..size], drv_meta::DRV_SECTION)?;
        let meta = drv_meta::parse(&DRIVER_BUF.0[offset..offset + length])?;
        if !has_space() {
            crate::serial::send_string("[DRV] registry is full\n");
            return None;
        }

        let name = drv_meta::name(&meta);
        let mut aspace = AddressSpace::new()?;
        let pid = match crate::loader::spawn_program(
            &mut aspace,
            &DRIVER_BUF.0[..size],
            name,
            Qos::Utility,
        ) {
            Some(pid) => pid,
            None => {
                crate::vmm::destroy_pml4(aspace.pml4_phys());
                return None;
            }
        };
        register(&meta, pid);
        crate::kprintln!("[DRV] loaded {} as pid={}", name, pid);
        Some(pid)
    }
}

fn has_space() -> bool {
    LOCK.lock();
    let free = unsafe { DRIVERS.iter().any(|slot| slot.is_none()) };
    LOCK.unlock();
    free
}

fn register(meta: &drv_meta::DrvMeta, pid: u64) {
    LOCK.lock();
    unsafe {
        for slot in DRIVERS.iter_mut() {
            if slot.is_none() {
                *slot = Some(Entry {
                    name: meta.name,
                    pid,
                    abi: meta.abi,
                    flags: meta.flags,
                });
                break;
            }
        }
    }
    LOCK.unlock();
}

pub fn count() -> usize {
    LOCK.lock();
    let n = unsafe { DRIVERS.iter().filter(|slot| slot.is_some()).count() };
    LOCK.unlock();
    n
}

/// Writes a 48-byte record: name[32], pid(u64 LE), abi(u32 LE), flags(u32 LE).
pub fn info(index: usize, out: &mut [u8; DRIVER_INFO_SIZE]) -> bool {
    LOCK.lock();
    let result = unsafe {
        if index >= DRIVERS.len() {
            None
        } else {
            DRIVERS[index]
        }
    };
    LOCK.unlock();
    let Some(entry) = result else {
        return false;
    };
    out[..32].copy_from_slice(&entry.name);
    out[32..40].copy_from_slice(&entry.pid.to_le_bytes());
    out[40..44].copy_from_slice(&entry.abi.to_le_bytes());
    out[44..48].copy_from_slice(&entry.flags.to_le_bytes());
    true
}
