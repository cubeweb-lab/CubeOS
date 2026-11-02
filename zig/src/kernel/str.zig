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

pub fn std_mem_eql(a: []const u8, b: []const u8) bool {
    if (a.len != b.len) return false;
    for (a, 0..) |item, i| {
        if (item != b[i]) return false;
    }
    return true;
}

pub fn startsWith(a: []const u8, b: []const u8) bool {
    if (a.len < b.len) return false;
    return std_mem_eql(a[0..b.len], b);
}

pub fn endsWith(a: []const u8, b: []const u8) bool {
    if (a.len < b.len) return false;
    return std_mem_eql(a[a.len - b.len ..], b);
}

pub fn asciiLower(c: u8) u8 {
    if (c >= 'A' and c <= 'Z') return c + 32;
    return c;
}

pub fn startsWithIgnoreCase(a: []const u8, b: []const u8) bool {
    if (a.len < b.len) return false;
    for (0..b.len) |i| {
        if (asciiLower(a[i]) != asciiLower(b[i])) return false;
    }
    return true;
}

pub fn lastIndexOf(slice: []const u8, c: u8) ?usize {
    var i: usize = slice.len;
    while (i > 0) {
        i -= 1;
        if (slice[i] == c) return i;
    }
    return null;
}

pub fn copy(dest: []u8, src: []const u8) void {
    const len = @min(dest.len, src.len);
    for (0..len) |i| dest[i] = src[i];
}

pub fn math_abs(n: i32) i32 {
    return if (n < 0) -n else n;
}

pub fn math_max(a: i32, b: i32) i32 {
    return if (a > b) a else b;
}

pub fn math_min(a: i32, b: i32) i32 {
    return if (a < b) a else b;
}
