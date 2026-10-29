TARGET      := x86_64-unknown-none
CARGO       ?= cargo +nightly
TEST_CARGO  ?= cargo
NASM        ?= nasm
CC          ?= gcc
LD          ?= ld
QEMU        ?= qemu-system-x86_64

BUILD       := build
KERNEL      := kernel.elf
ISO         := cubeos.iso
HDD         := cubeos.img
CSH         := target/apps/$(TARGET)/release/csh
KERNEL_LIB  := target/$(TARGET)/release/libcube_core.a

HDD_SIZE_MB := 512
# Partition 1 is the boot FAT32 volume, partition 2 is the system volume that the kernel mounts (MBR index 1).
BOOT_OFFSET   := 1048576
SYSTEM_OFFSET := 210763776
SYSTEM_KERNEL := ::/system/kernel/cubecore_1-0-0-dev.elf

QEMU_FLAGS  := -m 256M -serial stdio -no-reboot -no-shutdown

.PHONY: all kernel csh iso hdd hdd-grub test run run-hdd clean FORCE

all: kernel csh

kernel: $(KERNEL)

csh: $(CSH)

iso: $(ISO)

hdd: $(HDD) $(KERNEL) $(CSH) config/boot.cfg
	-mmd -i $(HDD)@@$(SYSTEM_OFFSET) ::/system ::/system/kernel ::/system/config ::/bin ::/home ::/tmp ::/etc
	mcopy -o -i $(HDD)@@$(SYSTEM_OFFSET) $(KERNEL) $(SYSTEM_KERNEL)
	mcopy -o -i $(HDD)@@$(SYSTEM_OFFSET) $(CSH) ::/bin/csh
	mcopy -o -i $(HDD)@@$(SYSTEM_OFFSET) config/boot.cfg ::/system/config/boot.cfg
	mcopy -o -i $(HDD)@@$(BOOT_OFFSET) $(KERNEL) ::/kernel.elf
	@echo "  HDD image updated: $(HDD)"

$(HDD):
	dd if=/dev/zero of=$@ bs=1M count=$(HDD_SIZE_MB) status=none
	printf 'label: dos\n2048, 409600, c, *\n411648, , c\n' | sfdisk -q $@
	mformat -i $@@@$(BOOT_OFFSET) -F -v CUBEBOOT ::
	mformat -i $@@@$(SYSTEM_OFFSET) -F -v CUBEOS ::

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
		$(CARGO) build -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem \
		--target $(TARGET) --release --target-dir ../../target

$(KERNEL): $(BUILD)/boot.o $(BUILD)/ata.o $(KERNEL_LIB) linker.ld
	$(LD) -T linker.ld -o $@ $(BUILD)/boot.o $(BUILD)/ata.o $(KERNEL_LIB) --gc-sections -z max-page-size=0x1000

$(CSH): FORCE
	cd src/apps/csh && RUSTFLAGS="-C link-arg=-Tlinker.ld -C relocation-model=static -C code-model=large" \
		$(CARGO) build -Z build-std=core,compiler_builtins \
		--target $(TARGET) --release --target-dir ../../../target/apps

$(ISO): $(KERNEL) config/grub.cfg
	rm -rf $(BUILD)/iso
	mkdir -p $(BUILD)/iso/boot/grub
	cp $(KERNEL) $(BUILD)/iso/boot/kernel.elf
	cp config/grub.cfg $(BUILD)/iso/boot/grub/grub.cfg
	grub-mkrescue -o $@ $(BUILD)/iso

test:
	cd tests/host && $(TEST_CARGO) test --quiet

run: $(ISO)
	$(QEMU) -cdrom $(ISO) $(QEMU_FLAGS)

run-hdd: hdd
	$(QEMU) -drive file=$(HDD),format=raw $(QEMU_FLAGS)

clean:
	rm -rf $(BUILD) target $(KERNEL) $(ISO) $(HDD)
