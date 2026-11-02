// Copyright 2025-2026 MinecAnton209
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://apache.org
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! CubeOS Zig side: the NovumOS FAT12/16/32 driver exposed to the Rust kernel through a C ABI.
//! Paths are relative to `start_dir` (a cluster number, 0 = root) unless they start with '/'.
const fat = @import("drivers/fat.zig");
const ata = @import("drivers/ata.zig");

pub const Stat = extern struct {
    is_dir: u32,
    size: u32,
    cluster: u32,
};

pub const Resolved = extern struct {
    cluster: u32,
    is_dir: u32,
    path: [256]u8,
    path_len: u32,
};

var mounted: ?fat.BPB = null;

pub export fn zig_fat_mount(partition_lba: u32) bool {
    ata.partition_lba = partition_lba;
    mounted = fat.read_bpb(.Master);
    return mounted != null;
}

pub export fn zig_fat_type() u32 {
    const bpb = mounted orelse return 0;
    return @intFromEnum(bpb.fat_type);
}

fn statPath(bpb: fat.BPB, start_dir: u32, path: []const u8) ?Stat {
    const loc = fat.resolve_path(.Master, bpb, start_dir, path) orelse return null;
    const entry = fat.find_entry_literal(.Master, bpb, loc.dir_cluster, loc.file_name) orelse return null;
    return .{
        .is_dir = @as(u32, @intFromBool((entry.attr & 0x10) != 0)),
        .size = entry.file_size,
        .cluster = @as(u32, entry.first_cluster_low) | (@as(u32, entry.first_cluster_high) << 16),
    };
}

pub export fn zig_fat_stat(start_dir: u32, path: [*]const u8, path_len: usize, out: *Stat) bool {
    const bpb = mounted orelse return false;
    out.* = statPath(bpb, start_dir, path[0..path_len]) orelse return false;
    return true;
}

/// Normalizes `input` against the working directory `cwd` (starting at `start_dir`) into an absolute path.
pub export fn zig_fat_resolve(
    start_dir: u32,
    cwd: [*]const u8,
    cwd_len: usize,
    input: [*]const u8,
    input_len: usize,
    out: *Resolved,
) bool {
    const bpb = mounted orelse return false;
    const res = fat.resolve_full_path(.Master, bpb, start_dir, cwd[0..cwd_len], input[0..input_len]) orelse return false;
    out.cluster = res.cluster;
    out.is_dir = @as(u32, @intFromBool(res.is_dir));
    out.path_len = @intCast(res.path_len);
    @memcpy(out.path[0..res.path_len], res.path[0..res.path_len]);
    return true;
}

pub export fn zig_fat_read(
    start_dir: u32,
    path: [*]const u8,
    path_len: usize,
    offset: u32,
    out: [*]u8,
    max_len: u32,
) i32 {
    const bpb = mounted orelse return -1;
    var handle = fat.fat_open(.Master, bpb, start_dir, path[0..path_len]) orelse return -1;
    _ = fat.fat_lseek(&handle, offset, 0);
    return fat.fat_read(&handle, out, max_len);
}

/// Writes the directory's entries into `items`. Returns the entry count, or -1 if `path` is not a directory.
pub export fn zig_fat_list(
    start_dir: u32,
    path: [*]const u8,
    path_len: usize,
    items: [*]fat.ListItem,
    max_items: u32,
) i32 {
    const bpb = mounted orelse return -1;
    const st = statPath(bpb, start_dir, path[0..path_len]) orelse return -1;
    if (st.is_dir == 0) return -1;
    const n = fat.collect_entries(.Master, bpb, st.cluster, items[0..@as(usize, max_items)]);
    return @intCast(n);
}

pub export fn zig_fat_write_file(
    start_dir: u32,
    path: [*]const u8,
    path_len: usize,
    data: [*]const u8,
    data_len: usize,
) bool {
    const bpb = mounted orelse return false;
    return fat.write_file(.Master, bpb, start_dir, path[0..path_len], data[0..data_len]);
}

pub export fn zig_fat_mkdir(start_dir: u32, path: [*]const u8, path_len: usize) bool {
    const bpb = mounted orelse return false;
    return fat.create_directory(.Master, bpb, start_dir, path[0..path_len]);
}

pub export fn zig_fat_unlink(start_dir: u32, path: [*]const u8, path_len: usize) bool {
    const bpb = mounted orelse return false;
    return fat.delete_file(.Master, bpb, start_dir, path[0..path_len]);
}

pub export fn zig_fat_rmdir(start_dir: u32, path: [*]const u8, path_len: usize, recursive: bool) bool {
    const bpb = mounted orelse return false;
    return fat.delete_directory(.Master, bpb, start_dir, path[0..path_len], recursive);
}

pub export fn zig_fat_rename(start_dir: u32, old: [*]const u8, old_len: usize, new: [*]const u8, new_len: usize) bool {
    const bpb = mounted orelse return false;
    return fat.rename_file(.Master, bpb, start_dir, old[0..old_len], new[0..new_len]);
}
