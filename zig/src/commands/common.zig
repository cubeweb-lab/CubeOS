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

// CubeOS shim for the NovumOS FAT driver: console output and string helpers the FAT code uses.

const builtin = @import("builtin");
const str = @import("../kernel/str.zig");

pub const std_mem_eql = str.std_mem_eql;
pub const startsWith = str.startsWith;
pub const copy = str.copy;

extern fn cube_console_putc(c: u8) void;

pub var current_path: [256]u8 = @splat(0);
pub var current_path_len: usize = 0;

pub fn print_char(c: u8) void {
    if (builtin.is_test) return;
    cube_console_putc(c);
}

pub fn printZ(s: []const u8) void {
    for (s) |c| {
        if (c == 0) break;
        print_char(c);
    }
}

pub fn printError(s: []const u8) void {
    printZ(s);
}

pub fn printNum(n: i32) void {
    var v: u32 = undefined;
    if (n < 0) {
        print_char('-');
        v = @intCast(-@as(i64, n));
    } else {
        v = @intCast(n);
    }

    var buf: [10]u8 = undefined;
    var i: usize = buf.len;
    while (true) {
        i -= 1;
        buf[i] = @intCast('0' + v % 10);
        v /= 10;
        if (v == 0) break;
    }
    printZ(buf[i..]);
}
