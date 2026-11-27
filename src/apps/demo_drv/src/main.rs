#![no_std]
#![no_main]

use cube_syscall as sys;

#[repr(C)]
struct DrvMeta {
    magic: [u8; 8],
    version: u32,
    abi: u32,
    flags: u32,
    name: [u8; 32],
    description: [u8; 64],
    reserved: [u8; 12],
}

const fn padded<const N: usize>(text: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < text.len() {
        out[i] = text[i];
        i += 1;
    }
    out
}

#[used]
#[link_section = ".drvmeta"]
static META: DrvMeta = DrvMeta {
    magic: *b"CUBEDRV\0",
    version: 1,
    abi: 1,
    flags: 0,
    name: padded(b"demo"),
    description: padded(b"CubeOS demo driver: prints a startup message"),
    reserved: [0; 12],
};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    sys::write(sys::STDOUT, b"[demo drv] initialized\n");
    sys::debug("[demo drv] debug channel\n");
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    sys::debug("[demo drv] panic\n");
    sys::exit(1)
}
