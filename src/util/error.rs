use core::fmt::{Debug, Display};

use super::errno::Errno;

pub type KResult<T> = Result<T, KError>;

pub enum KError {
    Message { msg: &'static str },
    Errno { errno: Errno },
}

impl KError {
    pub fn msg(&self) -> Option<&'static str> {
        match self {
            // KError::Error { .. } => None,
            // KError::ErrorWithMessage { msg, .. } => Some(msg),
            KError::Message { msg } => Some(msg),
            KError::Errno { errno: _ } => None,
        }
    }

    pub fn errno(&self) -> Option<Errno> {
        match self {
            KError::Errno { errno } => Some(*errno),
            _ => None,
        }
    }
}

impl Debug for KError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            // KError::Error { err } => write!(f, "{:?}", err),
            // KError::ErrorWithMessage { err, msg } => write!(f, "{:?}: {}", err, msg),
            KError::Message { msg } => write!(f, "{}", msg),
            KError::Errno { errno } => write!(f, "{:?}", errno),
        }
    }
}

impl Display for KError {
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
        $crate::util::error::KError::Errno { errno: $e }
    };
}
