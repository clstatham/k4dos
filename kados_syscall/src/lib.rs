use core::arch::asm;

use consts::*;

pub mod consts;

#[no_mangle]
#[inline(always)]
#[allow(clippy::missing_safety_doc)]
pub extern "C" fn syscall0(n: usize) -> isize {
    let ret: isize;
    unsafe { asm!("syscall", in("rax") n, lateout("rax") ret); }
    ret
}

#[no_mangle]
#[inline(always)]
#[allow(clippy::missing_safety_doc)]
pub extern "C" fn syscall1(n: usize, a1: usize) -> isize {
    let ret: isize;
    unsafe { asm!("syscall", in("rax") n, in("rdi") a1, lateout("rax") ret); }
    ret
}

#[no_mangle]
#[inline(always)]
#[allow(clippy::missing_safety_doc)]
pub extern "C" fn syscall2(n: usize, a1: usize, a2: usize) -> isize {
    let ret: isize;
    unsafe { asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, lateout("rax") ret); }
    ret
}

#[no_mangle]
#[inline(always)]
#[allow(clippy::missing_safety_doc)]
pub extern "C" fn syscall3(n: usize, a1: usize, a2: usize, a3: usize) -> isize {
    let ret: isize;
    unsafe { asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret); }
    ret
}

#[no_mangle]
#[inline(always)]
#[allow(clippy::missing_safety_doc)]
pub extern "C" fn syscall4(n: usize, a1: usize, a2: usize, a3: usize, a4: usize) -> isize {
    let ret: isize;
    unsafe { asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, lateout("rax") ret); }
    ret
}

#[no_mangle]
#[inline(always)]
#[allow(clippy::missing_safety_doc)]
pub extern "C" fn syscall5(n: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize) -> isize {
    let ret: isize;
    unsafe { asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, in("r8") a5, lateout("rax") ret); }
    ret
}

#[no_mangle]
#[inline(always)]
#[allow(clippy::missing_safety_doc)]
pub extern "C" fn syscall6(n: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize) -> isize {
    let ret: isize;
    unsafe { asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, in("r8") a5, in("r9") a6, lateout("rax") ret); }
    ret
}

pub fn sys_arch_prctl(code: i32, address: usize) -> SyscallResult {
    syscall_result(syscall2(SYS_ARCH_PRCTL, code as usize, address))
}

pub fn sys_set_tid_address(address: usize) -> SyscallResult {
    syscall_result(syscall1(SYS_SET_TID_ADDRESS, address))
}

pub fn sys_write(fd: i32, address: usize, len: usize) -> SyscallResult {
    syscall_result(syscall3(SYS_WRITE, fd as usize, address, len))
}

pub fn sys_writev(fd: i32, iov_base: usize, iov_count: usize) -> SyscallResult {
    syscall_result(syscall3(SYS_WRITEV, fd as usize, iov_base, iov_count))
}

pub fn sys_read(fd: i32, address: usize, len: usize) -> SyscallResult {
    syscall_result(syscall3(SYS_READ, fd as usize, address, len))
}

pub fn sys_fork() -> SyscallResult {
    syscall_result(syscall0(SYS_FORK))
}

pub fn sys_wait4(pid: i32, status_addr: usize, options: i32, rusage_addr: usize) -> SyscallResult {
    syscall_result(syscall4(SYS_WAIT4, pid as usize, status_addr, options as usize, rusage_addr))
}

pub fn sys_execve(path_addr: usize, argv_addr: usize, envp_addr: usize) -> SyscallResult {
    syscall_result(syscall3(SYS_EXECVE, path_addr, argv_addr, envp_addr))
}

pub fn sys_getcwd(buf_addr: usize, buf_len: usize) -> SyscallResult {
    syscall_result(syscall2(SYS_GETCWD, buf_addr, buf_len))
}
