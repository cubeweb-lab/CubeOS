// src/cube_core/vesa_log.rs — вывод логов в framebuffer
mod font;

pub struct VesaLogger {
    fb: *mut u32,
    width: usize,
    height: usize,
    pitch: usize,
    x: usize,
    y: usize,
    fg: u32,
    bg: u32,
}

impl VesaLogger {
    pub fn new(fb_addr: u64, width: u32, height: u32, pitch: u32) -> Self {
        Self {
            fb: crate::memory::phys_to_virt(fb_addr) as *mut u32,
            width: width as usize,
            height: height as usize,
            pitch: pitch as usize,
            x: 0,
            y: 0,
            fg: 0x00FFFFFF, // белый
            bg: 0x00000000, // чёрный
        }
    }

    fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            unsafe {
                let offset = y * (self.pitch / 4) + x;
                *self.fb.add(offset) = color;
            }
        }
    }

    fn draw_char(&mut self, x: usize, y: usize, ch: u8) {
        if ch < 0x20 || ch > 0x7E { return; }
        let char_index = (ch as usize) - 0x20;
        let base = char_index * font::FONT_HEIGHT;

        for row in 0..font::FONT_HEIGHT {
            let byte = font::FONT[base + row];
            for col in 0..font::FONT_WIDTH {
                let pixel_x = x + col;
                let pixel_y = y + row;
                if byte & (1 << (7 - col)) != 0 {
                    self.put_pixel(pixel_x, pixel_y, self.fg);
                } else {
                    self.put_pixel(pixel_x, pixel_y, self.bg);
                }
            }
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.x = 0;
                self.y += font::FONT_HEIGHT;
            }
            b'\r' => {
                self.x = 0;
            }
            // Backspace erases the previous cell; the cursor moves back without wrapping
            b'\x08' => {
                if self.x >= font::FONT_WIDTH {
                    self.x -= font::FONT_WIDTH;
                    self.draw_char(self.x, self.y, b' ');
                }
                return;
            }
            _ => {
                self.draw_char(self.x, self.y, byte);
                self.x += font::FONT_WIDTH;
            }
        }

        // Перенос строки
        if self.x + font::FONT_WIDTH > self.width {
            self.x = 0;
            self.y += font::FONT_HEIGHT;
        }

        // Скроллинг
        if self.y + font::FONT_HEIGHT > self.height {
            self.scroll();
        }
    }

    pub fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.put_pixel(x, y, self.bg);
            }
        }
        self.x = 0;
        self.y = 0;
    }

    fn scroll(&mut self) {
        let line_height = font::FONT_HEIGHT;
        let line_bytes = self.pitch * line_height;

        unsafe {
            core::ptr::copy(
                self.fb.add(line_height * (self.pitch / 4)),
                            self.fb,
                            (self.height - line_height) * (self.pitch / 4),
            );

            // Очищаем последнюю строку
            let last_line = (self.height - line_height) * (self.pitch / 4);
            for i in 0..(self.width * line_height) {
                *self.fb.add(last_line + i) = self.bg;
            }
        }

        self.y -= line_height;
    }

    pub fn log(&mut self, level: &str, msg: &str) {
        let color = match level {
            "OK" => 0x0000FF00,    // зелёный
            "INFO" => 0x00FFFFFF,  // белый
            "WARN" => 0x00FFFF00,  // жёлтый
            "ERROR" => 0x00FF0000, // красный
            _ => 0x00FFFFFF,
        };

        self.fg = color;
        self.write_string("[");
        self.write_string(level);
        self.write_string("] ");
        self.fg = 0x00FFFFFF;
        self.write_string(msg);
        self.write_string("\n");
    }
}

// Global console used by the shell via the kernel API; the boot logger keeps its own cursor.
static mut CONSOLE: Option<VesaLogger> = None;

pub fn console_init(fb_addr: u64, width: u32, height: u32, pitch: u32) {
    unsafe {
        CONSOLE = Some(VesaLogger::new(fb_addr, width, height, pitch));
    }
}

pub fn console_reset() {
    unsafe {
        if let Some(ref mut console) = CONSOLE {
            console.clear();
        }
    }
}

pub extern "C" fn console_putc(byte: u8) {
    unsafe {
        if let Some(ref mut console) = CONSOLE {
            console.write_byte(byte);
        }
    }
}

pub extern "C" fn console_clear() {
    console_reset();
}
