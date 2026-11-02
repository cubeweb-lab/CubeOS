pub const SYS_READ:     usize = 0;
pub const SYS_WRITE:    usize = 1;
pub const SYS_OPEN:     usize = 2;
pub const SYS_CLOSE:    usize = 3;
pub const SYS_EXIT:     usize = 5;
pub const SYS_GETDENTS: usize = 11;
pub const SYS_GETCWD:   usize = 12;
pub const SYS_CHDIR:    usize = 13;
pub const SYS_MKDIR:    usize = 14;
pub const SYS_UNLINK:   usize = 34;
pub const SYS_RENAME:   usize = 82;
pub const SYS_FSTAT:    usize = 50;
pub const SYS_DEBUG:    usize = 78;

pub const O_RDONLY:    usize = 0o0;
pub const O_WRONLY:    usize = 0o1;
pub const O_RDWR:      usize = 0o2;
pub const O_CREAT:     usize = 0o100;
pub const O_TRUNC:     usize = 0o1000;
pub const O_APPEND:    usize = 0o2000;
pub const O_DIRECTORY: usize = 0o200000;
pub const O_CLOEXEC:   usize = 0o2000000;

pub const SEEK_SET: usize = 0;
pub const SEEK_CUR: usize = 1;
pub const SEEK_END: usize = 2;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_REG:     u8 = 8;
pub const DT_DIR:     u8 = 4;

pub const STDIN:  usize = 0;
pub const STDOUT: usize = 1;
pub const STDERR: usize = 2;
