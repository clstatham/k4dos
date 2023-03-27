#! /bin/sh
#
# This script will be executed by `cargo run`.

set -xe

# Cargo passes the path to the built executable as the first argument.
KERNEL=$1

# Copy the needed files into an ISO image.
mkdir -p target/iso_root/boot/grub
cp $KERNEL target/iso_root/boot
cp conf/grub.cfg target/iso_root/boot/grub

grub-mkrescue -o $KERNEL.iso target/iso_root  >&2

# Run the created image with QEMU.
# -machine q35 -cpu EPYC
echo "Running in release mode." >&2
qemu-system-x86_64 \
    -cpu host -enable-kvm -M smm=off \
    -D target/log.txt -d int,guest_errors -no-reboot -no-shutdown \
    -s \
    -m 4G \
    -cdrom $KERNEL.iso >&2
