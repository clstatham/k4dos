.DEFAULT: all
.PHONY: all

MUSL_CC ?= musl-gcc

all:
	$(MUSL_CC) main.c -m64 -static -nostdlib -o testapp
	cp testapp ../initramfs/busybox_fs/bin/
	cd .. && ./setup-externals.sh makeimg