use core::mem::size_of;
use core::ptr;
use crate::checks;

#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub fb_type: u8,
    pub red_field_position: u8,
    pub red_mask_size: u8,
    pub green_field_position: u8,
    pub green_mask_size: u8,
    pub blue_field_position: u8,
    pub blue_mask_size: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryMapEntry {
    pub base_addr: u64,
    pub length: u64,
    pub typ: u32,
}

pub struct MultibootInfo {
    pub framebuffer: Option<FramebufferInfo>,
    pub memory_map: &'static [MemoryMapEntry],
    pub total_size: u32,
}

impl MultibootInfo {
    pub unsafe fn parse(info_addr: u32) -> Self {
        let total_size = ptr::read(info_addr as *const u32);
        let mut ptr_cur = (info_addr as *const u32).add(2);
        let end = info_addr.checked_add(total_size).unwrap_or(info_addr) as *const u32;

        let mut framebuffer = None;
        let mut mmap_ptr: *const MemoryMapEntry = core::ptr::null();
        let mut mmap_entries: usize = 0;

        while ptr_cur < end {
            let typ = ptr::read(ptr_cur);
            let size = ptr::read(ptr_cur.add(1)) as usize;

            if typ == 0 { break; }
            if size < 8 { break; }

            match typ {
                8 => {
                    // Framebuffer tag: type(4) + size(4) + framebuffer_data
                    let fb_ptr = ptr_cur.add(2) as *const u8;
                    let addr = ptr::read_unaligned(fb_ptr as *const u64);
                    let pitch = ptr::read_unaligned(fb_ptr.add(8) as *const u32);
                    let width = ptr::read_unaligned(fb_ptr.add(12) as *const u32);
                    let height = ptr::read_unaligned(fb_ptr.add(16) as *const u32);
                    let bpp = ptr::read_unaligned(fb_ptr.add(20) as *const u8);
                    let fb_type = ptr::read_unaligned(fb_ptr.add(21) as *const u8);
                    let (mut red_field_position, mut red_mask_size) = (0u8, 0u8);
                    let (mut green_field_position, mut green_mask_size) = (0u8, 0u8);
                    let (mut blue_field_position, mut blue_mask_size) = (0u8, 0u8);
                    if fb_type == 1 && size >= 30 {
                        red_field_position = ptr::read_unaligned(fb_ptr.add(24));
                        red_mask_size = ptr::read_unaligned(fb_ptr.add(25));
                        green_field_position = ptr::read_unaligned(fb_ptr.add(26));
                        green_mask_size = ptr::read_unaligned(fb_ptr.add(27));
                        blue_field_position = ptr::read_unaligned(fb_ptr.add(28));
                        blue_mask_size = ptr::read_unaligned(fb_ptr.add(29));
                    }

                    framebuffer = Some(FramebufferInfo {
                        addr, pitch, width, height, bpp, fb_type,
                        red_field_position, red_mask_size,
                        green_field_position, green_mask_size,
                        blue_field_position, blue_mask_size,
                    });
                }
                6 => {
                    // Memory map tag: type(4) + size(4) + entry_size(4) + entry_version(4) + entries
                    if size >= 16 {
                        let entry_size = ptr::read(ptr_cur.add(2)) as usize;
                        let count = checks::mmap_entry_count(size, 16, entry_size, size_of::<MemoryMapEntry>());
                        if count > 0 {
                            mmap_ptr = (ptr_cur as *const u8).add(16) as *const MemoryMapEntry;
                            mmap_entries = count;
                        }
                    }
                }
                _ => {}
            }

            let aligned_size = (size + 7) & !7;
            ptr_cur = (ptr_cur as *const u8).add(aligned_size) as *const u32;
        }

        let memory_map = if mmap_ptr.is_null() || mmap_entries == 0 {
            &[]
        } else {
            core::slice::from_raw_parts(mmap_ptr, mmap_entries)
        };

        Self {
            framebuffer,
            memory_map,
            total_size,
        }
    }

    pub fn available_memory(&self) -> u64 {
        self.memory_map
        .iter()
        .filter(|e| e.typ == 1)
        .map(|e| e.length)
        .sum()
    }

    pub fn total_memory(&self) -> u64 {
        self.memory_map.iter().map(|e| e.length).sum()
    }
}
