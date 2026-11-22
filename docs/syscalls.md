# CubeOS Syscall Reference

This document describes the syscall ABI implemented by CubeOS and the user-space wrappers exposed by `src/libs/cube_syscall`.

## Calling convention

User code invokes a syscall with the `syscall` instruction.

| Item | Register |
|---|---|
| Syscall number | `rax` |
| First argument | `rdi` |
| Second argument | `rsi` |
| Third argument | `rdx` |
| Fourth argument | `r10` |
| Fifth argument | `r8` |
| Sixth argument | `r9` |
| Return value | `rax` |

The kernel dispatcher currently consumes only the syscall number and the first three arguments. The fourth through sixth arguments are preserved by the entry stub but are not passed to the dispatcher. `rcx` and `r11` are clobbered by `syscall`.

Most calls return `0` or a non-negative result on success and `u64::MAX` (`0xFFFF_FFFF_FFFF_FFFF`, or `-1` as `isize`) on failure. Calls that return a byte count return that count directly. A few calls have special failure values; these are documented explicitly below.

Buffers passed to the kernel must be in user address space. The kernel validates pointer ranges for most pointer-taking calls and rejects a null pointer or a zero-length range. The checked user range ends at `0x0000_7FFF_FFFF_FFFF`.

## Quick reference

| Number | Name | Arguments | Success return |
|---:|---|---|---|
| 0 | `SYS_READ` | `fd`, `buf`, `len` | Bytes read |
| 1 | `SYS_WRITE` | `fd`, `buf`, `len` | Bytes written |
| 2 | `SYS_OPEN` | `flags`, `path`, `path_len` | File descriptor |
| 3 | `SYS_CLOSE` | `fd` | `0` |
| 5 | `SYS_EXIT` | `code` | Does not return |
| 11 | `SYS_GETDENTS` | `fd`, `buf`, `len` | Bytes written |
| 12 | `SYS_GETCWD` | `buf`, `len` | Bytes copied |
| 13 | `SYS_CHDIR` | `path`, `path_len` | `0` |
| 14 | `SYS_MKDIR` | `path`, `path_len`, `mode` | `0` |
| 34 | `SYS_UNLINK` | `path`, `path_len` | `0` |
| 50 | `SYS_FSTAT` | Not implemented | None |
| 78 | `SYS_DEBUG` | `ignored`, `buf`, `len` | Bytes written |
| 82 | `SYS_RENAME` | `old_new_buf`, `total_len` | `0` |
| 99 | `SYS_EXEC` | `path`, `path_len` | New PID |
| 100 | `SYS_FB_INFO` | `out`, `out_len` | `0` |
| 101 | `SYS_FB_MAP` | None | User VA |

Unknown syscall numbers are logged to the serial console as `[SYSCALL] unknown num=N` and return `0`.

## File descriptors

Descriptors `0`, `1`, and `2` are the standard input, standard output, and standard error console descriptors. Open file descriptors are allocated from `3` through `15`, so a process can have at most 13 files or directories open at once.

Files opened for writing are buffered in memory. Each write descriptor has a 4096-byte buffer, and the data is written back to the filesystem when the descriptor is closed.

## `SYS_READ` (0)

```rust
let n = sys::read(fd, &mut buf);
```

Reads from the keyboard when `fd == 0`. The call blocks until at least one key is available, then returns bytes until a newline or until the buffer is full. Input is echoed to the serial port and the VESA console. There is no line-editing or backspace handling in the kernel.

For an open file descriptor, reads at most `min(len, file_size - offset)` bytes and advances the file offset. It returns `0` at end of file. Reading from a directory returns `0`. Reading from a write-only descriptor or an invalid descriptor returns `u64::MAX`.

The destination buffer must be non-null, non-empty, and entirely in user space.

## `SYS_WRITE` (1)

```rust
let n = sys::write(fd, data);
```

For `fd` values `0`, `1`, and `2`, writes the bytes to both the serial port and the VESA text console. The length must be between 1 and 65536 bytes. The console path returns the number of bytes written, or `0` for an invalid buffer or length; it does not perform the full user-range check used by file reads and writes.

For a descriptor opened with `O_WRONLY` or `O_RDWR`, copies data into the descriptor's 4096-byte write buffer and advances its position. The call returns the number of bytes copied. It returns `u64::MAX` if the descriptor is not writable, the buffer is invalid, or the data would exceed 4096 bytes. Data is persisted only when the descriptor is closed.

## `SYS_OPEN` (2)

```rust
let fd = sys::open(path, flags);
```

Arguments are `flags`, `path`, and `path_len`. Paths are at most 256 bytes. Relative paths are resolved against the process's current working directory.

Read-only opens accept files and directories. Directories receive a directory descriptor. Passing `O_DIRECTORY` for a non-directory fails. Write opens require `O_WRONLY` or `O_RDWR`; `O_CREAT` creates a missing file, `O_TRUNC` discards existing contents, and `O_APPEND` starts writing at the end. Directories and files larger than 4096 bytes cannot be opened for writing. The call returns a descriptor in the range `3..=15` or `u64::MAX`.

### Open flags

| Flag | Value | Meaning |
|---|---:|---|
| `O_RDONLY` | `0` | Read-only |
| `O_WRONLY` | `0o1` | Write-only |
| `O_RDWR` | `0o2` | Read and write |
| `O_CREAT` | `0o100` | Create if missing |
| `O_TRUNC` | `0o1000` | Truncate existing contents |
| `O_APPEND` | `0o2000` | Start at end of file |
| `O_DIRECTORY` | `0o200000` | Require a directory |
| `O_CLOEXEC` | `0o2000000` | Declared but not implemented |

## `SYS_CLOSE` (3)

```rust
let result = sys::close(fd);
```

Closes a descriptor in the range `0..=15`. Closing a write descriptor flushes its buffer to the filesystem and returns the flush result (`0` or `u64::MAX`). Closing a non-write descriptor returns `0`. An out-of-range descriptor returns `u64::MAX`.

## `SYS_EXIT` (5)

```rust
sys::exit(code);
```

Terminates the calling user task. The exit code is printed to the serial console. The call does not return; control goes back to the kernel main loop.

## `SYS_GETDENTS` (11)

```rust
let n = sys::getdents(fd, &mut buf);
```

Reads directory entries from a directory descriptor into a packed byte buffer. The descriptor remembers the next entry index, so repeated calls continue through the directory.

Each record has the following layout:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 8 | Filesystem cluster/inode identifier |
| 8 | 8 | Directory index |
| 16 | 2 | Record length, including padding |
| 18 | 1 | Entry type: `DT_DIR` (`4`) or `DT_REG` (`8`) |
| 19 | varies | NUL-terminated entry name |
| remainder | padding | Pad to the next 8-byte boundary |

The record length is `(19 + name_len + 1 + 7) & !7`. Records are written in native little-endian x86_64 layout. The return value is the number of bytes written to the buffer. If the next record does not fit, the call returns the bytes written so far; a later call continues with the same entry. A call on a non-directory descriptor or an invalid buffer returns `u64::MAX`.

## `SYS_GETCWD` (12)

```rust
let n = sys::getcwd(&mut buf);
```

Copies the absolute current working directory path into `buf`. At most `len` bytes are copied; the result is truncated if the buffer is too small and is not NUL-terminated. Returns the number of bytes copied. A null buffer or zero length returns `u64::MAX`.

## `SYS_CHDIR` (13)

```rust
let result = sys::chdir(path);
```

Resolves `path` against the current working directory, requires the target to exist and be a directory, and updates the process working directory. Paths are at most 256 bytes. Returns `0` on success or `u64::MAX` on failure.

## `SYS_MKDIR` (14)

```rust
let result = sys::mkdir(path, mode);
```

Creates a directory. The `mode` argument is accepted but ignored by the current implementation. Paths are at most 256 bytes. Returns `0` on success or `u64::MAX` on failure.

## `SYS_UNLINK` (34)

```rust
let result = sys::unlink(path);
```

Removes a file. Directories are rejected; there is no `rmdir` syscall in this ABI. Paths are at most 256 bytes. Returns `0` on success or `u64::MAX` on failure.

## `SYS_FSTAT` (50)

`SYS_FSTAT` is declared by the user-space constants but has no kernel handler. Calling it reaches the unknown-syscall fallback, logs the number, and returns `0`. Do not use it as a status call.

## `SYS_DEBUG` (78)

```rust
let n = sys::debug("message");
```

Writes `[USER] `, the supplied bytes, and a newline to the serial console only. The first argument is ignored. The maximum length is 65536 bytes. Returns the number of bytes supplied, or `0` for an invalid pointer or length. This call does not validate that the buffer is in user space.

## `SYS_RENAME` (82)

```rust
let result = sys::rename(old_path, new_path);
```

Both paths are packed into one buffer as `old_path\0new_path`; the second argument is the total buffer length. The buffer is limited to 512 bytes. The first NUL byte separates the two paths. Returns `0` on success or `u64::MAX` on failure.

The user-space wrapper builds this buffer automatically. There is no separate old-path/new-path register pair.

## `SYS_EXEC` (99)

```rust
let pid = sys::syscall2(SYS_EXEC, path.as_ptr() as usize, path_len_with_nul);
```

Loads an ELF executable and starts it as a new user task. The path is read from the filesystem root, must include a NUL terminator, and is limited to 512 bytes total. The ELF image is limited to 65536 bytes.

On success, returns the new task's PID. The calling task continues running; `SYS_EXEC` does not replace the caller's address space. On failure, returns `u64::MAX` and logs the reason to the serial console.

## `SYS_FB_INFO` (100)

```rust
let mut info = sys::FbInfo::default();
let result = sys::syscall2(SYS_FB_INFO, &mut info as *mut _ as usize, core::mem::size_of::<sys::FbInfo>());
```

Writes the framebuffer description to the supplied user buffer. The buffer must be at least 44 bytes. Returns `0` on success or `u64::MAX` if no framebuffer is available or the buffer is invalid.

### `FbInfo` layout

| Offset | Size | Field |
|---:|---:|---|
| 0 | 4 | `width` in pixels |
| 4 | 4 | `height` in pixels |
| 8 | 4 | `pitch` in bytes per scanline |
| 12 | 4 | `bpp` in bits per pixel |
| 16 | 4 | `fb_type` (`1` = direct RGB) |
| 20 | 4 | `red_field_position` |
| 24 | 4 | `red_mask_size` |
| 28 | 4 | `green_field_position` |
| 32 | 4 | `green_mask_size` |
| 36 | 4 | `blue_field_position` |
| 40 | 4 | `blue_mask_size` |

All fields are `u32`. The convenience type `sys::Framebuffer` accepts only direct-RGB 32-bit framebuffers with a pitch of at least `width * 4`.

## `SYS_FB_MAP` (101)

```rust
let addr = sys::syscall0(SYS_FB_MAP);
```

Maps the physical linear framebuffer into the calling process's address space at the fixed user address `0x0000_6000_0000_0000`. The mapping is cached: repeated calls return the same address. Returns the user virtual address on success or `u64::MAX` if no framebuffer is available.

The framebuffer frames belong to the firmware. Process teardown removes the user mappings but does not return those frames to the physical memory manager.

## Not implemented

The user-space constants also declare `SEEK_SET`, `SEEK_CUR`, and `SEEK_END`, but there is no `lseek` syscall and file offsets cannot be changed explicitly. The only seek behavior is the automatic offset advance performed by `SYS_READ` and `SYS_WRITE`.
