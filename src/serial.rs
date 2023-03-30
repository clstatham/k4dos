use lazy_static::lazy_static;

use uart_16550::SerialPort;
use x86::io::inb;

use crate::util::lock::IrqMutex;

pub const SERIAL0_IOPORT: u16 = 0x3f8;
pub const SERIAL1_IOPORT: u16 = 0x2f8;
pub const SERIAL2_IOPORT: u16 = 0x3e8;

lazy_static! {
    pub static ref SERIAL0: IrqMutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(SERIAL0_IOPORT) };
        serial_port.init();

        IrqMutex::new(serial_port)
    };
    pub static ref SERIAL1: IrqMutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(SERIAL1_IOPORT) };
        serial_port.init();
        IrqMutex::new(serial_port)
    };
    pub static ref SERIAL2: IrqMutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(SERIAL2_IOPORT) };
        serial_port.init();
        IrqMutex::new(serial_port)
    };
}

#[doc(hidden)]
#[allow(unreachable_code)]
pub fn _print0(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    // #[cfg(debug_assertions)]
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
    () => ($crate::serial0_print!("\n"));
    ($fmt:expr) => ($crate::serial0_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial0_print!(
        concat!($fmt, "\n"), $($arg)*));
}

#[doc(hidden)]
#[allow(unreachable_code)]
pub fn _print1(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    // #[cfg(debug_assertions)]
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
}

#[inline]
pub fn serial1_recv() -> Option<u8> {
    // #[cfg(debug_assertions)]
    unsafe {
        let line_sts = inb(SERIAL1_IOPORT + 5);
        if line_sts & 0x1 != 0 {
            return Some(inb(SERIAL1_IOPORT));
        }
        None
    }
    // #[cfg(not(debug_assertions))]
    // None

    // })
}

/// Prints to the host through the user serial interface.
#[macro_export]
macro_rules! serial1_print {
    ($($arg:tt)*) => {
        $crate::serial::_print1(format_args!($($arg)*))
    };
}

/// Prints to the host through the user serial interface, appending a newline.
#[macro_export]
macro_rules! serial1_println {
    () => ($crate::serial1_print!("\r\n"));
    ($fmt:expr) => ($crate::serial1_print!(concat!($fmt, "\r\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial1_print!(
        concat!($fmt, "\r\n"), $($arg)*));
}

#[doc(hidden)]
#[allow(unreachable_code)]
pub fn _print2(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    // #[cfg(debug_assertions)]
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL2
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
}

/// Prints to the host through the TTY logging serial interface.
#[macro_export]
macro_rules! serial2_print {
    ($($arg:tt)*) => {
        $crate::serial::_print2(format_args!($($arg)*))
    };
}

/// Prints to the host through the TTY logging serial interface, appending a newline.
#[macro_export]
macro_rules! serial2_println {
    () => ($crate::serial2_print!("\n"));
    ($fmt:expr) => ($crate::serial2_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial2_print!(
        concat!($fmt, "\n"), $($arg)*));
}
