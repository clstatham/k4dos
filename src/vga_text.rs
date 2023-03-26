use core::ops::Add;

use lazy_static::lazy_static;
use volatile::Volatile;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    pub fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }

    pub fn background(self) -> Color {
        unsafe { core::mem::transmute((self.0 >> 4) & 0xf) }
    }

    pub fn foreground(self) -> Color {
        unsafe { core::mem::transmute(self.0 & 0xf) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

pub const VGA_BUFFER_START_PADDR: usize = 0xb8000;
const BUFFER_HEIGHT: usize = 24;
const BUFFER_WIDTH: usize = 80;

#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

pub struct Writer {
    x: usize,
    y: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            0x8 => self.backspace(),
            b'\n' => self.new_line(),
            b'\r' => self.x = 0,
            byte => {
                if self.x >= BUFFER_WIDTH-1 {
                    self.new_line();
                }

                let row = self.y;
                let col = self.x;

                let color_code = self.color_code;
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code,
                });
                self.move_right();
            }
        }
        self.cursor_color_hook();
    }

    fn cursor_color_hook(&mut self) {
        // let cursor = self.buffer.chars[self.y][self.x].read();
        // for y in 0..BUFFER_HEIGHT {
        //     for x in 0..BUFFER_WIDTH {
        //         let chr = self.buffer.chars[y][x].read();
        //         if y == self.y && x == self.x {
        //             self.buffer.chars[y][x].write(ScreenChar { ascii_character: cursor.ascii_character, color_code: ColorCode::new(Color::White, Color::Cyan) });
        //         } else {
        //             self.buffer.chars[y][x].write(ScreenChar { ascii_character: chr.ascii_character, color_code: self.color_code });
        //         }
        //     }
        // }
    }

    pub fn backspace(&mut self) {
        let row = self.y;
        let col = self.x.saturating_sub(1);
        let color_code = self.color_code;
        self.buffer.chars[row][col].write(ScreenChar {
            ascii_character: b' ',
            color_code,
        });
        self.x = col;
        self.cursor_color_hook();
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            // match byte {
            //     0x20..=0x7e | b'\n' | b'\r' | 0x8 => self.write_byte(byte),
            //     // _ => self.write_byte(0xfe),
            //     _ => {},
            // }
            self.write_byte(byte)
        }
    }

    fn new_line(&mut self) {
        if self.y >= BUFFER_HEIGHT - 1 {
            for row in 1..BUFFER_HEIGHT {
                for col in 0..BUFFER_WIDTH {
                    let character = self.buffer.chars[row][col].read();
                    self.buffer.chars[row - 1][col].write(character);
                }
            }
            self.y = BUFFER_HEIGHT - 1;
            self.clear_row(self.y);
            self.x = 0;
        } else {
            self.y += 1;
            self.x = 0;
        }
        self.cursor_color_hook();
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
        self.cursor_color_hook();
    }
    fn clear_until_end(&mut self) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in self.x..BUFFER_WIDTH {
            self.buffer.chars[self.y][col].write(blank);
        }
        for row in self.y+1..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.cursor_color_hook();
    }
    fn clear_until_beginning(&mut self) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..self.x {
            self.buffer.chars[self.y][col].write(blank);
        }
        for row in 0..self.y-1 {
            self.clear_row(row);
        }
        self.cursor_color_hook();
    }
    fn clear_until_eol(&mut self) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in self.x..BUFFER_WIDTH {
            self.buffer.chars[self.y][col].write(blank);
        }
        self.cursor_color_hook();
    }
    fn clear_from_bol(&mut self) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..self.x {
            self.buffer.chars[self.y][col].write(blank);
        }
        self.cursor_color_hook();
    }
    fn clear_line(&mut self) {
        self.clear_row(self.y);
    }
    fn clear_all(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row)
        }
        self.cursor_color_hook();
    }
    fn move_up(&mut self) {
        let new_y = self.y.saturating_sub(1);
        let mut new_x = self.x;
        while new_x > 0 && self.buffer.chars[new_y][new_x].read().ascii_character == b' ' {
            new_x -= 1;
        }
        self.y = new_y;
        self.x = new_x;
        self.cursor_color_hook();
    }
    fn move_down(&mut self) {
        let new_y = self.y.add(1).min(BUFFER_HEIGHT-1);
        let mut new_x = self.x;
        while new_x > 0 && self.buffer.chars[new_y][new_x].read().ascii_character == b' ' {
            new_x -= 1;
        }
        self.y = new_y;
        self.x = new_x;
        self.cursor_color_hook();
    }
    fn move_left(&mut self) {
        self.x = self.x.saturating_sub(1);
        self.cursor_color_hook();
    }
    fn move_right(&mut self) {
        self.x = self.x.add(1).min(BUFFER_WIDTH-1);
        self.cursor_color_hook();
    }
}

impl core::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

lazy_static! {
    pub static ref WRITER: spin::Mutex<Writer> = spin::Mutex::new(Writer {
        y: 0,
        x: 0,
        color_code: ColorCode::new(Color::White, Color::Black),
        buffer: unsafe { &mut *(VGA_BUFFER_START_PADDR as *mut Buffer) },
    });
}

pub fn clear_screen() {
    WRITER.lock().clear_all()
}

pub fn backspace() {
    WRITER.lock().backspace()
}

pub fn set_color_code(color_code: ColorCode) {
    WRITER.lock().color_code = color_code;
}

pub fn get_color_code() -> ColorCode {
    WRITER.lock().color_code
}

pub fn set_cursor_x(x: usize) {
    WRITER.lock().x = x.min(BUFFER_WIDTH-1);
}

pub fn set_cursor_y(y: usize) {
    WRITER.lock().y = y.min(BUFFER_HEIGHT-1);
}

pub fn set_cursor_xy(xy: (usize, usize)) {
    set_cursor_x(xy.0.min(BUFFER_WIDTH-1));
    set_cursor_y(xy.1.min(BUFFER_HEIGHT-1));
}

pub fn cursor_xy() -> (usize, usize) {
    let writer = WRITER.lock();
    (writer.x, writer.y)
}

pub fn write_byte(byte: u8) {
    WRITER.lock().write_byte(byte);
}

pub fn clear_until_end() {
    WRITER.lock().clear_until_end();
}

pub fn clear_until_beginning() {
    WRITER.lock().clear_until_beginning();
}

pub fn clear_from_bol() {
    WRITER.lock().clear_from_bol();
}

pub fn clear_until_eol() {
    WRITER.lock().clear_until_eol();
}

pub fn clear_line() {
    WRITER.lock().clear_line();
}

pub fn move_up() {
    WRITER.lock().move_up();
}

pub fn move_down() {
    WRITER.lock().move_down();
}

pub fn move_left() {
    WRITER.lock().move_left();
}

pub fn move_right() {
    WRITER.lock().move_right();
}

#[macro_export]
macro_rules! vga_print {
    ($($arg:tt)*) => ($crate::vga_text::_vga_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! vga_println {
    () => ($crate::vga_print!("\n"));
    ($($arg:tt)*) => ($crate::vga_print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _vga_print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}
