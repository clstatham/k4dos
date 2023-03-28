use alloc::{borrow::ToOwned, collections::VecDeque, string::String, sync::Arc};
use spin::Once;
use x86_64::instructions::interrupts;

use crate::{
    task::{get_scheduler, Task},
    util::BlockingMutex,
};

pub static GOD_MODE_TASK: Once<Arc<Task>> = Once::new();

pub static GOD_MODE_FIFO: Once<BlockingMutex<VecDeque<u8>>> = Once::new();

pub fn init() {
    GOD_MODE_FIFO.call_once(|| BlockingMutex::new(VecDeque::new()));
    let sched = get_scheduler();
    GOD_MODE_TASK.call_once(|| Task::new_kernel(sched, god_mode_repl, true));
    sched.push_runnable(GOD_MODE_TASK.get().unwrap().clone(), false);
}

fn read_cmd() -> String {
    let mut cmd = String::new();
    loop {
        let mut lock = GOD_MODE_FIFO.get().unwrap().try_lock();
        while let Ok(Some(ch)) = lock.as_mut().map(|lock| lock.pop_front()) {
            if ch == b'\n' || ch == b'\r' {
                drop(lock);
                return cmd;
            }
            let st = core::str::from_utf8(&[ch]).unwrap().to_owned();
            cmd.push_str(&st);
            serial1_print!("{}", st);
        }
        drop(lock);
        interrupts::enable_and_hlt();
    }
}

pub fn god_mode_repl() {
    loop {
        serial1_print!("\nGM > ");
        let cmd = read_cmd();
        serial1_println!();
        log::warn!("God said: {}", cmd);
    }
}
