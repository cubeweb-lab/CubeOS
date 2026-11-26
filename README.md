# CubeOS

**CubeOS** is a 64-bit operating system written from scratch in Rust (with elements of Assembly and C). The project is created for learning low-level programming, OS architecture, and hardware interaction.

[![Build Status](https://github.com/cubeweb-lab/CubeOS/actions/workflows/rust.yml/badge.svg)](https://github.com/cubeweb-lab/CubeOS/actions)
[![Release - v1.0.0-dev-prealpha](https://img.shields.io/badge/Release-v1.0.0--dev--prealpha-2ea44f?logo=github)](https://github.com/cubeweb-lab/CubeOS/releases)
[![License - GPLv3](https://img.shields.io/badge/License-GPLv3-orange?logo=github)](https://github.com/cubeweb-lab/CubeOS/blob/main/LICENSE)

## Features (CubeCORE v1.0)
*   **Boot:** Multiboot2 via GRUB2.
*   **Architecture:** x86_64, Long Mode.
*   **Memory:** Physical Memory Manager (PMM), Multiboot2 memory map parsing.
*   **Interrupts:** IDT, PIC remapping (IRQ 0-15), PS/2 keyboard support.
*   **Graphics:** VESA Framebuffer support, custom logger to screen and serial port (COM1).
*   **Drivers:** ATA PIO driver for HDD operations.
*   **File System:** Custom FAT32 driver (in development).

## Build Requirements
To build CubeOS from source, you need a Linux environment (preferably Debian/Ubuntu or Arch) and the following set of tools:
*   `rustc` and `cargo` (Nightly toolchain)
*   `nasm`
*   `gcc`
*   `ld.lld` (LLVM linker)
*   `grub-mkrescue` (for creating ISO)
*   `xorriso`, `mtools`, `qemu-system-x86_64`

## Build and Run
```bash
# Clone the repository
git clone https://github.com/cubeweb-lab/CubeOS.git
cd CubeOS

# Build the kernel and csh
make all

# Create a bootable ISO
make iso
# OR create/update the HDD image (no root needed)
make hdd

# Run in QEMU
make run        # from the ISO
make run-hdd    # from the HDD image

# Host-side tests
make test
```

## Graphical user programs

User programs can access the VESA linear framebuffer through two syscalls exposed by `cube_syscall`: `SYS_FB_INFO` (100) returns the geometry and channel masks, and `SYS_FB_MAP` (101) maps the physical framebuffer into the process at a fixed user address. The frames stay owned by the firmware, so process teardown does not return them to the PMM.

```rust
use cube_syscall as sys;

let mut fb = sys::Framebuffer::new().expect("no 32-bit linear framebuffer");
let background = fb.rgb(0, 0, 128);
let red = fb.rgb(255, 0, 0);
fb.fill(background);
for y in 0..200 {
    for x in 0..200 {
        fb.put_pixel(x, y, red);
    }
}
```

`src/apps/gfxdemo` is a complete freestanding ELF example. Build it with `make gfxdemo`; `make hdd` also copies it to `/bin/gfxdemo`. To launch it at boot, set `cmdline="/bin/gfxdemo"` in `config/boot.cfg`, or use the same path with the `SYS_EXEC` syscall.

## SparkD and drivers

`config/boot.cfg` launches `/bin/sparkd` by default. SparkD scans `/system/drivers/*.drv`, loads each module through `SYS_LOAD_DRIVER`, then starts the programs listed in `/system/config/startup.cfg` (`program /path/to/binary`). With no startup config it launches `/bin/csh`.

A `.drv` module is a static ELF executable with a `.drvmeta` section carrying the `CUBEDRV\0` signature, metadata version, ABI version, flags, and a NUL-terminated name. Modules run as user-mode utility tasks; kernel-mode modules are rejected. `src/apps/demo_drv` is a minimal example, built and staged as `/system/drivers/demo.drv` by `make hdd`. See [docs/syscalls.md](docs/syscalls.md) for the syscall ABI.

## Booting without an ATA controller

The kernel only has a legacy ATA PIO driver, so an AHCI-only laptop or a USB stick is invisible to it. GRUB therefore loads an ext4 root image as a Multiboot2 module (`module2 /system/kernel/system.img` in `config/grub-hdd.cfg`, `module2 /boot/system.img` in `config/grub.cfg`) and the kernel mounts it as a RAM disk; the boot media itself is never accessed after GRUB hands over.

To build a bootable USB stick for a real PC, use the raw HDD image (it has an MBR, a FAT32 boot partition and GRUB installed in the MBR):

```bash
make hdd
sudo make hdd-grub
sudo dd if=cubeos.img of=/dev/sdX bs=4M status=progress conv=fsync
sudo sync
```

Replace `/dev/sdX` with the USB device (`lsblk`) — this overwrites its partition table. In UEFI, enable CSM/Legacy boot and disable Secure Boot; a UEFI-only boot needs a UEFI loader, which this image does not have. The ISO variant (`make iso`) can be written with Rufus in DD mode if the firmware boots the stick as a CD-ROM.

The `hdd` image also works in QEMU without special flags: `make run-hdd`. With no multiboot module present, root falls back to the ATA path, which is useful for the original disk image workflow.

## Syscall reference

The complete syscall ABI, including register conventions, error values, file descriptor limits, directory record layout, open flags, and framebuffer structures, is documented in [docs/syscalls.md](docs/syscalls.md).

Contributing

We welcome Pull Requests! If you want to make major changes, please create an Issue first to discuss them. Check out our issues.

License

This project is distributed under the GPL-3.0 License. See the LICENSE file for details.

Russian Documentation

Read the Russian documentation here: README_RU_ru.md
