// src/cube_core/ext4fs.rs
// ext4 backend built on the `ext4_rs` crate. The crate works through a
// block-device trait that reads and writes 4 KiB blocks, so this module adapts
// ATA sectors and normalizes the path-based API used by the syscall layer.
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::str;
use ext4_rs::{BlockDevice, Ext4, InodeFileType, BLOCK_SIZE};

use crate::ata;
use crate::sync::SpinLock;
use crate::zigfs::{DirItem, Resolved, Stat};

const ROOT_INODE: u32 = 2;
const MAX_PATH: usize = 255;

static mut FS: Option<Ext4> = None;
static LOCK: SpinLock = SpinLock::new();

struct AtaBlockDevice {
    base_lba: u32,
}

impl AtaBlockDevice {
    fn read_sector(&self, lba: u32, buf: &mut [u8; 512]) -> bool {
        ata::read_sector_raw(lba, buf)
    }

    fn write_sector(&self, lba: u32, buf: &[u8; 512]) -> bool {
        ata::write_sector_raw(lba, buf)
    }
}

impl BlockDevice for AtaBlockDevice {
    fn read_offset(&self, offset: usize) -> Vec<u8> {
        let mut out = vec![0u8; BLOCK_SIZE];
        let first = offset / 512;
        let skip = offset % 512;
        let sectors = (skip + BLOCK_SIZE + 511) / 512;
        for i in 0..sectors {
            let mut sector = [0u8; 512];
            if !self.read_sector(self.base_lba + (first + i) as u32, &mut sector) {
                break;
            }
            let dst = i * 512;
            let src = if i == 0 { skip } else { 0 };
            let len = (512 - src).min(BLOCK_SIZE - dst);
            out[dst..dst + len].copy_from_slice(&sector[src..src + len]);
        }
        out
    }

    fn write_offset(&self, offset: usize, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let first = offset / 512;
        let skip = offset % 512;
        if skip == 0 && data.len() % 512 == 0 {
            for (i, chunk) in data.chunks(512).enumerate() {
                let mut sector = [0u8; 512];
                sector.copy_from_slice(chunk);
                if !self.write_sector(self.base_lba + (first + i) as u32, &sector) {
                    break;
                }
            }
            return;
        }

        let mut done = 0usize;
        let mut sector_index = first;
        let mut offset_in_sector = skip;
        while done < data.len() {
            let mut sector = [0u8; 512];
            if !self.read_sector(self.base_lba + sector_index as u32, &mut sector) {
                break;
            }
            let len = (512 - offset_in_sector).min(data.len() - done);
            sector[offset_in_sector..offset_in_sector + len]
                .copy_from_slice(&data[done..done + len]);
            if !self.write_sector(self.base_lba + sector_index as u32, &sector) {
                break;
            }
            done += len;
            sector_index += 1;
            offset_in_sector = 0;
        }
    }
}

fn with_fs<R>(f: impl FnOnce(&Ext4) -> R) -> Option<R> {
    LOCK.lock();
    let result = unsafe { (*core::ptr::addr_of!(FS)).as_ref().map(f) };
    LOCK.unlock();
    result
}

pub fn mount(base_lba: u32) -> bool {
    let fs = Ext4::open(Arc::new(AtaBlockDevice { base_lba }));
    LOCK.lock();
    unsafe { FS = Some(fs) };
    LOCK.unlock();
    true
}

pub fn is_mounted() -> bool {
    LOCK.lock();
    let mounted = unsafe { (*core::ptr::addr_of!(FS)).is_some() };
    LOCK.unlock();
    mounted
}

fn push_component(out: &mut [u8; MAX_PATH + 1], len: &mut usize, part: &[u8]) -> bool {
    if part.is_empty() {
        return true;
    }
    let extra = if *len == 0 { part.len() } else { part.len() + 1 };
    if *len + extra > MAX_PATH {
        return false;
    }
    if *len != 0 {
        out[*len] = b'/';
        *len += 1;
    }
    out[*len..*len + part.len()].copy_from_slice(part);
    *len += part.len();
    true
}

fn pop_component(out: &mut [u8; MAX_PATH + 1], len: &mut usize) {
    while *len > 0 {
        *len -= 1;
        if out[*len] == b'/' {
            return;
        }
    }
}

/// Joins `input` with `cwd` into a root-relative path (no leading slash).
pub fn normalize(cwd: &[u8], input: &[u8]) -> Option<([u8; MAX_PATH + 1], usize)> {
    let mut out = [0u8; MAX_PATH + 1];
    let mut len = 0usize;

    if !input.starts_with(b"/") {
        for part in cwd.split(|&b| b == b'/') {
            if part.is_empty() || part == b"." {
                continue;
            }
            if part == b".." {
                pop_component(&mut out, &mut len);
            } else if !push_component(&mut out, &mut len, part) {
                return None;
            }
        }
    }

    for part in input.split(|&b| b == b'/') {
        if part.is_empty() || part == b"." {
            continue;
        }
        if part == b".." {
            pop_component(&mut out, &mut len);
        } else if !push_component(&mut out, &mut len, part) {
            return None;
        }
    }

    Some((out, len))
}

fn open_inode(ext4: &Ext4, path: &str) -> Option<u32> {
    if path.is_empty() {
        return Some(ROOT_INODE);
    }
    let mut parent = ROOT_INODE;
    let mut name_off = 0;
    ext4.generic_open(path, &mut parent, false, 0, &mut name_off)
        .ok()
}

fn split_parent(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(index) => (&path[..index], &path[index + 1..]),
        None => ("", path),
    }
}

fn is_empty_dir(ext4: &Ext4, inode: u32) -> bool {
    for entry in ext4.dir_get_entries(inode) {
        let len = entry.name_len as usize;
        if len == 0 {
            continue;
        }
        let name = &entry.name[..len];
        if name == b"." || name == b".." {
            continue;
        }
        return false;
    }
    true
}

fn stat_locked(ext4: &Ext4, path: &str) -> Option<Stat> {
    let inode = open_inode(ext4, path)?;
    let inode_ref = ext4.get_inode_ref(inode);
    Some(Stat {
        is_dir: inode_ref.inode.is_dir() as u32,
        size: inode_ref.inode.size().min(u32::MAX as u64) as u32,
        cluster: inode,
    })
}

pub fn resolve(cwd: &[u8], input: &[u8]) -> Option<Resolved> {
    let (path, len) = normalize(cwd, input)?;
    let text = str::from_utf8(&path[..len]).ok()?;
    let stat = with_fs(|ext4| stat_locked(ext4, text)).flatten()?;
    Some(Resolved {
        cluster: stat.cluster,
        is_dir: stat.is_dir,
        path,
        path_len: len as u32,
    })
}

pub fn stat(path: &[u8]) -> Option<Stat> {
    let (buf, len) = normalize(b"/", path)?;
    let text = str::from_utf8(&buf[..len]).ok()?;
    with_fs(|ext4| stat_locked(ext4, text)).flatten()
}

pub fn read(path: &[u8], offset: u32, out: &mut [u8]) -> Option<usize> {
    let (buf, len) = normalize(b"/", path)?;
    let text = str::from_utf8(&buf[..len]).ok()?;
    with_fs(|ext4| {
        let inode = open_inode(ext4, text)?;
        ext4.read_at(inode, offset as usize, out).ok()
    })
    .flatten()
}

pub fn list(path: &[u8], items: &mut [DirItem]) -> Option<usize> {
    let (buf, len) = normalize(b"/", path)?;
    let text = str::from_utf8(&buf[..len]).ok()?;
    with_fs(|ext4| {
        let inode = open_inode(ext4, text)?;
        let entries = ext4.dir_get_entries(inode);
        let mut count = 0usize;
        for entry in entries {
            let name_len = entry.name_len as usize;
            if name_len == 0 {
                continue;
            }
            let name = &entry.name[..name_len];
            if name == b"." || name == b".." {
                continue;
            }
            if count >= items.len() {
                break;
            }
            let is_dir = entry.get_de_type() == 2;
            let size = ext4
                .get_inode_ref(entry.inode)
                .inode
                .size()
                .min(u32::MAX as u64) as u32;
            let mut item = DirItem {
                name: [0; 256],
                name_len: name_len as u32,
                attr: if is_dir { 0x10 } else { 0 },
                size,
                cluster: entry.inode,
            };
            item.name[..name_len].copy_from_slice(name);
            items[count] = item;
            count += 1;
        }
        Some(count)
    })
    .flatten()
}

pub fn write_file(path: &[u8], data: &[u8]) -> bool {
    let Some((buf, len)) = normalize(b"/", path) else {
        return false;
    };
    let Ok(text) = str::from_utf8(&buf[..len]) else {
        return false;
    };
    with_fs(|ext4| {
        let mut parent = ROOT_INODE;
        let mut name_off = 0;
        let inode = ext4
            .generic_open(
                text,
                &mut parent,
                true,
                InodeFileType::S_IFREG.bits(),
                &mut name_off,
            )
            .ok()?;
        let mut inode_ref = ext4.get_inode_ref(inode);
        if inode_ref.inode.size() != 0 {
            ext4.truncate_inode(&mut inode_ref, 0).ok()?;
        }
        ext4.write_at(inode, 0, data).ok()?;
        Some(())
    })
    .flatten()
    .is_some()
}

pub fn mkdir(path: &[u8]) -> bool {
    let Some((buf, len)) = normalize(b"/", path) else {
        return false;
    };
    let Ok(text) = str::from_utf8(&buf[..len]) else {
        return false;
    };
    with_fs(|ext4| ext4.dir_mk(text).is_ok()).unwrap_or(false)
}

pub fn unlink(path: &[u8]) -> bool {
    let Some((buf, len)) = normalize(b"/", path) else {
        return false;
    };
    let Ok(text) = str::from_utf8(&buf[..len]) else {
        return false;
    };
    with_fs(|ext4| {
        let child = open_inode(ext4, text)?;
        let child_ref = ext4.get_inode_ref(child);
        if child_ref.inode.is_dir() {
            return None;
        }
        let (parent_path, name) = split_parent(text);
        let parent = open_inode(ext4, parent_path)?;
        let mut parent_ref = ext4.get_inode_ref(parent);
        let mut child_ref = child_ref;

        if child_ref.inode.links_count() <= 1 {
            if child_ref.inode.size() != 0 {
                ext4.truncate_inode(&mut child_ref, 0).ok()?;
            }
            child_ref.inode.set_links_count(0);
            child_ref
                .inode
                .set_dtime((crate::uptime::seconds() + 1_700_000_000) as u32);
            ext4.write_back_inode(&mut child_ref);
            ext4.dir_remove_entry(&mut parent_ref, name).ok()?;
            ext4.ialloc_free_inode(child, false);
        } else {
            child_ref
                .inode
                .set_links_count(child_ref.inode.links_count() - 1);
            ext4.write_back_inode(&mut child_ref);
            ext4.dir_remove_entry(&mut parent_ref, name).ok()?;
        }
        ext4.write_back_inode(&mut parent_ref);
        Some(())
    })
    .flatten()
    .is_some()
}

pub fn rmdir(path: &[u8]) -> bool {
    let Some((buf, len)) = normalize(b"/", path) else {
        return false;
    };
    let Ok(text) = str::from_utf8(&buf[..len]) else {
        return false;
    };
    with_fs(|ext4| {
        let child = open_inode(ext4, text)?;
        let mut child_ref = ext4.get_inode_ref(child);
        if !child_ref.inode.is_dir() || !is_empty_dir(ext4, child) {
            return None;
        }
        let (parent_path, name) = split_parent(text);
        let parent = open_inode(ext4, parent_path)?;
        let mut parent_ref = ext4.get_inode_ref(parent);

        ext4.truncate_inode(&mut child_ref, 0).ok()?;
        child_ref.inode.set_links_count(0);
        child_ref
            .inode
            .set_dtime((crate::uptime::seconds() + 1_700_000_000) as u32);
        ext4.write_back_inode(&mut child_ref);
        ext4.dir_remove_entry(&mut parent_ref, name).ok()?;
        parent_ref
            .inode
            .set_links_count(parent_ref.inode.links_count().saturating_sub(1));
        ext4.write_back_inode(&mut parent_ref);
        ext4.ialloc_free_inode(child, true);
        Some(())
    })
    .flatten()
    .is_some()
}

pub fn rename(old: &[u8], new: &[u8]) -> bool {
    let Some(stat) = stat(old) else {
        return false;
    };
    if stat.is_dir != 0 {
        return false;
    }
    let mut data = vec![0u8; stat.size as usize];
    let read_len = read(old, 0, &mut data).unwrap_or(0);
    write_file(new, &data[..read_len]) && unlink(old)
}

/// Boot-time smoke test for the ext4 backend, enabled by `kernel-debug`.
pub fn self_test() -> bool {
    const PATH: &[u8] = b"/tmp/ext4-selftest";
    const MOVED: &[u8] = b"/tmp/ext4-dir/moved";
    const DATA: &[u8] = b"cubeos ext4 self-test\n";

    let mut config = [0u8; 4096];
    let config_ok = read(b"/system/config/boot.cfg", 0, &mut config)
        .map(|n| n > 0)
        .unwrap_or(false);

    let mut items = [crate::zigfs::EMPTY_ITEM; 8];
    let list_ok = list(b"/system/info", &mut items).map(|n| n > 0).unwrap_or(false);

    let written = write_file(PATH, DATA);
    let mut buf = [0u8; 64];
    let read_ok = read(PATH, 0, &mut buf) == Some(DATA.len()) && buf[..DATA.len()] == *DATA;
    let mkdir_ok = mkdir(b"/tmp/ext4-dir");
    let rename_ok = rename(PATH, MOVED);
    let mut moved_buf = [0u8; 64];
    let moved_ok =
        read(MOVED, 0, &mut moved_buf) == Some(DATA.len()) && moved_buf[..DATA.len()] == *DATA;
    let unlink_ok = unlink(MOVED);
    let rmdir_ok = rmdir(b"/tmp/ext4-dir");

    let ok = config_ok
        && list_ok
        && written
        && read_ok
        && mkdir_ok
        && rename_ok
        && moved_ok
        && unlink_ok
        && rmdir_ok;
    if !config_ok {
        crate::serial::send_string("[TEST] config read failed\n");
    }
    if !list_ok {
        crate::serial::send_string("[TEST] list failed\n");
    }
    if !written {
        crate::serial::send_string("[TEST] write failed\n");
    }
    if !read_ok {
        crate::serial::send_string("[TEST] read failed\n");
    }
    if !mkdir_ok {
        crate::serial::send_string("[TEST] mkdir failed\n");
    }
    if !rename_ok {
        crate::serial::send_string("[TEST] rename failed\n");
    }
    if !moved_ok {
        crate::serial::send_string("[TEST] moved read failed\n");
    }
    if !unlink_ok {
        crate::serial::send_string("[TEST] unlink failed\n");
    }
    if !rmdir_ok {
        crate::serial::send_string("[TEST] rmdir failed\n");
    }
    crate::serial::send_string(if ok {
        "[  OK ] ext4 self-test: read, list, write, rename and delete passed\n"
    } else {
        "[ERROR] ext4 self-test failed\n"
    });
    ok
}
