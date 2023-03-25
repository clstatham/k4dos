#! /bin/sh
#
# This script will be executed by `cargo run`.

set -xe

# LIMINE_GIT_URL="https://github.com/limine-bootloader/limine.git"

# Cargo passes the path to the built executable as the first argument.
KERNEL=$1

# # Clone the `limine` repository if we don't have it yet.
# if [ ! -d target/limine ]; then
#     git clone $LIMINE_GIT_URL --depth=1 --branch v3.0-branch-binary target/limine
# fi

# # Make sure we have an up-to-date version of the bootloader.
# cd target/limine
# git fetch
# make
# cd -

# Copy the needed files into an ISO image.
mkdir -p target/iso_root/boot/grub
cp $KERNEL target/iso_root/boot
cp conf/grub.cfg target/iso_root/boot/grub

# xorriso -as mkisofs                                             \
#     -b limine-cd.bin                                            \
#     -no-emul-boot -boot-load-size 4 -boot-info-table            \
#     --efi-boot limine-cd-efi.bin                                \
#     -efi-boot-part --efi-boot-image --protective-msdos-label    \
#     target/iso_root -o $KERNEL.iso

grub-mkrescue -o $KERNEL.iso target/iso_root

# For the image to be bootable on BIOS systems, we must run `limine-deploy` on it.
# target/limine/limine-deploy $KERNEL.iso

# Run the created image with QEMU.
qemu-system-x86_64 \
    -machine q35 -cpu EPYC -M smm=off \
    -D target/log.txt -d int,guest_errors -no-reboot -no-shutdown \
    -s -S \
    -serial mon:stdio \
    -serial mon:pty \
    -m 1G \
    -cdrom $KERNEL.iso