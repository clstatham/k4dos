#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/wait.h>

void main(void) {
    printf("I'm a user mode process written in C!\n");
    if (fork() == 0) {
        printf("I'm the child process!\n");
    } else {
        printf("I'm the parent process!\n");
        wait(NULL);
    }
    exit(0);
}