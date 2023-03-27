use core::ops::Add;

use alloc::boxed::Box;
use embedded_graphics::{prelude::{DrawTarget, OriginDimensions, Size, IntoStorage, Dimensions, Point, RgbColor}, pixelcolor::Rgb888, Pixel, mono_font::{MonoTextStyle, ascii::{FONT_10X20}, MonoFont}, text::{Text, Alignment}, Drawable};
use multiboot2::FramebufferTag;
use spin::Once;

use crate::{util::{KResult, IrqMutex, IrqMutexGuard}, mem::addr::{VirtAddr, PhysAddr}, vga_text::{BUFFER_WIDTH, BUFFER_HEIGHT}};

// static MONO_FONT: Once<MonoTextStyle<'static, Rgb888>> = Once::new();

const FONT: MonoFont = FONT_10X20;


pub struct FrameBuffer {
    back_buffer: Box<[u32]>,
    start_addr: VirtAddr,
    width: usize,
    height: usize,
    bpp: usize,
    text_buf: [[u8; BUFFER_WIDTH]; BUFFER_HEIGHT],
    text_cursor_x: usize,
    text_cursor_y: usize,
    text_fgcolor: Rgb888,
}

impl FrameBuffer {
    pub fn render_text_buf(&mut self) {
        let mut out = [b' '; BUFFER_WIDTH * BUFFER_HEIGHT + BUFFER_HEIGHT];
        for line in 0..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                out[col + line * (BUFFER_WIDTH + 1)] = self.text_buf[line][col];
            }
            out[BUFFER_WIDTH + line * (BUFFER_WIDTH + 1)] = b'\n';
        }

        let mono_font = MonoTextStyle::new(&FONT, self.text_fgcolor);

        Text::with_alignment(core::str::from_utf8(&out).unwrap(), self.bounding_box().top_left + Point::new(FONT.character_size.width as i32, FONT.character_size.height as i32), mono_font, Alignment::Left).draw(self).unwrap();
    }

    pub fn clear_pixels(&mut self) {
        <Self as DrawTarget>::clear(self, Rgb888::new(20, 20, 20)).unwrap();
    }

    pub fn flip(&mut self) {
        unsafe {
            self.start_addr.as_mut_ptr::<u32>().copy_from_nonoverlapping(self.back_buffer.as_ptr(), self.width * self.height);
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            0x8 => self.backspace(),
            b'\n' => self.new_line(),
            b'\r' => self.text_cursor_x = 0,
            byte => {
                if self.text_cursor_x >= BUFFER_WIDTH - 1 {
                    self.new_line();
                }

                let row = self.text_cursor_y;
                let col = self.text_cursor_x;

                // let color_code = self.color_code;
                self.text_buf[row][col] = byte;
                self.move_right();
            }
        }
        self.cursor_color_hook();
    }

    fn cursor_color_hook(&mut self) {
        // let cursor = self.buffer.chars[self.text_cursor_y][self.text_cursor_x].read();
        // for y in 0..BUFFER_HEIGHT {
        //     for x in 0..BUFFER_WIDTH {
        //         let chr = self.buffer.chars[y][x].read();
        //         if y == self.text_cursor_y && x == self.text_cursor_x {
        //             self.buffer.chars[y][x].write(ScreenChar { ascii_character: cursor.ascii_character, color_code: ColorCode::new(Color::White, Color::Cyan) });
        //         } else {
        //             self.buffer.chars[y][x].write(ScreenChar { ascii_character: chr.ascii_character, color_code: self.color_code });
        //         }
        //     }
        // }
    }

    pub fn backspace(&mut self) {
        let row = self.text_cursor_y;
        let col = self.text_cursor_x.saturating_sub(1);
        self.text_buf[row][col] = b' ';
        self.text_cursor_x = col;
        self.cursor_color_hook();
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte)
        }
    }

    fn new_line(&mut self) {
        if self.text_cursor_y >= BUFFER_HEIGHT - 1 {
            for row in 1..BUFFER_HEIGHT {
                for col in 0..BUFFER_WIDTH {
                    let character = self.text_buf[row][col];
                    self.text_buf[row - 1][col] = character;
                }
            }
            self.text_cursor_y = BUFFER_HEIGHT - 1;
            self.clear_row(self.text_cursor_y);
            self.text_cursor_x = 0;
        } else {
            self.text_cursor_y += 1;
            self.text_cursor_x = 0;
        }
        self.cursor_color_hook();
    }

    fn clear_row(&mut self, row: usize) {
        for col in 0..BUFFER_WIDTH {
            self.text_buf[row][col] = b' ';
        }
        self.cursor_color_hook();
    }
    fn clear_until_end(&mut self) {
        for col in self.text_cursor_x..BUFFER_WIDTH {
            self.text_buf[self.text_cursor_y][col]= b' ';
        }
        for row in self.text_cursor_y + 1..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.cursor_color_hook();
    }
    fn clear_until_beginning(&mut self) {
        for col in 0..self.text_cursor_x {
            self.text_buf[self.text_cursor_y][col] = b' ';
        }
        for row in 0..self.text_cursor_y - 1 {
            self.clear_row(row);
        }
        self.cursor_color_hook();
    }
    fn clear_until_eol(&mut self) {
        for col in self.text_cursor_x..BUFFER_WIDTH {
            self.text_buf[self.text_cursor_y][col] = b' ';
        }
        self.cursor_color_hook();
    }
    fn clear_from_bol(&mut self) {
        for col in 0..self.text_cursor_x {
            self.text_buf[self.text_cursor_y][col] = b' ';
        }
        self.cursor_color_hook();
    }
    fn clear_line(&mut self) {
        self.clear_row(self.text_cursor_y);
    }
    fn clear_all(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row)
        }
        self.cursor_color_hook();
    }
    fn move_up(&mut self) {
        let new_y = self.text_cursor_y.saturating_sub(1);
        let mut new_x = self.text_cursor_x;
        while new_x > 0 && self.text_buf[new_y][new_x] == b' ' {
            new_x -= 1;
        }
        self.text_cursor_y = new_y;
        self.text_cursor_x = new_x;
        self.cursor_color_hook();
    }
    fn move_down(&mut self) {
        let new_y = self.text_cursor_y.add(1).min(BUFFER_HEIGHT - 1);
        let mut new_x = self.text_cursor_x;
        while new_x > 0 && self.text_buf[new_y][new_x] == b' ' {
            new_x -= 1;
        }
        self.text_cursor_y = new_y;
        self.text_cursor_x = new_x;
        self.cursor_color_hook();
    }
    fn move_left(&mut self) {
        self.text_cursor_x = self.text_cursor_x.saturating_sub(1);
        self.cursor_color_hook();
    }
    fn move_right(&mut self) {
        self.text_cursor_x = self.text_cursor_x.add(1).min(BUFFER_WIDTH - 1);
        self.cursor_color_hook();
    }
}

impl core::fmt::Write for FrameBuffer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

impl DrawTarget for FrameBuffer {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>> {
        assert_eq!(self.bpp, 32);
        for Pixel(coord, color) in pixels.into_iter() {
            let (x, y) = coord.into();
            if (0..self.width as i32).contains(&x) && (0..self.height as i32).contains(&y) {
                let index: usize = x as usize + y as usize * self.width;
                self.back_buffer[index] = color.into_storage();
            }
        }

        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        unsafe {
            core::slice::from_raw_parts_mut(self.back_buffer.as_mut_ptr(), self.width * self.height).fill(color.into_storage());
        }
        Ok(())
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> embedded_graphics::prelude::Size {
        Size::new(self.width as u32, self.height as u32)
    }
}

pub static FRAMEBUFFER: Once<IrqMutex<FrameBuffer>> = Once::new();

pub fn fb<'a>() -> IrqMutexGuard<'a, FrameBuffer> {
    FRAMEBUFFER.get().unwrap().lock()
}

pub fn clear_screen() {
    fb().clear_all()
}

pub fn backspace() {
    fb().backspace()
}

pub fn set_cursor_x(x: usize) {
    fb().text_cursor_x = x.min(BUFFER_WIDTH - 1);
}

pub fn set_cursor_y(y: usize) {
    fb().text_cursor_y = y.min(BUFFER_HEIGHT - 1);
}

pub fn set_cursor_xy(xy: (usize, usize)) {
    set_cursor_x(xy.0.min(BUFFER_WIDTH - 1));
    set_cursor_y(xy.1.min(BUFFER_HEIGHT - 1));
}

pub fn cursor_xy() -> (usize, usize) {
    let fb = fb();
    (fb.text_cursor_x, fb.text_cursor_y)
}

pub fn write_byte(byte: u8) {
    fb().write_byte(byte);
}

pub fn clear_until_end() {
    fb().clear_until_end();
}

pub fn clear_until_beginning() {
    fb().clear_until_beginning();
}

pub fn clear_from_bol() {
    fb().clear_from_bol();
}

pub fn clear_until_eol() {
    fb().clear_until_eol();
}

pub fn clear_line() {
    fb().clear_line();
}

pub fn move_up() {
    fb().move_up();
}

pub fn move_down() {
    fb().move_down();
}

pub fn move_left() {
    fb().move_left();
}

pub fn move_right() {
    fb().move_right();
}

pub fn render_text_buf() {
    fb().clear_pixels();
    fb().render_text_buf();
    fb().flip();
}


#[macro_export]
macro_rules! fb_print {
    ($($arg:tt)*) => ($crate::graphics::_fb_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! fb_println {
    () => ($crate::vga_print!("\n"));
    ($($arg:tt)*) => ($crate::fb_print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _fb_print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        FRAMEBUFFER.get().unwrap().lock().write_fmt(args).unwrap();
    });
}

pub fn init(fb_tag: &FramebufferTag) -> KResult<()> {
    assert!(matches!(fb_tag.buffer_type, multiboot2::FramebufferType::RGB { .. }));
    let framebuf = FrameBuffer {
        back_buffer: alloc::vec![0u32; fb_tag.width as usize * fb_tag.height as usize].into_boxed_slice(),
        start_addr: PhysAddr::new(fb_tag.address as usize).as_hhdm_virt(),
        width: fb_tag.width as usize,
        height: fb_tag.height as usize,
        bpp: fb_tag.bpp as usize,
        text_buf: [[b' '; BUFFER_WIDTH]; BUFFER_HEIGHT],
        text_cursor_x: 0,
        text_cursor_y: 0,
        text_fgcolor: Rgb888::GREEN,
    };

    FRAMEBUFFER.call_once(|| IrqMutex::new(framebuf));

    fb().clear_pixels();
    clear_screen();
    Ok(())
}