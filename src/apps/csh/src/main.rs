#![no_std]
#![no_main]

// CubeShell: interactive shell that talks to the kernel only through syscalls.
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

fn prompt() {
    let mut cwd = [0u8; MAX_PATH];
    let n = sys::getcwd(&mut cwd);
    out("root@cubeos:");
    if n > 0 {
        emit(&cwd[..n as usize]);
    }
    out("# ");
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

fn cmd_version() {
    out("\nCTK shell - CubeOS terminal shell.\n");
    out("version 0.0.1-010-alpha release (x86_64-cubeos-ctk)\n");
    out("This program from CTK(CubeToolKit) free-software pack.\n");
    out("Copyright (C) 2026 CubeDev team\n");
    out("License GPLv3+: GNU GPL version 3 or later\n");
    out("http://gnu.org/licenses/gpl.html\n");
    out("\n");
    out("You are free to change and redistribute it.\n");
    out("There is NO WARRANTY, to the extent permitted by law.\n");
}

fn cmd_help() {
    out("commands: help, echo [text] [> file | >> file], pwd, cd <dir>, ls [dir], cat <file>,\n");
    out("     version, touch <file>, cp <src> <dst>, mv <src> <dst>, mkdir <dir>, rm <file>, exit\n");
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

fn cmd_v() {
    out("csh v0.0.1-0010-alpha release\n");
}

fn cubefetch() {
    let osr_path = "/system/info/os-release";
    let host_path = "/system/info/comp";
    let krnlr_path = "/system/info/kernel-release";
    let upt_path = "/system/info/uptime";
    let disp_path = "/system/info/display";
    let cpu_path = "/system/info/cpu-m";
    let de_path = "/system/info/desk-env";
    let wm_path = "/system/info/win-man";
    let wmt_path = "/system/info/wm-theme";
    let det_path = "/system/info/de-theme";
    let ic_path = "/system/info/icons";
    let font_path = "/system/info/font";
    let cur_path = "/system/info/cursor";
    let term_path = "/system/info/term";
    let tfont_path = "/system/info/term-font";

    out("\n");
    out("  ______________________    \n");
    out(" /                     /|    root@cubeos\n");
    out("/_____________________/ |   -----------------------\n");
    out("|                     | |   OS: ");
    cmd_cat(osr_path);
    out("|::::       ::        | |   Host: ");
    cmd_cat(host_path);
    out("|::         ::   .... | |   Kernel: ");
    cmd_cat(krnlr_path);
    out("|::   :: :: :::: :''' | |   Uptime: ");
    cmd_cat(upt_path);
    out("|:::: ::::: :::: .... | |   Packages: \n");
    out("|     ..... ....      | |   Shell: ");
    cmd_v();
    out("|     :   : :...      | |   Display: ");
    cmd_cat(disp_path);
    out("|     :...: ...:      | /   DE: ");
    cmd_cat(de_path);
    out("|_____________________|/    WM: ");
    cmd_cat(wm_path);
    out("                            WM Theme: ");
    cmd_cat(wmt_path);
    out("                            Theme: ");
    cmd_cat(det_path);
    out("                            Icons: ");
    cmd_cat(ic_path);
    out("                            Font: ");
    cmd_cat(font_path);
    out("                            Cursor: ");
    cmd_cat(cur_path);
    out("                            Terminal: ");
    cmd_cat(term_path);
    out("                            Terminal Font: ");
    cmd_cat(tfont_path);
    out("                            CPU: ");
    cmd_cat(cpu_path);
    out("                            GPU: ");


}

// Returns false when the shell should exit.
fn run(args: &[&str]) -> bool {
    match args[0] {
        "help" => cmd_help(),
        "version" => cmd_version(),
        "echo" => cmd_echo(&args[1..]),
        "pwd" => cmd_pwd(),
        "v" => cmd_v(),
        "cd" => {
            let target = args.get(1).copied().unwrap_or("/");
            if sys::chdir(target.as_bytes()) < 0 {
                err("cd: no such directory: ");
                err(target);
                err("\n");
            }
        }
        "ls" => cmd_ls(args.get(1).copied().unwrap_or(".")),
        "cat" => match args.get(1) {
            Some(path) => cmd_cat(path),
            None => err("usage: cat <file>\n"),
        },
        "mkdir" => match args.get(1) {
            Some(path) => {
                if sys::mkdir(path.as_bytes(), 0o755) < 0 {
                    err("mkdir: cannot create '");
                    err(path);
                    err("'\n");
                }
            }
            None => err("usage: mkdir <dir>\n"),
        },
        "rm" => match args.get(1) {
            Some(path) => {
                if sys::unlink(path.as_bytes()) < 0 {
                    err("rm: cannot remove '");
                    err(path);
                    err("'\n");
                }
            }
            None => err("usage: rm <file>\n"),
        },
        "touch" => match args.get(1) {
            Some(path) => cmd_touch(path),
            None => err("usage: touch <file>\n"),
        },
        "cp" => match (args.get(1), args.get(2)) {
            (Some(src), Some(dst)) => cmd_cp(src, dst),
            _ => err("usage: cp <src> <dst>\n"),
        },
        "mv" => match (args.get(1), args.get(2)) {
            (Some(src), Some(dst)) => cmd_mv(src, dst),
            _ => err("usage: mv <src> <dst>\n"),
        },
        "cubefetch" => cubefetch(),
        "exit" => return false,
        other => {
            err("csh: command not found: ");
            err(other);
            err("\n");
        }
    }
    true
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    out("\nCubeShell (csh). Type 'help' for commands.\n");

    let mut line = [0u8; MAX_LINE];
    loop {
        prompt();
        let len = read_line(&mut line);
        let text = match core::str::from_utf8(&line[..len]) {
            Ok(text) => text,
            Err(_) => continue,
        };

        let mut args = [""; MAX_ARGS];
        let argc = tokenize(text, &mut args);
        if argc == 0 {
            continue;
        }
        if !run(&args[..argc]) {
            sys::exit(0);
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    err("csh: panic\n");
    sys::exit(1)
}
