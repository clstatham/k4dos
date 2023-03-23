use log::Level;
use log::Log;

use crate::serial0_print;
use crate::serial0_println;
use crate::task::SCHEDULER;

struct KaDOSLogger;

impl Log for KaDOSLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        // metadata.level() <= Level::Trace
        true
    }

    fn log(&self, record: &log::Record) {
        match record.level() {
            Level::Debug => serial0_print!("\x1b[1;32m"),
            Level::Error => serial0_print!("\x1b[1;31m"),
            Level::Info => serial0_print!("\x1b[1;36m"),
            Level::Warn => serial0_print!("\x1b[1;33m"),
            Level::Trace => serial0_print!("\x1b[1;37m"),
        }
        if let Some(sched) = SCHEDULER.get() {
            if let Some(current) = sched.current_task_opt() {
                serial0_print!(
                    "[{}] [{}] - {}",
                    // record.file().unwrap_or("(no file)"),
                    // record.line().unwrap_or(0),
                    record.level(),
                    current.pid().as_usize(),
                    record.args()
                );
            } else {
                serial0_print!(
                    "[{}] {}",
                    // record.file().unwrap_or("(no file)"),
                    // record.line().unwrap_or(0),
                    record.level(),
                    record.args()
                );
            }
        } else {
            serial0_print!(
                "[{}] {}",
                // record.file().unwrap_or("(no file)"),
                // record.line().unwrap_or(0),
                record.level(),
                record.args()
            );
        }
        serial0_println!("\x1b[0m");
    }

    fn flush(&self) {}
}

pub fn init() {
    log::set_logger(&KaDOSLogger).expect("error setting logger");
    log::set_max_level(log::LevelFilter::Trace);
}
