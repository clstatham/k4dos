use core::fmt;
use limine::{LimineTerminalRequest, LimineTerminalResponse};
use spin::Mutex;

static TERMINAL_REQUEST: LimineTerminalRequest = LimineTerminalRequest::new(0);

struct Writer {
    terminals: Option<&'static LimineTerminalResponse>,
}

unsafe impl Send for Writer {}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Get the Terminal response and cache it.
        let response = match self.terminals {
            None => {
                let response = TERMINAL_REQUEST.get_response().get().ok_or(fmt::Error)?;
                self.terminals = Some(response);
                response
            }
            Some(resp) => resp,
        };

        let write = response.write().ok_or(fmt::Error)?;

        // Output the string onto each terminal.
        for terminal in response.terminals() {
            write(terminal, s);
        }

        Ok(())
    }
}

static WRITER: Mutex<Writer> = Mutex::new(Writer { terminals: None });

pub fn _print(args: fmt::Arguments) {
    // XXX: Locking needs to happen around `print_fmt`, not `print_str`, as the former
    //       will call the latter potentially multiple times per invocation.
    let mut writer = WRITER.lock();
    fmt::Write::write_fmt(&mut *writer, args).ok();
}

#[macro_export]
macro_rules! terminal_print {
    ($($t:tt)*) => { $crate::io::_print(format_args!($($t)*)) };
}

#[macro_export]
macro_rules! terminal_println {
    ()          => { $crate::terminal_print!("\n"); };
    // On nightly, `format_args_nl!` could also be used.
    ($($t:tt)*) => { $crate::terminal_print!("{}\n", format_args!($($t)*)); };
}
