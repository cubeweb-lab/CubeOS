// src/cube_core/elf.rs
use core::mem::size_of;
use crate::checks::{self, PAGE_SIZE};
use crate::vmm::{AddressSpace, USER_BASE, USER_LIMIT};

pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const PT_LOAD: u32 = 1;

// Флаги сегментов
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

// ELF64 заголовок
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C, packed)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

pub struct LoadedProgram {
    pub entry_point: u64,
    pub base_addr: u64,
    pub size: u64,
}

fn read_phdr(data: &[u8], header: &Elf64Header, idx: usize) -> Option<Elf64Phdr> {
    let offset = header.e_phoff.checked_add(idx as u64 * header.e_phentsize as u64)?;
    if !checks::range_fits(offset, size_of::<Elf64Phdr>() as u64, data.len() as u64) {
        return None;
    }
    // Program headers are not guaranteed to be aligned inside the file buffer.
    Some(unsafe { core::ptr::read_unaligned(data.as_ptr().add(offset as usize) as *const Elf64Phdr) })
}

/// Загружает ELF-файл в адресное пространство процесса
/// Возвращает точку входа и базовый адрес
pub fn load_elf(data: &[u8], space: &mut AddressSpace) -> Option<LoadedProgram> {
    if data.len() < size_of::<Elf64Header>() {
        return None;
    }

    let header = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Elf64Header) };

    if header.e_ident[0..4] != ELF_MAGIC {
        return None;
    }

    if (header.e_phentsize as usize) < size_of::<Elf64Phdr>() {
        crate::serial::send_string("[ELF] Program header size too small\n");
        return None;
    }

    let file_len = data.len() as u64;

    // Validate every segment and the entry point before writing any memory,
    // so a malformed file cannot leave the kernel half-overwritten.
    let mut base_addr = u64::MAX;
    let mut end_addr = 0u64;
    let mut entry_ok = false;

    for ph_idx in 0..header.e_phnum as usize {
        let phdr = read_phdr(data, &header, ph_idx)?;
        if phdr.p_type != PT_LOAD {
            continue;
        }

        if !checks::elf_segment_fits(
            phdr.p_vaddr,
            phdr.p_offset,
            phdr.p_filesz,
            phdr.p_memsz,
            file_len,
            USER_BASE,
            USER_LIMIT,
        ) {
            crate::serial::send_string("[ELF] Segment rejected: outside load region or malformed\n");
            return None;
        }

        if phdr.p_flags & PF_X != 0 && checks::in_segment(header.e_entry, phdr.p_vaddr, phdr.p_memsz) {
            entry_ok = true;
        }

        base_addr = base_addr.min(phdr.p_vaddr);
        end_addr = end_addr.max(phdr.p_vaddr + phdr.p_memsz);
    }

    if base_addr == u64::MAX {
        crate::serial::send_string("[ELF] No PT_LOAD segments found\n");
        return None;
    }

    if !entry_ok {
        crate::serial::send_string("[ELF] Entry point is not inside an executable segment\n");
        return None;
    }

    for ph_idx in 0..header.e_phnum as usize {
        let phdr = read_phdr(data, &header, ph_idx)?;
        if phdr.p_type != PT_LOAD {
            continue;
        }

        let (first, end) = checks::page_span(phdr.p_vaddr, phdr.p_memsz)?;
        let file_end = phdr.p_vaddr + phdr.p_filesz;
        let writable = phdr.p_flags & PF_W != 0;

        let mut page = first;
        while page < end {
            // Segments can share a page; reuse the frame an earlier segment already mapped.
            let frame = space.map_user_zeroed(page, writable)?;

            let copy_start = page.max(phdr.p_vaddr);
            let copy_end = (page + PAGE_SIZE).min(file_end);
            if copy_start < copy_end {
                let src = data.as_ptr().wrapping_add((phdr.p_offset + (copy_start - phdr.p_vaddr)) as usize);
                let dst = crate::memory::phys_to_virt(frame.start_address().as_u64() + (copy_start - page)) as *mut u8;
                unsafe { core::ptr::copy_nonoverlapping(src, dst, (copy_end - copy_start) as usize) };
            }
            page += PAGE_SIZE;
        }
    }

    crate::serial::send_string("[ELF] Loaded segments\n");

    Some(LoadedProgram {
        entry_point: header.e_entry,
        base_addr,
        size: end_addr - base_addr,
    })
}

