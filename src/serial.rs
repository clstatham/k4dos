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
}

#[doc(hidden)]
#[allow(unreachable_code)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL0
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}
