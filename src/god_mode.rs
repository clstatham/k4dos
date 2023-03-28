use alloc::{borrow::ToOwned, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use spin::Once;
use x86_64::instructions::interrupts;

use crate::{
    task::{get_scheduler, Task, TaskId},
    util::BlockingMutex, mem::{allocator::KERNEL_FRAME_ALLOCATOR, consts::PAGE_SIZE, addr::{VirtAddr, PhysAddr}},
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
        serial1_print!("\ngodmode > ");
        let cmd = read_cmd();
        let mut args = cmd.split_whitespace();
        let cmd = if let Some(cmd) = args.next() {
            cmd
        } else {
            continue;
        };
        // let args = args.collect::<Vec<_>>();

        serial1_println!();
        log::warn!("God said: {}", cmd);
        match cmd {
            "f" => {
                serial1_println!("Dumping free physical memory.");
                let fa = KERNEL_FRAME_ALLOCATOR.get().unwrap().lock();
                let mut total_space_pages = 0;
                for area in fa.free_chunks.iter() {
                    total_space_pages += area.size_in_pages();
                    serial1_println!("Free chunk at {:?}", area);
                }
                serial1_println!("Total free pages: {}", total_space_pages);
                serial1_println!("Total free bytes: {}", total_space_pages * PAGE_SIZE);
            }
            "vm" => {
                let pid = if let Some(Ok(pid)) = args.next().map(|arg| arg.parse()) {
                    TaskId::new(pid)
                } else {
                    serial1_println!("Invalid argument. Specify a PID to inspect vmem of.");
                    continue;
                };
                let sched = get_scheduler();
                let task = if let Some(task) = sched.find_task(pid) {
                    task
                } else {
                    serial1_println!("Invalid argument. PID not found.");
                    continue;
                };
                serial1_println!("Dumping virtual memory of pid {}. Check serial0 (stdio).", pid.as_usize());
                task.vmem().lock().log();
            }
            "d" => {
                let addr_type = if let Some(addr_type) = args.next() {
                    match addr_type {
                        "p" => "p",
                        "v" => "v",
                        _ => {
                            serial1_println!("Invalid argument. Specify `v` for dumping virtual memory or `p` for physical memory.");
                            continue;
                        }
                    }
                } else {
                    serial1_println!("Invalid argument. Specify `v` for dumping virtual memory or `p` for physical memory.");
                    continue;
                };
                let start = if let Some(Ok(start)) = args.next().map(|arg| usize::from_str_radix(arg, 16)) {
                    start
                } else {
                    serial1_println!("Invalid argument. Specify the physical address to dump the frame of.");
                    continue;
                };

                let ptr = if addr_type == "p" {
                    PhysAddr::new(start).as_hhdm_virt().as_ptr::<u64>()
                } else {
                    VirtAddr::new(start).as_ptr::<u64>()
                };
                let max_i = PAGE_SIZE/core::mem::size_of::<u64>();
                serial1_println!("Dumping page at {:#x}.", start);
                for i in 0..max_i/4 {
                    let i = i * 4;
                    serial1_print!("{:#016x}\t|\t", start + i * core::mem::size_of::<u64>());
                    for j in 0..4 {
                        let offset = i + j;
                        serial1_print!("{:016x} ", unsafe { ptr.add(offset).read_volatile() });
                    }
                    serial1_println!();
                }
            }
            _ => {}
        }
    }
}
