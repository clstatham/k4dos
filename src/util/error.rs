use core::fmt::{Debug, Display};

use super::errno::Errno;

pub type KResult<T> = Result<T, KError<'static>>;

#[derive(Clone, Default, Debug)]
pub struct KError<'a> {
    pub(crate) msg: Option<&'a str>,
    pub(crate) errno: Option<Errno>,
}

impl<'a> KError<'a> {
    pub fn msg(&self) -> Option<&'a str> {
        self.msg
    }

    pub fn errno(&self) -> Option<Errno> {
        self.errno
    }
}

impl Display for KError<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (self.errno, self.msg) {
            (Some(errno), Some(msg)) => write!(f, "{:?}: {}", errno, msg),
            (Some(errno), None) => write!(f, "{:?}", errno),
            (None, Some(msg)) => write!(f, "{}", msg),
            (None, None) => write!(f, "Unknown error"),
        }
    }
}

#[macro_export]
macro_rules! kerror {
    ($e:ident) => {
        $crate::util::error::KError {
            errno: Some($crate::util::errno::Errno::$e),
            msg: None,
        }
    };
    ($e:ident, $($tts:tt)*) => {
        $crate::util::error::KError {
            errno: Some($crate::util::errno::Errno::$e),
            msg: format_args!($($tts)*).as_str(),
        }
    };
    ($($tts:tt)*) => {
        $crate::util::error::KError {
            errno: None,
            msg: format_args!($($tts)*).as_str(),
        }
    };
}

#[macro_export]
macro_rules! kbail {
    ($($e:tt)*) => {
        return Err($crate::kerror!($($e)*))
    };
}
