pub const ITIMER_REAL: usize = 0;
pub const ITIMER_VIRTUAL: usize = 1;
pub const ITIMER_PROF: usize = 2;

#[derive(Default, PartialEq)]
#[repr(C)]
pub struct TimeVal {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[derive(Default, PartialEq)]
#[repr(C)]
pub struct ITimerVal {
    pub it_interval: TimeVal,
    pub it_value: TimeVal,
}

#[derive(Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct TimeSpec {
    pub tv_sec: isize,
    pub tv_nsec: isize,
}

impl TimeSpec {
    pub const fn zero() -> Self {
        TimeSpec {
            tv_sec: 0,
            tv_nsec: 0,
        }
    }
}
