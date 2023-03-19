#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

void main(void) {
    puts("I'm a user mode process written in C!\n");
    for (;;) {}
}