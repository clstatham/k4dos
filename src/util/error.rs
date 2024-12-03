use core::fmt::{Debug, Display};

use super::errno::Errno;

pub type KResult<T> = Result<T, KError<'static>>;

#[derive(Clone)]
pub enum KError<'a> {
    Message { msg: &'a str },
    Errno { errno: Errno, msg: Option<&'a str> },
}

impl<'a> KError<'a> {
    pub fn msg(&self) -> Option<&'a str> {
        match self {
            KError::Message { msg } => Some(msg),
            KError::Errno { msg, .. } => *msg,
        }
    }

    pub fn errno(&self) -> Option<Errno> {
        match self {
            KError::Errno { errno, .. } => Some(*errno),
            _ => None,
        }
    }
}

impl Debug for KError<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            // KError::Error { err } => write!(f, "{:?}", err),
            // KError::ErrorWithMessage { err, msg } => write!(f, "{:?}: {}", err, msg),
            KError::Message { msg } => write!(f, "{}", msg),
            KError::Errno { errno, msg } => match msg {
                Some(msg) => write!(f, "{:?}: {}", errno, msg),
                None => write!(f, "{:?}", errno),
            },
        }
    }
}

impl Display for KError<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[macro_export]
macro_rules! kerrmsg {
    ($s:expr) => {
        $crate::util::error::KError::Message { msg: $s }
    };
}

#[macro_export]
macro_rules! errno {
    ($e:expr) => {
        $crate::util::error::KError::Errno {
            errno: $e,
            msg: None,
        }
    };
    ($e:expr, $msg:expr) => {
        $crate::util::error::KError::Errno {
            errno: $e,
            msg: Some($msg),
        }
    };
}
