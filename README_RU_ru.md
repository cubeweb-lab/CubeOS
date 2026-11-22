# CubeOS

**CubeOS** — это 64-битная операционная система, написанная с нуля на языке Rust (с элементами ассемблера и C). Проект создаётся для изучения низкоуровневого программирования, архитектуры ОС и взаимодействия с железом.

[![Статус сборки](https://github.com/cubeweb-lab/CubeOS/actions/workflows/rust.yml/badge.svg)](https://github.com/cubeweb-lab/CubeOS/actions)
[![Release - v1.0.0-dev-prealpha](https://img.shields.io/badge/Release-v1.0.0--dev--prealpha-2ea44f?logo=github)](https://github.com/cubeweb-lab/CubeOS/releases)
[![License - GPLv3](https://img.shields.io/badge/License-GPLv3-orange?logo=github)](https://github.com/cubeweb-lab/CubeOS/blob/main/LICENSE)

##  Возможности (CubeCORE v1.0)
*   **Загрузка:** Multiboot2 через GRUB2.
*   **Архитектура:** x86_64, Long Mode.
*   **Память:** Менеджер физической памяти (PMM), парсинг карты памяти Multiboot2.
*   **Прерывания:** IDT, ремап PIC (IRQ 0-15), поддержка PS/2 клавиатуры.
*   **Графика:** Поддержка VESA Framebuffer, собственный логгер на экран и последовательный порт (COM1).
*   **Драйверы:** Драйвер ATA PIO для работы с жёстким диском.
*   **Файловая система:** Собственный драйвер FAT32 (в разработке).

##  Требования для сборки
Для сборки CubeOS из исходников вам понадобится Linux-окружение (желательно Debian/Ubuntu или Arch) и следующий набор инструментов:
*   `rustc` и `cargo` (ночная версия / Nightly toolchain)
*   `nasm`
*   `gcc`
*   `ld.lld` (линкер от LLVM)
*   `grub-mkrescue` (для создания ISO)
*   `xorriso`, `mtools`, `qemu-system-x86_64`

## Сборка и запуск
```bash
# Клонируем репозиторий
git clone https://github.com/cubeweb-lab/CubeOS.git
cd CubeOS

# Собираем ядро и csh
make all

# Создаём загрузочный ISO
make iso
# ИЛИ создаём/обновляем образ HDD (root не нужен)
make hdd

# Запускаем в QEMU
make run        # из ISO
make run-hdd    # из образа HDD

# Тесты host-части
make test
```

## Графические программы

Пользовательские программы получают доступ к линейному VESA-фреймбуферу через два syscall из `cube_syscall`: `SYS_FB_INFO` (100) возвращает геометрию и маски каналов, а `SYS_FB_MAP` (101) отображает физический фреймбуфер в адресное пространство процесса по фиксированному адресу. Кадры остаются собственностью прошивки, поэтому при завершении процесса они не возвращаются в PMM.

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

`src/apps/gfxdemo` — полноценный пример отдельного ELF. Соберите его командой `make gfxdemo`; `make hdd` также копирует его в `/bin/gfxdemo`. Для запуска при загрузке укажите `cmdline="/bin/gfxdemo"` в `config/boot.cfg` или запустите этот путь через syscall `SYS_EXEC`.

Участие в разработке

Мы рады Pull Request'ам! Если вы хотите внести серьёзные изменения, сначала создайте Issue, чтобы обсудить их. Посмотрите наши задачи (issues).

Лицензия

Проект распространяется под лицензией GPL-3.0. Подробности в файле LICENSE.

English Documentation

Read the English documentation here: README.md
