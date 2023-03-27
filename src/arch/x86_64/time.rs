use core::sync::atomic::{AtomicUsize, Ordering};

use x86::io::{inb, outb};

use crate::{util::IrqMutex, userland::syscall::syscall_impl::time::TimeSpec};

const PIT_FREQUENCY_HZ: usize = 1000;
pub const PIT_DIVIDEND: usize = 1193182;

static UPTIME_RAW: AtomicUsize = AtomicUsize::new(0);
static UPTIME_SEC: AtomicUsize = AtomicUsize::new(0);

pub static EPOCH: AtomicUsize = AtomicUsize::new(usize::MAX);
pub static RT_CLOCK: IrqMutex<TimeSpec> = IrqMutex::new(TimeSpec { tv_sec: 0, tv_nsec: 0 });

pub fn get_uptime_ns() -> usize {
    let ts = get_rt_clock();
    (ts.tv_sec * 1000000000 + ts.tv_nsec) as usize
}

pub fn get_uptime_ms() -> usize {
    get_uptime_ns() / 1000000
}

pub fn get_rt_clock() -> TimeSpec {
    *RT_CLOCK.lock()
}

pub fn get_pit_count() -> u16 {
    unsafe {
        outb(0x43, 0);

        let lower = inb(0x40) as u16;
        let higher = inb(0x40) as u16;

        (higher << 8) | lower
    }
}

pub fn set_reload_value(new_count: u16) {
    unsafe {
        outb(0x43, 0x34);
        outb(0x40, new_count as u8);
        outb(0x40, (new_count >> 8) as u8);
    }
}

pub fn set_pit_frequency(frequency: usize) {
    let mut new_divisor = PIT_DIVIDEND / frequency;

    if PIT_DIVIDEND % frequency > frequency / 2 {
        new_divisor += 1;
    }

    set_reload_value(new_divisor as u16);
}

pub fn pit_irq() {
    {
        let interval = TimeSpec {
            tv_sec: 0,
            tv_nsec: (1000000000 / PIT_FREQUENCY_HZ) as isize,
        };

        let mut clk = RT_CLOCK.lock();

        if clk.tv_nsec + interval.tv_nsec > 999999999 {
            let diff = (clk.tv_nsec + interval.tv_nsec) - 1000000000;

            clk.tv_nsec = diff;
            clk.tv_sec += 1;
        } else {
            clk.tv_nsec += interval.tv_nsec;
        }
    }

    let value = UPTIME_RAW.fetch_add(1, Ordering::Relaxed);
    if value % PIT_FREQUENCY_HZ == 0 {
        UPTIME_SEC.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn init(boot_time: i64) {
    EPOCH.store(boot_time as usize, Ordering::SeqCst);
    RT_CLOCK.lock().tv_sec = boot_time as isize;
    set_pit_frequency(PIT_FREQUENCY_HZ);
}
