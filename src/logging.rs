use log::Level;
use log::Log;

use crate::serial0_print;
use crate::serial0_println;
use crate::task::SCHEDULER;

struct KaDOSLogger;

impl Log for KaDOSLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        match record.level() {
            Level::Debug => serial0_print!("\x1b[1;36m"), // cyan
            Level::Error => serial0_print!("\x1b[1;31m"), // red
            Level::Info => serial0_print!("\x1b[1;32m"),  // green
            Level::Warn => serial0_print!("\x1b[1;33m"),  // yellow
            Level::Trace => serial0_print!("\x1b[1;37m"), // white
        }
        if let Some(sched) = SCHEDULER.get() {
            if let Some(current) = sched.current_task_opt() {
                serial0_print!(
                    "[{}]\t[{}] - {}",
                    record.level(),
                    current.pid().as_usize(),
                    record.args()
                );
            } else {
                serial0_print!("[{}]\t{}", record.level(), record.args());
            }
        } else {
            serial0_print!("[{}]\t{}", record.level(), record.args());
        }
        serial0_println!("\x1b[0m"); // reset color
    }

    fn flush(&self) {}
}

pub fn init() {
    log::set_logger(&KaDOSLogger).expect("error setting logger");
    let level = if let Some(level) = option_env!("RUST_LOG")
        .or(option_env!("KADOS_LOG"))
        .or(option_env!("K4DOS_LOG"))
    {
        match level {
            "error" => log::LevelFilter::Error,
            "warn" => log::LevelFilter::Warn,
            "info" => log::LevelFilter::Info,
            "debug" => log::LevelFilter::Debug,
            "trace" => log::LevelFilter::Trace,
            _ => log::LevelFilter::Info,
        }
    } else {
        log::LevelFilter::Info
    };
    log::set_max_level(level);
}
