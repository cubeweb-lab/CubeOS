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

const ata = @import("../ata.zig");

pub const FatCacheSize = 64;

var fat_cache_sectors: [FatCacheSize][512]u8 = undefined;
var fat_cache_sector: [FatCacheSize]u32 = undefined;
var fat_cache_count: usize = 0;
var fat_cache_drive: ata.Drive = undefined;
var fat_cache_init: bool = false;

fn fat_cache_init_internal(drive: ata.Drive) void {
    fat_cache_drive = drive;
    fat_cache_count = 0;
    fat_cache_init = true;
}

/// Invalidate the FAT sector cache entirely (call after mkfs/reformat).
pub fn invalidate_fat_cache() void {
    fat_cache_count = 0;
    fat_cache_init = false;
}

pub fn fat_read_cached_sector(drive: ata.Drive, sector: u32) [*]u8 {
    if (!fat_cache_init or fat_cache_drive != drive) {
        fat_cache_init_internal(drive);
    }

    for (0..fat_cache_count) |i| {
        if (fat_cache_sector[i] == sector) {
            return &fat_cache_sectors[i];
        }
    }

    if (fat_cache_count < FatCacheSize) {
        ata.read_sector(drive, sector, &fat_cache_sectors[fat_cache_count]);
        fat_cache_sector[fat_cache_count] = sector;
        fat_cache_count += 1;
        return &fat_cache_sectors[fat_cache_count - 1];
    }

    const oldest = 0;
    ata.read_sector(drive, sector, &fat_cache_sectors[oldest]);
    fat_cache_sector[oldest] = sector;
    return &fat_cache_sectors[oldest];
}
