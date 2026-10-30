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

const std = @import("std");

pub fn build(b: *std.Build) void {
    // The kernel runs with SSE/MMX disabled and does not save FPU state on
    // interrupts, so the Zig objects must never emit SIMD or x87 instructions.
    const target = b.resolveTargetQuery(.{
        .cpu_arch = .x86_64,
        .os_tag = .freestanding,
        .abi = .none,
        .cpu_model = .{ .explicit = &std.Target.x86.cpu.x86_64 },
        .cpu_features_sub = std.Target.x86.featureSet(&.{ .mmx, .sse, .sse2, .avx, .avx2 }),
        .cpu_features_add = std.Target.x86.featureSet(&.{.soft_float}),
    });
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseSmall });

    const mod = b.createModule(.{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        // Interrupts can fire while Zig code uses the red zone below %rsp,
        // so the objects must not rely on it (same reason as -mno-red-zone for C).
        .red_zone = false,
        // The kernel stacks are fixed-size and pre-mapped; stack probing would need __zig_probe_stack.
        .stack_check = false,
    });

    // Zig only offers a standard install step for libraries and executables, so the
    // relocatable object is installed by hand and linked directly into the kernel.
    const obj = b.addObject(.{
        .name = "cube_zig",
        .root_module = mod,
        .use_llvm = true,
    });
    b.getInstallStep().dependOn(&b.addInstallFile(obj.getEmittedBin(), "lib/cube_zig.o").step);

    // Host tests run the FAT driver against an in-memory disk (see ata.zig test_image).
    const fat_tests = b.addTest(.{
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/fat_test.zig"),
            .target = b.graph.host,
            .optimize = .Debug,
        }),
    });
    const test_step = b.step("test", "Run host tests for the FAT driver");
    test_step.dependOn(&b.addRunArtifact(fat_tests).step);
}
