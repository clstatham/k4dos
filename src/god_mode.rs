use alloc::{borrow::ToOwned, collections::VecDeque, string::String, sync::Arc};
use spin::Once;
use x86_64::instructions::interrupts;

use crate::{
    mem::{
        addr::PhysAddr,
        allocator::{GLOBAL_ALLOC, KERNEL_FRAME_ALLOCATOR},
        consts::{KERNEL_HEAP_SIZE, PAGE_SIZE},
    },
    task::{get_scheduler, Task, TaskId},
    util::{align_down, BlockingMutex},
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

        serial1_println!();
        log::warn!("God said: {}", cmd);
        match cmd {
            "f" | "frames" => {
                serial1_println!("Dumping free physical memory.");
                let fa = KERNEL_FRAME_ALLOCATOR.get().unwrap().lock();
                let mut total_space_pages = 0;
                for area in fa.free_regions() {
                    total_space_pages += area.size_in_pages();
                    serial1_println!("Free chunk at {:?}", area);
                }
                serial1_println!("Total free pages: {}", total_space_pages);
                serial1_println!("Total free bytes: {}", total_space_pages * PAGE_SIZE);
            }
            "vm" | "vmem" => {
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
                serial1_println!(
                    "Dumping virtual memory of pid {}. Check serial0 (stdio).",
                    pid.as_usize()
                );
                task.vmem().lock().log();
            }
            "x" | "examine" => {
                let start = if let Some(Ok(start)) =
                    args.next().map(|arg| usize::from_str_radix(arg, 16))
                {
                    align_down(start, PAGE_SIZE)
                } else {
                    serial1_println!("Invalid argument. Specify the address to dump the frame of.");
                    continue;
                };

                let ptr = PhysAddr::new(start).as_hhdm_virt().as_raw_ptr::<u64>();

                let max_i = PAGE_SIZE / core::mem::size_of::<u64>();
                serial1_println!("Dumping frame at {:#x}.", start);
                for i in 0..max_i / 4 {
                    let i = i * 4;
                    serial1_print!("{:#016x} >> ", start + i * core::mem::size_of::<u64>());
                    for j in 0..4 {
                        let offset = i + j;
                        serial1_print!("{:016x} ", unsafe { ptr.add(offset).read_volatile() });
                    }
                    serial1_println!();
                }
            }
            "h" | "heap" => {
                let lock = GLOBAL_ALLOC.try_lock();
                if let Some(lock) = lock {
                    serial1_println!("Kernel heap size:           {:#08x}", KERNEL_HEAP_SIZE);
                    serial1_println!(
                        "Kernel heap usage (actual): {:#08x} ({:.4}%)",
                        lock.stats_alloc_actual(),
                        lock.stats_alloc_actual() as f64 / KERNEL_HEAP_SIZE as f64 * 100.0
                    );
                    serial1_println!(
                        "Kernel heap usage (user):   {:#08x} ({:.4}%)",
                        lock.stats_alloc_user(),
                        lock.stats_alloc_user() as f64 / KERNEL_HEAP_SIZE as f64 * 100.0
                    );
                } else {
                    serial1_println!("Error locking global allocator.");
                }
            }
            _ => {}
        }
    }
}
