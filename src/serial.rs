use lazy_static::lazy_static;

use uart_16550::SerialPort;

use crate::util::lock::SpinLock;

pub const SERIAL0_IOPORT: u16 = 0x3f8;
pub const SERIAL1_IOPORT: u16 = 0x2f8;
pub const SERIAL0_IRQ: u8 = 4;
pub const SERIAL1_IRQ: u8 = 3;

lazy_static! {
    pub static ref SERIAL0: SpinLock<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(SERIAL0_IOPORT) };
        serial_port.init();
        SpinLock::new(serial_port)
    };
    pub static ref SERIAL1: SpinLock<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(SERIAL1_IOPORT) };
        serial_port.init();
        SpinLock::new(serial_port)
    };
}

#[doc(hidden)]
#[allow(unreachable_code)]
pub fn _print0(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL0
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
}

/// Prints to the host through the logging serial interface.
#[macro_export]
macro_rules! serial0_print {
    ($($arg:tt)*) => {
        $crate::serial::_print0(format_args!($($arg)*))
    };
}

/// Prints to the host through the logging serial interface, appending a newline.
#[macro_export]
macro_rules! serial0_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial0_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}

#[doc(hidden)]
#[allow(unreachable_code)]
pub fn _print1(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
}

#[inline]
pub fn serial1_recv() -> u8 {
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL1.lock().receive()
    })
}

/// Prints to the host through the logging serial interface.
#[macro_export]
macro_rules! serial1_print {
    ($($arg:tt)*) => {
        $crate::serial::_print1(format_args!($($arg)*))
    };
}

/// Prints to the host through the logging serial interface, appending a newline.
#[macro_export]
macro_rules! serial1_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial1_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}

