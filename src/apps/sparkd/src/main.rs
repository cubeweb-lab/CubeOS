#![no_std]
#![no_main]

// SparkD: the CubeOS service daemon. It loads drivers from /system/drivers
// and starts programs from /system/config/startup.cfg through syscalls only.
use cube_syscall as sys;

const MAX_LINE: usize = 256;
const MAX_PATH: usize = 256;
const MAX_ARGS: usize = 8;
const DRIVER_DIR: &[u8] = b"/system/drivers";
const STARTUP: &[u8] = b"/system/config/startup.cfg";
const DT_REG: u8 = 8;

fn emit(bytes: &[u8]) {
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            sys::write(sys::STDOUT, &bytes[start..i]);
            sys::write(sys::STDOUT, b"\r\n");
            start = i + 1;
        }
    }
    if start < bytes.len() {
        sys::write(sys::STDOUT, &bytes[start..]);
    }
}

fn out(s: &str) {
    emit(s.as_bytes());
}

fn err(s: &str) {
    sys::write(sys::STDERR, s.as_bytes());
}

fn write_num(n: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut i = buf.len();
    let mut value = n;
    if value == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while value > 0 && i > 0 {
            i -= 1;
            buf[i] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    &buf[i..]
}

fn tokenize<'a>(line: &'a str, args: &mut [&'a str; MAX_ARGS]) -> usize {
    let mut n = 0;
    for token in line.split_ascii_whitespace() {
        if n == MAX_ARGS {
            break;
        }
        args[n] = token;
        n += 1;
    }
    n
}

fn exec(args: &[&str]) -> bool {
    if args.is_empty() {
        err("sparkd: exec without a path\n");
        return false;
    }
    let mut packed: [&[u8]; MAX_ARGS] = [b""; MAX_ARGS];
    for (slot, arg) in packed.iter_mut().zip(args.iter().take(MAX_ARGS)) {
        *slot = arg.as_bytes();
    }
    let pid = sys::exec(&packed[..args.len().min(MAX_ARGS)]);
    if pid < 0 {
        err("sparkd: failed to launch ");
        err(args[0]);
        err("\n");
        return false;
    }
    out("sparkd: launched ");
    out(args[0]);
    out(" as pid=");
    let mut digits = [0u8; 20];
    emit(write_num(pid as u64, &mut digits));
    out("\n");
    true
}

fn load_driver(path: &[u8]) -> bool {
    let mut request = [0u8; MAX_PATH];
    if path.is_empty() || path.len() + 1 > request.len() {
        err("sparkd: driver path too long\n");
        return false;
    }
    request[..path.len()].copy_from_slice(path);
    request[path.len()] = 0;
    let pid = sys::load_driver(&request[..path.len() + 1]);
    if pid < 0 {
        err("sparkd: failed to load driver ");
        emit(path);
        err("\n");
        return false;
    }
    out("sparkd: loaded driver ");
    emit(path);
    out(" as pid=");
    let mut digits = [0u8; 20];
    emit(write_num(pid as u64, &mut digits));
    out("\n");
    true
}

fn scan_drivers() -> usize {
    let fd = sys::open(DRIVER_DIR, sys::O_RDONLY | sys::O_DIRECTORY);
    if fd < 0 {
        out("sparkd: no /system/drivers directory\n");
        return 0;
    }

    let mut buf = [0u8; 2048];
    let mut loaded = 0;
    loop {
        let n = sys::getdents(fd as usize, &mut buf);
        if n <= 0 {
            break;
        }
        let n = n as usize;
        let mut offset = 0;
        while offset + 19 <= n {
            let reclen = u16::from_le_bytes([buf[offset + 16], buf[offset + 17]]) as usize;
            if reclen < 20 || offset + reclen > n {
                break;
            }
            let record = &buf[offset..offset + reclen];
            let name_len = record[19..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(record.len() - 19);
            let name = &record[19..19 + name_len];
            if record[18] == DT_REG && name.ends_with(b".drv") {
                let mut path = [0u8; MAX_PATH];
                let prefix = b"/system/drivers/";
                if prefix.len() + name.len() <= path.len() {
                    path[..prefix.len()].copy_from_slice(prefix);
                    path[prefix.len()..prefix.len() + name.len()].copy_from_slice(name);
                    out("sparkd: loading ");
                    emit(&path[..prefix.len() + name.len()]);
                    out("\n");
                    if load_driver(&path[..prefix.len() + name.len()]) {
                        loaded += 1;
                    }
                }
            }
            offset += reclen;
        }
    }
    sys::close(fd as usize);
    loaded
}

fn report_drivers() {
    let count = sys::driver_count();
    out("sparkd: loaded drivers: ");
    let mut digits = [0u8; 20];
    emit(write_num(count as u64, &mut digits));
    out("\n");
    for index in 0..count {
        let mut info = sys::DriverInfo {
            name: [0; 32],
            pid: 0,
            abi: 0,
            flags: 0,
        };
        if sys::driver_info(index, &mut info) == 0 {
            out("sparkd: driver ");
            out(info.name());
            out(" pid=");
            emit(write_num(info.pid, &mut digits));
            out(" abi=");
            emit(write_num(info.abi as u64, &mut digits));
            out("\n");
        }
    }
}

fn handle_line(line: &[u8]) -> bool {
    let text = match core::str::from_utf8(line) {
        Ok(text) => text.trim(),
        Err(_) => return false,
    };
    if text.is_empty() || text.starts_with('#') {
        return false;
    }
    let mut args = [""; MAX_ARGS];
    let n = tokenize(text, &mut args);
    if n == 0 {
        return false;
    }
    match args[0] {
        "driver" if n > 1 => {
            load_driver(args[1].as_bytes());
            false
        }
        "program" | "exec" if n > 1 => exec(&args[1..n]),
        _ => false,
    }
}

fn load_startup() {
    let fd = sys::open(STARTUP, sys::O_RDONLY);
    if fd < 0 {
        out("sparkd: no startup config, launching /bin/csh\n");
        exec(&["/bin/csh"]);
        return;
    }

    let mut buf = [0u8; 1024];
    let mut line = [0u8; MAX_LINE];
    let mut line_len = 0;
    let mut programs = 0;
    loop {
        let n = sys::read(fd as usize, &mut buf);
        if n <= 0 {
            break;
        }
        for &byte in &buf[..n as usize] {
            if byte == b'\n' {
                if handle_line(&line[..line_len]) {
                    programs += 1;
                }
                line_len = 0;
            } else if line_len < MAX_LINE {
                line[line_len] = byte;
                line_len += 1;
            }
        }
    }
    if line_len > 0 && handle_line(&line[..line_len]) {
        programs += 1;
    }
    sys::close(fd as usize);
    if programs == 0 {
        out("sparkd: no programs in startup config, launching /bin/csh\n");
        exec(&["/bin/csh"]);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    out("\nCTK SparkD - CubeOS service daemon.\n");
    out("version 0.1.0-001-alpha prerelease (x86_64-cubeos-ctk)\n");
    out("Copyright (C) 2026 CubeDev team\n");
    out("License GPLv3+: GNU GPL version 3 or later\n\n");

    out("sparkd: scanning /system/drivers\n");
    let drivers = scan_drivers();
    out("sparkd: discovered ");
    let mut digits = [0u8; 20];
    emit(write_num(drivers as u64, &mut digits));
    out(" .drv file(s)\n");
    report_drivers();
    out("sparkd: loading startup programs\n");
    load_startup();

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    err("sparkd: panic\n");
    sys::exit(1)
}
