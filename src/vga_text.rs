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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

pub const VGA_BUFFER_START_PADDR: usize = 0xb8000;
const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

// pub fn remap_vga_memory(new_table: &mut PageTable) {
//     let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(0xb8000));
//     let page = Page::<Size4KiB>::containing_address(VirtAddr::new(0xb8000));
//     new_table.map_page(UserVirtAddr::new(page.start_address().as_u64()).unwrap(), frame.start_address(), PageAttrs::WRITABLE | PageAttrs::PRESENT);// new_table.map_page(UserVirtAddr::new(0xb8f00).unwrap(), PhysAddr::new(0xb8f00), PageAttrs::WRITABLE);
// }

pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            0x8 => self.backspace(),
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                let color_code = self.color_code;
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code,
                });
                self.column_position += 1;
            }
        }
    }

    pub fn backspace(&mut self) {
        let row = BUFFER_HEIGHT - 1;
        let col = self.column_position.saturating_sub(1);
        let color_code = self.color_code;
        self.buffer.chars[row][col].write(ScreenChar {
            ascii_character: b' ',
            color_code,
        });
        self.column_position = col;
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            // match byte {
            //     0x20..=0x7e | b'\n' | 0x8 => self.write_byte(byte),
            //     _ => self.write_byte(0xfe),
            // }
            self.write_byte(byte)
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }
    fn clear_all(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row)
        }
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
        column_position: 0,
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
