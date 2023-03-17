use log::Level;
use log::Log;

use crate::serial_print;
use crate::serial_println;

struct KaDOSLogger;

impl Log for KaDOSLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        // metadata.level() <= Level::Trace
        true
    }

    fn log(&self, record: &log::Record) {
        match record.level() {
            Level::Debug => serial_print!("\x1b[1;32m"),
            Level::Error => serial_print!("\x1b[1;31m"),
            Level::Info => serial_print!("\x1b[1;36m"),
            Level::Warn => serial_print!("\x1b[1;33m"),
            Level::Trace => serial_print!("\x1b[1;37m"),
        }
        serial_print!(
            "{}:{} [{}] {}",
            record.file().unwrap_or("(no file)"),
            record.line().unwrap_or(0),
            record.level(),
            record.args()
        );
        // terminal_println!(
        //     "{}:{} [{}] {}",
        //     record.file().unwrap_or("(no file)"),
        //     record.line().unwrap_or(0),
        //     record.level(),
        //     record.args()
        // );
        serial_println!("\x1b[0m");
    }

    fn flush(&self) {}
}

pub fn init() {
    log::set_logger(&KaDOSLogger).expect("error setting logger");
    log::set_max_level(log::LevelFilter::Trace);
}
