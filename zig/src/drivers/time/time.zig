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

// CubeOS shim for the NovumOS FAT driver: CubeOS has no RTC driver yet, so directory
// timestamps use a fixed date.

pub const DateTime = struct {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
};

pub fn get_datetime() DateTime {
    return .{ .year = 2026, .month = 1, .day = 1, .hour = 0, .minute = 0, .second = 0 };
}
