use crate::userland::syscall::SyscallFrame;

#[repr(C)]
#[derive(Debug)]
pub struct SignalFrame {
    restart_syscall: usize,
    frame: SyscallFrame,
    sigmask: usize,
}

impl SignalFrame {
    pub fn from_syscall(restart: bool, syscall_result: isize, frame: &mut SyscallFrame, sigmask: usize) -> Self {
        Self { restart_syscall: if restart { frame.rax } else { syscall_result as usize }, frame: frame.clone(), sigmask }
    }
}