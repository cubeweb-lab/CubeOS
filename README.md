# CubeOS
64-bit OS written in NASM

---

## Features

- Custom 3-stage bootloader
- Shell with 14 commands
- GUI-prototype(alpha-testing cooming soon)
- ATA PIO Driver
- FAT32 Parser(in development)
- 64-bit Long Mode

---

## System Requirements

- CPU: x86_64 with Long Mode Support
- RAM: 8 MB(Recommended 64-128 MB)
- HDD: 20 MB
- Display: 80x25

## Build and Run (for Windows) 
- Run build.bat for compiling and building source code to HDD-image
- Run in QEMU: qemu-system-x86_64 -drive format=raw file=build\cubeos.img -m 128M
warning: you need install NASM for compiling source code!

## License
Copyright © 2026, CubeDev
Licensed under the BSD 3-Clause License. 
See LICENSE.md file for details

## Author
CubeDev, 2026
GitHub: github.com/cubeweb-lab • Website https://cubeweb-lab.github.io/cubeweb.github.io/
