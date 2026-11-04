#![no_std]
#![no_main]

// SparkD: System initial utility that talks to the kernel only through syscalls.
use cube_syscall as sys;

const MAX_LINE: usize = 128;
const MAX_ARGS: usize = 8;
const MAX_PATH: usize = 256;
const DT_DIR: u8 = 4;

// Writes to stdout, converting "\n" to "\r\n" for serial terminals.
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

// Reads one line. The kernel echoes typed characters itself; backspace only needs its
// erase sequence here because the kernel moves the cursor but does not clear the cell.
fn read_line(buf: &mut [u8; MAX_LINE]) -> usize {
    let mut len = 0;
    loop {
        let mut b = [0u8; 1];
        if sys::read(sys::STDIN, &mut b) != 1 {
            core::hint::spin_loop();
            continue;
        }
        match b[0] {
            b'\n' => return len,
            0x08 => {
                if len > 0 {
                    len -= 1;
                    sys::write(sys::STDOUT, b" \x08");
                }
            }
            c if (0x20..0x7F).contains(&c) && len < MAX_LINE => {
                buf[len] = c;
                len += 1;
            }
            _ => {}
        }
    }
}

fn tokenize<'a>(line: &'a str, args: &mut [&'a str; MAX_ARGS]) -> usize {
    let mut n = 0;
    for tok in line.split_ascii_whitespace() {
        if n == MAX_ARGS {
            break;
        }
        args[n] = tok;
        n += 1;
    }
    n
}

fn write_text(fd: isize, args: &[&str]) {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            sys::write(fd as usize, b" ");
        }
        sys::write(fd as usize, arg.as_bytes());
    }
    sys::write(fd as usize, b"\n");
}

fn cmd_echo(args: &[&str]) {
    // "echo text > file" truncates the file, "echo text >> file" appends to it.
    if let Some(k) = args.iter().position(|a| *a == ">" || *a == ">>") {
        let target = match args.get(k + 1) {
            Some(t) => *t,
            None => {
                err("echo: missing file after redirection\n");
                return;
            }
        };
        let flags = sys::O_WRONLY | sys::O_CREAT | if args[k] == ">>" { sys::O_APPEND } else { sys::O_TRUNC };
        let fd = sys::open(target.as_bytes(), flags);
        if fd < 0 {
            err("echo: cannot write '");
            err(target);
            err("'\n");
            return;
        }
        write_text(fd, &args[..k]);
        sys::close(fd as usize);
        return;
    }
    write_text(sys::STDOUT as isize, args);
}

fn cmd_touch(path: &str) {
    let fd = sys::open(path.as_bytes(), sys::O_WRONLY | sys::O_CREAT);
    if fd < 0 {
        err("touch: cannot touch '");
        err(path);
        err("'\n");
        return;
    }
    sys::close(fd as usize);
}

fn cmd_cp(src: &str, dst: &str) {
    let input = sys::open(src.as_bytes(), sys::O_RDONLY);
    if input < 0 {
        err("cp: cannot read '");
        err(src);
        err("': no such file\n");
        return;
    }
    let output = sys::open(dst.as_bytes(), sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC);
    if output < 0 {
        sys::close(input as usize);
        err("cp: cannot write '");
        err(dst);
        err("'\n");
        return;
    }
    let mut buf = [0u8; 512];
    loop {
        let n = sys::read(input as usize, &mut buf);
        if n <= 0 {
            break;
        }
        let n = n as usize;
        if sys::write(output as usize, &buf[..n]) != n as isize {
            err("cp: copy limit reached (4 KiB per file)\n");
            break;
        }
    }
    sys::close(output as usize);
    sys::close(input as usize);
}

fn cmd_mv(src: &str, dst: &str) {
    if sys::rename(src.as_bytes(), dst.as_bytes()) < 0 {
        err("mv: cannot move '");
        err(src);
        err("' to '");
        err(dst);
        err("'\n");
    }
}

fn cmd_pwd() {
    let mut cwd = [0u8; MAX_PATH];
    let n = sys::getcwd(&mut cwd);
    if n > 0 {
        emit(&cwd[..n as usize]);
    }
    out("\n");
}

fn cmd_ls(path: &str) {
    let fd = sys::open(path.as_bytes(), sys::O_RDONLY);
    if fd < 0 {
        err("ls: cannot access '");
        err(path);
        err("': no such directory\n");
        return;
    }

    // Each getdents record: u64 cluster, u64 index, u16 reclen, u8 type, NUL-terminated name at offset 19.
    let mut buf = [0u8; 2048];
    loop {
        let n = sys::getdents(fd as usize, &mut buf);
        if n <= 0 {
            break;
        }
        let n = n as usize;
        let mut off = 0;
        while off + 19 <= n {
            let reclen = u16::from_le_bytes([buf[off + 16], buf[off + 17]]) as usize;
            if reclen < 20 || off + reclen > n {
                break;
            }
            let rec = &buf[off..off + reclen];
            let name_len = rec[19..].iter().position(|&b| b == 0).unwrap_or(rec.len() - 19);
            emit(&rec[19..19 + name_len]);
            if rec[18] == DT_DIR {
                out("/");
            }
            out("\n");
            off += reclen;
        }
    }
    sys::close(fd as usize);
}

fn cmd_cat(path: &str) {
    let fd = sys::open(path.as_bytes(), sys::O_RDONLY);
    if fd < 0 {
        err("cat: ");
        err(path);
        err(": no such file\n");
        return;
    }
    let mut buf = [0u8; 512];
    loop {
        let n = sys::read(fd as usize, &mut buf);
        if n <= 0 {
            break;
        }
        emit(&buf[..n as usize]);
    }
    sys::close(fd as usize);
}

fn run(args: &[&str]) {
    if args.is_empty() {
        err("exec: missing command\\n");
        return;
    }

    const SYS_EXEC: u64 = 99;

    // Буфер для пути + аргументов, разделённых \\0
    // Формат: path\\0arg1\\0arg2\\0...\\0\\0
    let mut exec_buf = [0u8; 512];
    let mut pos = 0;

    // Копируем путь
    let path = args[0];
    let path_bytes = path.as_bytes();
    if path_bytes.len() + 1 > exec_buf.len() {
        err("exec: path too long\\n");
        return;
    }
    exec_buf[pos..pos + path_bytes.len()].copy_from_slice(path_bytes);
    pos += path_bytes.len();
    exec_buf[pos] = 0; // \\0 после пути
    pos += 1;

    // Копируем аргументы
    for &arg in args.iter().skip(1) {
        let arg_bytes = arg.as_bytes();
        if pos + arg_bytes.len() + 1 > exec_buf.len() {
            err("exec: arguments too long\\n");
            return;
        }
        exec_buf[pos..pos + arg_bytes.len()].copy_from_slice(arg_bytes);
        pos += arg_bytes.len();
        exec_buf[pos] = 0; // \\0 после аргумента
        pos += 1;
    }
    exec_buf[pos] = 0; // Финальный \\0

    // Вызываем syscall
    // a1 = ptr to buffer, a2 = длина буфера
    let pid = unsafe { sys::syscall2(SYS_EXEC as usize, exec_buf.as_ptr() as usize, pos as usize) };

    if pid == usize::MAX {
        err("exec: failed to launch '");
        err(path);
        err("'\\n");
    } else {
        out("exec: launched '");
        out(path);
        out("' as pid=");
        // Выводим PID — простой способ без alloc
        let mut tmp = [0u8; 20];
        let mut i = 19;
        let mut n = pid as usize;
        if n == 0 {
            tmp[i] = b'0';
            i -= 1;
        } else {
            while n > 0 && i > 0 {
                tmp[i] = b'0' + (n % 10) as u8;
                n /= 10;
                i -= 1;
            }
        }
        out(core::str::from_utf8(&tmp[i+1..]).unwrap_or("?"));
        out("\\n");
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    out("\nCTK SparkD - system initial utility.\n");
    out("version 0.0.1-001-alpha prerelease (x86_64-cubeos-ctk)\n");
    out("This program from CTK(CubeToolKit) free-software pack.\n");
    out("Copyright (C) 2026 CubeDev team\n");
    out("License GPLv3+: GNU GPL version 3 or later\n");
    out("http://gnu.org/licenses/gpl.html\n");
    out("\n");
    out("You are free to change and redistribute it.\n");
    out("There is NO WARRANTY, to the extent permitted by law.\n");
    let program_path = &["/bin/csh"];
    run(program_path);

    loop { }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    err("sparkd: panic\n");
    sys::exit(1)
}
