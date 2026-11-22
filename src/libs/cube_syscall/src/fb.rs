use crate::consts::{SYS_FB_INFO, SYS_FB_MAP};
use crate::syscall::{syscall0, syscall2};

/// ABI-compatible framebuffer description returned by the kernel.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct FbInfo {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub fb_type: u32,
    pub red_field_position: u32,
    pub red_mask_size: u32,
    pub green_field_position: u32,
    pub green_mask_size: u32,
    pub blue_field_position: u32,
    pub blue_mask_size: u32,
}

/// Returns the framebuffer geometry and pixel format.
pub fn info() -> Option<FbInfo> {
    let mut info = FbInfo::default();
    let result = syscall2(
        SYS_FB_INFO,
        &mut info as *mut FbInfo as usize,
        core::mem::size_of::<FbInfo>(),
    );
    if result == 0 { Some(info) } else { None }
}

/// Maps the linear framebuffer into this process and returns its user address.
pub fn map() -> Option<usize> {
    let addr = syscall0(SYS_FB_MAP);
    if addr == usize::MAX { None } else { Some(addr) }
}

/// A 32-bit linear framebuffer mapped into user space.
pub struct Framebuffer {
    base: *mut u32,
    info: FbInfo,
}

impl Framebuffer {
    pub fn new() -> Option<Self> {
        let info = info()?;
        if info.bpp != 32
            || info.fb_type != 1
            || info.pitch % 4 != 0
            || info.width == 0
            || info.height == 0
            || info.pitch < info.width.checked_mul(4)?
        {
            return None;
        }
        let addr = map()?;
        Some(Self { base: addr as *mut u32, info })
    }

    pub fn info(&self) -> FbInfo {
        self.info
    }

    pub fn width(&self) -> u32 {
        self.info.width
    }

    pub fn height(&self) -> u32 {
        self.info.height
    }

    pub fn pitch(&self) -> u32 {
        self.info.pitch
    }

    pub fn put_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.info.width && y < self.info.height {
            let offset = y * (self.info.pitch / 4) + x;
            unsafe { *self.base.add(offset as usize) = color; }
        }
    }

    pub fn fill(&mut self, color: u32) {
        for y in 0..self.info.height {
            for x in 0..self.info.width {
                self.put_pixel(x, y, color);
            }
        }
    }

    pub fn clear(&mut self) {
        self.fill(0);
    }

    /// Packs an RGB color using the format reported by the firmware.
    pub fn rgb(&self, r: u8, g: u8, b: u8) -> u32 {
        field(r, self.info.red_mask_size, self.info.red_field_position)
            | field(g, self.info.green_mask_size, self.info.green_field_position)
            | field(b, self.info.blue_mask_size, self.info.blue_field_position)
    }
}

fn field(value: u8, size: u32, position: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    let bits = size.min(8);
    let max = (1u32 << bits) - 1;
    (((value as u32) * max + 127) / 255) << position
}
