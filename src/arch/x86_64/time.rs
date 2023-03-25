use core::sync::atomic::{AtomicUsize, Ordering};

use x86::io::{inb, outb};

const PIT_FREQUENCY_HZ: usize = 1000;
pub const PIT_DIVIDEND: usize = 1193182;

static UPTIME_RAW: AtomicUsize = AtomicUsize::new(0);
static UPTIME_SEC: AtomicUsize = AtomicUsize::new(0);

pub fn get_uptime_ticks() -> usize {
    UPTIME_RAW.load(Ordering::SeqCst)
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
    let value = UPTIME_RAW.fetch_add(1, Ordering::Relaxed);
    if value % PIT_FREQUENCY_HZ == 0 {
        UPTIME_SEC.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn init() {
    set_pit_frequency(PIT_FREQUENCY_HZ);
}
