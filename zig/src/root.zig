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
const fat = @import("drivers/fat.zig");
const ata = @import("drivers/ata.zig");

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

pub export fn zig_fat_file_size(path: [*]const u8, path_len: usize) u32 {
    const bpb = mounted orelse return 0;
    return fat.fat_size(.Master, bpb, 0, path[0..path_len]);
}

pub export fn zig_fat_read_file(path: [*]const u8, path_len: usize, out: [*]u8, max_len: u32) i32 {
    const bpb = mounted orelse return -1;
    return fat.read_file_bounded(.Master, bpb, 0, path[0..path_len], out, max_len);
}

pub export fn zig_fat_list_dir(path: [*]const u8, path_len: usize) bool {
    const bpb = mounted orelse return false;
    const res = fat.resolve_full_path(.Master, bpb, 0, "/", path[0..path_len]) orelse return false;
    if (!res.is_dir) return false;
    fat.list_directory(.Master, bpb, res.cluster, false);
    return true;
}
