#![no_std]
#![no_main]

use cube_syscall as sys;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys::exit(1)
}

fn put_num(buf: &mut [u8; 20], mut n: usize) -> &str {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 { break; }
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}

fn put_hex(buf: &mut [u8; 10], v: u32) -> &str {
    let digits = b"0123456789ABCDEF";
    for i in 0..8 {
        buf[i] = digits[((v >> ((7 - i) * 4)) & 0xF) as usize];
    }
    core::str::from_utf8(&buf[..8]).unwrap_or("?")
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    sys::write(sys::STDOUT, b"[GFX] start\n");
    match sys::Framebuffer::new() {
        Some(mut fb) => {
            let info = fb.info();
            let mut buf = [0u8; 20];
            let mut hexbuf = [0u8; 10];
            sys::write(sys::STDOUT, b"[GFX] info: ");
            sys::write(sys::STDOUT, put_num(&mut buf, info.width as usize).as_bytes());
            sys::write(sys::STDOUT, b"x");
            sys::write(sys::STDOUT, put_num(&mut buf, info.height as usize).as_bytes());
            sys::write(sys::STDOUT, b" pitch=");
            sys::write(sys::STDOUT, put_num(&mut buf, info.pitch as usize).as_bytes());
            sys::write(sys::STDOUT, b" bpp=");
            sys::write(sys::STDOUT, put_num(&mut buf, info.bpp as usize).as_bytes());
            sys::write(sys::STDOUT, b"\n");

            let red = fb.rgb(255, 0, 0);
            let green = fb.rgb(0, 255, 0);
            let blue = fb.rgb(0, 0, 255);
            sys::write(sys::STDOUT, b" masks: ");
            sys::write(sys::STDOUT, put_num(&mut buf, info.red_field_position as usize).as_bytes());
            sys::write(sys::STDOUT, b"/");
            sys::write(sys::STDOUT, put_num(&mut buf, info.red_mask_size as usize).as_bytes());
            sys::write(sys::STDOUT, b" ");
            sys::write(sys::STDOUT, put_num(&mut buf, info.green_field_position as usize).as_bytes());
            sys::write(sys::STDOUT, b"/");
            sys::write(sys::STDOUT, put_num(&mut buf, info.green_mask_size as usize).as_bytes());
            sys::write(sys::STDOUT, b" ");
            sys::write(sys::STDOUT, put_num(&mut buf, info.blue_field_position as usize).as_bytes());
            sys::write(sys::STDOUT, b"/");
            sys::write(sys::STDOUT, put_num(&mut buf, info.blue_mask_size as usize).as_bytes());
            sys::write(sys::STDOUT, b" colors: ");
            sys::write(sys::STDOUT, put_hex(&mut hexbuf, red).as_bytes());
            sys::write(sys::STDOUT, b" ");
            sys::write(sys::STDOUT, put_hex(&mut hexbuf, green).as_bytes());
            sys::write(sys::STDOUT, b" ");
            sys::write(sys::STDOUT, put_hex(&mut hexbuf, blue).as_bytes());
            sys::write(sys::STDOUT, b"\n");

            fb.fill(fb.rgb(0, 0, 128));
            for y in 0..fb.height().min(200) {
                for x in 0..fb.width().min(200) {
                    fb.put_pixel(x, y, red);
                }
            }
            sys::write(sys::STDOUT, b"[GFX] painted\n");
        }
        None => {
            sys::write(sys::STDOUT, b"[GFX] no framebuffer\n");
        }
    }
    sys::exit(0)
}
