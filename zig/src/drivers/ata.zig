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

// CubeOS shim for the NovumOS FAT driver: sector I/O goes through the kernel's ATA code.

pub const Drive = enum(u1) {
    Master = 0,
    Slave = 1,
};

const builtin = @import("builtin");

extern fn cube_ata_read_sector(lba: u32, buf: [*]u8) bool;
extern fn cube_ata_write_sector(lba: u32, buf: [*]const u8) bool;

/// First LBA of the partition the FAT driver addresses, set by the kernel
/// after it locates the partition in the MBR.
pub var partition_lba: u32 = 0;

/// Sector image used by host tests in place of the disk.
pub var test_image: []u8 = &.{};

fn testSector(lba: u32) ?*[512]u8 {
    const offset = (@as(usize, partition_lba) + lba) * 512;
    if (offset + 512 > test_image.len) return null;
    return test_image[offset..][0..512];
}

// The kernel's ATA driver only addresses the primary master, so the slave
// is reported as an unreadable (all-zero) device instead of aliasing master.
pub fn read_sector(drive: Drive, lba: u32, buffer: [*]u8) void {
    if (drive != .Master) {
        @memset(buffer[0..512], 0);
        return;
    }
    if (builtin.is_test) {
        if (testSector(lba)) |sector| {
            @memcpy(buffer[0..512], sector);
        } else {
            @memset(buffer[0..512], 0);
        }
        return;
    }
    if (!cube_ata_read_sector(partition_lba +% lba, buffer)) {
        @memset(buffer[0..512], 0);
    }
}

pub fn write_sector(drive: Drive, lba: u32, data: [*]const u8) void {
    if (drive != .Master) return;
    if (builtin.is_test) {
        if (testSector(lba)) |sector| @memcpy(sector, data[0..512]);
        return;
    }
    _ = cube_ata_write_sector(partition_lba +% lba, data);
}
