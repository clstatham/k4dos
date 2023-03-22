use super::idt::InterruptFrame;



#[repr(C)]
#[derive(Debug)]
pub struct SignalFrame {
    restart_syscall: usize,
    frame: InterruptFrame,
    sigmask: usize,
}

impl SignalFrame {
    pub fn from_syscall(restart: bool, syscall_result: isize, frame: &mut InterruptFrame, sigmask: usize) -> Self {
        Self { restart_syscall: if restart { frame.rax } else { syscall_result as usize }, frame: frame.clone(), sigmask }
    }
}