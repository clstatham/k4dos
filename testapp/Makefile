.DEFAULT: all
.PHONY: all

MUSL_CC ?= musl-gcc

# cd .. && ./setup-externals.sh makeimg
all:
	$(MUSL_CC) main.c -m64 -pie -static -o testapp
	cp testapp ../initramfs/busybox_fs/bin/
	