TARGET      := x86_64-unknown-none
CARGO       ?= cargo +nightly
TEST_CARGO  ?= cargo
NASM        ?= nasm
CC          ?= gcc
LD          ?= ld
QEMU        ?= qemu-system-x86_64
ZIG         ?= zig
# Cargo features for the kernel, e.g. make FEATURES=kernel-debug to run the boot-time FAT self-test
FEATURES    ?=

BUILD       := build
KERNEL      := kernel.elf
ISO         := cubeos.iso
HDD         := cubeos.img
CSH         := target/apps/$(TARGET)/release/csh
SPARKD      := target/apps/$(TARGET)/release/sparkd
GFXDEMO     := target/apps/$(TARGET)/release/gfxdemo
KERNEL_LIB  := target/$(TARGET)/release/libcube_core.a
DEMO_DRV    := target/apps/$(TARGET)/release/demo_drv.drv
ZIG_OBJ     := zig/zig-out/lib/cube_zig.o
INFO_IN     := info/*

HDD_SIZE_MB := 512
# Partition 1 is the boot FAT32 volume, partition 2 is the system volume that the kernel mounts (MBR index 1).
BOOT_OFFSET   := 1048576
SYSTEM_OFFSET := 210763776
SYSTEM_START  := 411648
HDD_SECTORS   := $(shell expr $(HDD_SIZE_MB) \* 2048)
SYSTEM_SECTORS := $(shell expr $(HDD_SECTORS) - $(SYSTEM_START))
SYSTEM_BYTES  := $(shell expr $(SYSTEM_SECTORS) \* 512)
SYSTEM_STAGE  := $(BUILD)/system-root
SYSTEM_IMG    := $(BUILD)/system.ext4
SYSTEM_RAM_IMG := $(BUILD)/system-ram.img
RAMDISK_MB    := 32
SYSTEM_KERNEL := ::/system/kernel/cubecore_1-0-0-dev.elf
INFO_OUT      := ::/system/info/

QEMU_FLAGS  := -m 256M -serial stdio -no-reboot -no-shutdown

.PHONY: all kernel csh sparkd gfxdemo demo-drv system-image iso hdd hdd-grub test run run-hdd clean FORCE

all: kernel csh

kernel: $(KERNEL)

csh: $(CSH)

sparkd: $(SPARKD)

demo-drv: $(DEMO_DRV)

iso: $(ISO)

system-image: $(KERNEL) $(CSH) $(SPARKD) $(GFXDEMO) $(DEMO_DRV) config/boot.cfg config/startup.cfg
	rm -rf $(SYSTEM_STAGE)
	mkdir -p $(SYSTEM_STAGE)/system/kernel $(SYSTEM_STAGE)/system/config $(SYSTEM_STAGE)/system/info $(SYSTEM_STAGE)/system/drivers $(SYSTEM_STAGE)/bin $(SYSTEM_STAGE)/home $(SYSTEM_STAGE)/tmp $(SYSTEM_STAGE)/etc
	cp $(KERNEL) $(SYSTEM_STAGE)/system/kernel/cubecore_1-0-0-dev.elf
	cp $(CSH) $(SYSTEM_STAGE)/bin/csh
	cp $(SPARKD) $(SYSTEM_STAGE)/bin/sparkd
	cp $(GFXDEMO) $(SYSTEM_STAGE)/bin/gfxdemo
	cp config/boot.cfg $(SYSTEM_STAGE)/system/config/boot.cfg
	cp config/startup.cfg $(SYSTEM_STAGE)/system/config/startup.cfg
	cp $(DEMO_DRV) $(SYSTEM_STAGE)/system/drivers/demo.drv
	cp $(INFO_IN) $(SYSTEM_STAGE)/system/info/
	truncate -s $(shell expr $(RAMDISK_MB) \* 1048576) $(SYSTEM_RAM_IMG)
	mke2fs -q -F -t ext4 -b 4096 -L CUBEOS -d $(SYSTEM_STAGE) $(SYSTEM_RAM_IMG)
	cp $(SYSTEM_RAM_IMG) $(SYSTEM_STAGE)/system/kernel/system.img
	truncate -s $(SYSTEM_BYTES) $(SYSTEM_IMG)
	mke2fs -q -F -t ext4 -b 4096 -L CUBEOS -d $(SYSTEM_STAGE) $(SYSTEM_IMG)

hdd: $(HDD) system-image
	dd if=$(SYSTEM_IMG) of=$(HDD) bs=512 seek=$(SYSTEM_START) conv=notrunc status=none
	mcopy -o -i $(HDD)@@$(BOOT_OFFSET) $(KERNEL) ::/kernel.elf
	@echo "  HDD image updated: $(HDD)"

$(HDD):
	dd if=/dev/zero of=$@ bs=1M count=$(HDD_SIZE_MB) status=none
	printf 'label: dos\n2048, 409600, c, *\n411648, , 83\n' | sfdisk -q $@
	mformat -i $@@@$(BOOT_OFFSET) -F -v CUBEBOOT ::

# Needs root for the loop device and GRUB install; the image itself is built without root.
hdd-grub: hdd config/grub-hdd.cfg
	sudo sh -c 'LOOP=$$(losetup -fP --show $(HDD)) && mkdir -p $(BUILD)/mnt && mount $${LOOP}p1 $(BUILD)/mnt && mkdir -p $(BUILD)/mnt/grub && cp config/grub-hdd.cfg $(BUILD)/mnt/grub/grub.cfg && grub-install --target=i386-pc --boot-directory=$(BUILD)/mnt --recheck $$LOOP; umount $(BUILD)/mnt; losetup -d $$LOOP'

$(BUILD)/boot.o: src/cube_core/boot.asm | $(BUILD)
	$(NASM) -f elf64 $< -o $@

$(BUILD)/ata.o: src/cube_core/ata.c | $(BUILD)
	$(CC) -ffreestanding -mno-red-zone -m64 -fno-stack-protector -c $< -o $@

$(BUILD):
	mkdir -p $@

FORCE:

$(KERNEL_LIB): FORCE
	cd src/cube_core && RUSTFLAGS="-C link-arg=-nostdlib -C relocation-model=static -C code-model=kernel" \
		$(CARGO) build -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem \
		--target $(TARGET) --release --target-dir ../../target $(if $(FEATURES),--features $(FEATURES))

$(ZIG_OBJ): FORCE
	cd zig && $(ZIG) build --release=small

$(KERNEL): $(BUILD)/boot.o $(BUILD)/ata.o $(ZIG_OBJ) $(KERNEL_LIB) linker.ld
	$(LD) -T linker.ld -o $@ $(BUILD)/boot.o $(BUILD)/ata.o $(ZIG_OBJ) $(KERNEL_LIB) --gc-sections -z max-page-size=0x1000

$(CSH): FORCE
	cd src/apps/csh && RUSTFLAGS="-C link-arg=-Tlinker.ld -C relocation-model=static -C code-model=large" \
		$(CARGO) build -Z build-std=core,compiler_builtins \
		--target $(TARGET) --release --target-dir ../../../target/apps

$(SPARKD): FORCE
	cd src/apps/sparkd && RUSTFLAGS="-C link-arg=-Tlinker.ld -C relocation-model=static -C code-model=large" \
		$(CARGO) build -Z build-std=core,compiler_builtins \
		--target $(TARGET) --release --target-dir ../../../target/apps

$(GFXDEMO): FORCE
	cd src/apps/gfxdemo && RUSTFLAGS="-C link-arg=-Tlinker.ld -C relocation-model=static -C code-model=large" \
		$(CARGO) build -Z build-std=core,compiler_builtins \
		--target $(TARGET) --release --target-dir ../../../target/apps

$(ISO): $(KERNEL) system-image config/grub.cfg
	rm -rf $(BUILD)/iso
	mkdir -p $(BUILD)/iso/boot/grub
	cp $(KERNEL) $(BUILD)/iso/boot/kernel.elf
	cp $(SYSTEM_RAM_IMG) $(BUILD)/iso/boot/system.img
	cp config/grub.cfg $(BUILD)/iso/boot/grub/grub.cfg
	grub-mkrescue -o $@ $(BUILD)/iso

test:
	cd tests/host && $(TEST_CARGO) test --quiet

run: $(ISO)
	$(QEMU) -cdrom $(ISO) $(QEMU_FLAGS)

run-hdd: hdd
	$(QEMU) -drive file=$(HDD),format=raw $(QEMU_FLAGS)

clean:
	rm -rf $(BUILD) target $(KERNEL) $(ISO) $(HDD) zig/zig-out zig/.zig-cache
$(DEMO_DRV): FORCE
	cd src/apps/demo_drv && RUSTFLAGS="-C link-arg=-Tlinker.ld -C relocation-model=static -C code-model=large" \
		$(CARGO) build -Z build-std=core,compiler_builtins \
		--target $(TARGET) --release --target-dir ../../../target/apps
	cp target/apps/$(TARGET)/release/demo_drv $@
