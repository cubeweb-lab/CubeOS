#![no_std]
#![no_main]

use cube_syscall as sys;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys::exit(1)
}

// --- числа (как в исходнике) -------------------------------------------

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

// --- клавиатура: WASD ----------------------------------------------------

const KEY_W: u8 = b'W';
const KEY_A: u8 = b'A';
const KEY_S: u8 = b'S';
const KEY_D: u8 = b'D';
const KEY_Q: u8 = b'Q';

/// Состояние клавиши сейчас (удержание W/A/S/D).
fn key_state(key: u8) -> bool {
    // SYSCALL: в списке (SYS_READ..SYS_FB_MAP) ввода клавиш нет.
    // Когда в ядре появится sys_get_last_key() -> u8 — вернуть true,
    // если key сейчас удерживается.
    let _ = key;
    false
}

/// Разовое нажатие (для Q — выход, W после аварии — рестарт).
fn key_pressed(key: u8) -> bool {
    // SYSCALL: sys_get_last_key_press() -> u8; запоминай предыдущее
    // значение и возвращай true при фронте 0->1.
    let _ = key;
    false
}

// --- игра ----------------------------------------------------------------

const ROAD_W:  u32 = 320;
const CAR_W:   u32 = 40;
const CAR_H:   u32 = 60;
const TRACK_W: u32 = 160;
const OBST_H:  u32 = 30;

struct Game {
    car_x: i32,
    speed: i32,
    score: u32,
    over:  bool,
    obs_x: i32,
    obs_y: i32,
    rng:   u32,
}

impl Game {
    fn new() -> Self {
        Game { car_x: 0, speed: 4, score: 0, over: false,
               obs_x: 0, obs_y: -80, rng: 0x1234_5678 }
    }

    fn rand(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng
    }

    fn step(&mut self, left: bool, right: bool, up: bool, down: bool) {
        if self.over {
            if up { *self = Game::new(); }
            return;
        }
        if left  { self.car_x -= 6; }
        if right { self.car_x += 6; }
        if up    { self.speed = (self.speed + 1).min(12); }
        if down  { self.speed = (self.speed - 1).max(2); }

        self.obs_y += self.speed;
        if self.obs_y > 480 {
            self.obs_y = -80;
            self.obs_x = (self.rand() % ROAD_W as u32) as i32 - ROAD_W as i32 / 2;
            self.score += 1;
        }

        let car_left  = self.car_x - CAR_W as i32 / 2;
        let car_right = self.car_x + CAR_W as i32 / 2;
        let car_top   = 400;
        let car_bot   = car_top + CAR_H as i32;
        if self.obs_y + OBST_H as i32 > car_top
            && self.obs_y < car_bot
            && self.obs_x + TRACK_W as i32 / 2 > car_left
            && self.obs_x - TRACK_W as i32 / 2 < car_right
        {
            self.over = true;
        }
    }
}

// --- отрисовка -----------------------------------------------------------

fn draw(fb: &mut sys::Framebuffer, g: &Game, w: u32, h: u32) {
    let sky   = fb.rgb(0x1E, 0x3A, 0x8A);
    let road  = fb.rgb(0x22, 0x22, 0x22);
    let grass = fb.rgb(0x1E, 0x7A, 0x1E);
    let car_c = fb.rgb(0xE0, 0x30, 0x20);
    let obs_c = fb.rgb(0x88, 0x88, 0x88);

    fb.fill(grass);
    for y in 0..h {
        for x in 0..w {
            if x >= w / 2 - ROAD_W / 2 && x < w / 2 + ROAD_W / 2 {
                fb.put_pixel(x, y, road);
            } else if y < h / 3 {
                fb.put_pixel(x, y, sky);
            }
        }
    }

    let cx = (w as i32 / 2 + g.car_x).max(0) as u32;
    let cy = 400u32;
    for dy in 0..CAR_H {
        for dx in 0..CAR_W {
            let px = cx.wrapping_add(dx).wrapping_sub(CAR_W / 2);
            let py = cy.wrapping_add(dy);
            if px < w && py < h { fb.put_pixel(px, py, car_c); }
        }
    }

    if g.obs_y >= 0 {
        let ox = (w as i32 / 2 + g.obs_x).max(0) as u32;
        let oy = g.obs_y as u32;
        for dy in 0..OBST_H {
            for dx in 0..TRACK_W {
                let px = ox.wrapping_add(dx).wrapping_sub(TRACK_W / 2);
                let py = oy.wrapping_add(dy);
                if px < w && py < h { fb.put_pixel(px, py, obs_c); }
            }
        }
    }

    let mut buf = [0u8; 20];
    if g.over {
        // только ASCII в байтовых литералах — «—» заменён на «-»
        sys::write(sys::STDOUT, b"[RACE] GAME OVER score=");
        sys::write(sys::STDOUT, put_num(&mut buf, g.score as usize).as_bytes());
        sys::write(sys::STDOUT, b"  W - restart, Q - quit\n");
    } else {
        sys::write(sys::STDOUT, b"[RACE] score=");
        sys::write(sys::STDOUT, put_num(&mut buf, g.score as usize).as_bytes());
        sys::write(sys::STDOUT, b"\n");
    }
}

fn read_keys() -> (bool, bool, bool, bool, bool) {
    (key_state(KEY_A), key_state(KEY_D), key_state(KEY_W), key_state(KEY_S), key_pressed(KEY_Q))
}

// --- main -----------------------------------------------------------------

#[no_mangle]
pub extern "C" fn _start() -> ! {
    sys::write(sys::STDOUT, b"[RACE] start\n");

    let mut fb = match sys::Framebuffer::new() {
        Some(fb) => fb,
        None => { sys::write(sys::STDOUT, b"[RACE] no framebuffer\n"); sys::exit(1); }
    };
    let info = fb.info();
    let w = info.width;
    let h = info.height;

    let mut buf = [0u8; 20];
    sys::write(sys::STDOUT, b"[RACE] mode ");
    sys::write(sys::STDOUT, put_num(&mut buf, w as usize).as_bytes());
    sys::write(sys::STDOUT, b"x");
    sys::write(sys::STDOUT, put_num(&mut buf, h as usize).as_bytes());
    sys::write(sys::STDOUT, b"\n");

    let mut game = Game::new();

    loop {
        let (left, right, up, down, quit) = read_keys();
        if quit { sys::exit(0); }

        game.step(left, right, up, down);
        draw(&mut fb, &game, w, h);
        // fb.flush();        // SYSCALL: если put_pixel пишет в back-buffer
        // sys::sleep_ms(16); // SYSCALL: задержка для ~60 FPS
    }
}