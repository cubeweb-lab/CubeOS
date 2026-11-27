use crate::multiboot::FramebufferInfo;
use x86_64::registers::control::Cr3;

/// Fixed user virtual address where the linear framebuffer is mapped on demand.
pub const USER_FB_BASE: u64 = 0x0000_6000_0000_0000;

/// ABI-compatible framebuffer description shared with `cube_syscall`.
#[repr(C)]
#[derive(Copy, Clone)]
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

#[derive(Copy, Clone)]
struct State {
    info: FbInfo,
    phys: u64,
    size: u64,
}

static mut STATE: Option<State> = None;

pub fn init(fb: FramebufferInfo) {
    let info = FbInfo {
        width: fb.width,
        height: fb.height,
        pitch: fb.pitch,
        bpp: fb.bpp as u32,
        fb_type: fb.fb_type as u32,
        red_field_position: fb.red_field_position as u32,
        red_mask_size: fb.red_mask_size as u32,
        green_field_position: fb.green_field_position as u32,
        green_mask_size: fb.green_mask_size as u32,
        blue_field_position: fb.blue_field_position as u32,
        blue_mask_size: fb.blue_mask_size as u32,
    };
    unsafe {
        STATE = Some(State {
            info,
            phys: fb.addr,
            size: fb.pitch as u64 * fb.height as u64,
        });
    }
}

pub fn info() -> Option<FbInfo> {
    unsafe { STATE.map(|s| s.info) }
}

/// Maps the framebuffer into the current user address space and returns its VA.
pub fn map_current() -> Option<u64> {
    let state = unsafe { STATE }?;
    if state.phys == 0 || state.size == 0 {
        return None;
    }
    let pages = ((state.size + 4095) / 4096) as usize;
    let (cr3, _) = Cr3::read();
    if crate::vmm::map_user_phys_range(
        cr3.start_address().as_u64(),
        USER_FB_BASE,
        state.phys,
        pages,
    ) {
        Some(USER_FB_BASE)
    } else {
        None
    }
}

/// Whether `phys` belongs to the framebuffer, whose frames are borrowed from firmware.
pub fn is_fb_frame(phys: u64) -> bool {
    match unsafe { STATE } {
        Some(state) => phys >= state.phys && phys < state.phys.saturating_add(state.size),
        None => false,
    }
}
