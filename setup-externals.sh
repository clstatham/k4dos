#!/bin/bash

set -e

if [[ -z "$@" ]]; then
    echo "Usage: setup-externals.sh [COMMANDS]..."
    echo "Available commands:"
    echo "download       Downloads Busybox"
    echo "menuconfig     Configures Busybox with menuconfig"
    echo "clean          Cleans the output directory"
    echo "build          Builds Busybox"
    echo "makeimg        Creates the initramfs image for K4DOS"
    exit
fi

STARTDIR=$(pwd)

mkdir -p extern

cd extern
if [[ $@ =~ "download" ]];
then
    echo "Downloading Busybox."
    git clone git://busybox.net/busybox.git
    echo "Installing musl with pacman."
    sudo pacman -S musl
    sudo ln -s /usr/bin/ar /usr/bin/musl-ar
    sudo ln -s /usr/bin/strip /usr/bin/musl-strip
    echo "Now run 'setup-externals.sh menuconfig' and load kados.config, located in the same directory as this script"
fi


BUSYBOXDIR=$STARTDIR/extern/busybox
cd busybox
if [[ $@ =~ "menuconfig" ]];
then
    make menuconfig
fi
if [[ $@ =~ "defconfig" ]];
then
    make defconfig
fi

if [[ $@ =~ "allnoconfig" ]];
then
    make allnoconfig
fi
if [[ $@ =~ "clean" ]];
then
    make clean
fi
if [[ $@ =~ "build" ]];
then
    time make -j6
    make install
fi
if [[ $@ =~ "makeimg" ]];
then
    cd $STARTDIR
    mkdir -p initramfs/busybox_fs
    cd testapp
    make
    cd $STARTDIR
    cd initramfs/busybox_fs
    rm -f ../initramfs_busybox
    cp -r $BUSYBOXDIR/_install/* ./
    cp -r $BUSYBOXDIR/busybox_unstripped ./bin/busybox
    mkdir -p dev
    find . | cpio -ov --format=newc > ../initramfs
fi

cd $STARTDIR
echo "Done."