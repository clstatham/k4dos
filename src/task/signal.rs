use bitvec::prelude::*;

use crate::{
    errno,
    util::{ctypes::c_int, errno::Errno, error::KResult},
};

use super::get_scheduler;

pub type Signal = c_int;
#[allow(unused)]
pub const SIGHUP: Signal = 1;
#[allow(unused)]
pub const SIGINT: Signal = 2;
#[allow(unused)]
pub const SIGQUIT: Signal = 3;
#[allow(unused)]
pub const SIGILL: Signal = 4;
#[allow(unused)]
pub const SIGTRAP: Signal = 5;
#[allow(unused)]
pub const SIGABRT: Signal = 6;
#[allow(unused)]
pub const SIGBUS: Signal = 7;
#[allow(unused)]
pub const SIGFPE: Signal = 8;
#[allow(unused)]
pub const SIGKILL: Signal = 9;
#[allow(unused)]
pub const SIGUSR1: Signal = 10;
#[allow(unused)]
pub const SIGSEGV: Signal = 11;
#[allow(unused)]
pub const SIGUSR2: Signal = 12;
#[allow(unused)]
pub const SIGPIPE: Signal = 13;
#[allow(unused)]
pub const SIGALRM: Signal = 14;
#[allow(unused)]
pub const SIGTERM: Signal = 15;
#[allow(unused)]
pub const SIGSTKFLT: Signal = 16;
#[allow(unused)]
pub const SIGCHLD: Signal = 17;
#[allow(unused)]
pub const SIGCONT: Signal = 18;
#[allow(unused)]
pub const SIGSTOP: Signal = 19;
#[allow(unused)]
pub const SIGTSTP: Signal = 20;
#[allow(unused)]
pub const SIGTTIN: Signal = 21;
#[allow(unused)]
pub const SIGTTOU: Signal = 22;
#[allow(unused)]
pub const SIGURG: Signal = 23;
#[allow(unused)]
pub const SIGXCPU: Signal = 24;
#[allow(unused)]
pub const SIGXFSZ: Signal = 25;
#[allow(unused)]
pub const SIGVTALRM: Signal = 26;
#[allow(unused)]
pub const SIGPROF: Signal = 27;
#[allow(unused)]
pub const SIGWINCH: Signal = 28;
#[allow(unused)]
pub const SIGIO: Signal = 29;
#[allow(unused)]
pub const SIGPWR: Signal = 30;
#[allow(unused)]
pub const SIGSYS: Signal = 31;

const SIGMAX: c_int = 32;

pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SigAction {
    Ignore,
    Handler { handler: fn() },
}

// TODO: Fill correct default actions
pub const DEFAULT_ACTIONS: [SigAction; SIGMAX as usize] = [
    /* (unused) */ SigAction::Ignore,
    /* SIGHUP */ SigAction::Handler { handler: terminate },
    /* SIGINT */ SigAction::Handler { handler: terminate },
    /* SIGQUIT */ SigAction::Handler { handler: terminate },
    /* SIGILL */ SigAction::Handler { handler: terminate },
    /* SIGTRAP */ SigAction::Ignore,
    /* SIGABRT */ SigAction::Handler { handler: terminate },
    /* SIGBUS */ SigAction::Handler { handler: terminate },
    /* SIGFPE */ SigAction::Handler { handler: terminate },
    /* SIGKILL */ SigAction::Handler { handler: terminate },
    /* SIGUSR1 */ SigAction::Ignore,
    /* SIGSEGV */ SigAction::Handler { handler: terminate },
    /* SIGUSR2 */ SigAction::Ignore,
    /* SIGPIPE */ SigAction::Handler { handler: terminate },
    /* SIGALRM */ SigAction::Ignore,
    /* SIGTERM */ SigAction::Handler { handler: terminate },
    /* SIGSTKFLT */ SigAction::Ignore,
    /* SIGCHLD */ SigAction::Ignore,
    /* SIGCONT */ SigAction::Handler { handler: terminate },
    /* SIGSTOP */ SigAction::Ignore,
    /* SIGTSTP */ SigAction::Ignore,
    /* SIGTTIN */ SigAction::Ignore,
    /* SIGTTOU */ SigAction::Ignore,
    /* SIGURG */ SigAction::Ignore,
    /* SIGXCPU */ SigAction::Ignore,
    /* SIGXFSZ */ SigAction::Ignore,
    /* SIGVTALRM */ SigAction::Ignore,
    /* SIGPROF */ SigAction::Ignore,
    /* SIGWINCH */ SigAction::Ignore,
    /* SIGIO */ SigAction::Ignore,
    /* SIGPWR */ SigAction::Ignore,
    /* SIGSYS */ SigAction::Ignore,
];

pub fn terminate() {
    log::warn!("Terminating current process");
    get_scheduler().exit_current(1);
}

#[derive(Clone)]
pub struct SignalDelivery {
    pending: u32,
    actions: [SigAction; SIGMAX as usize],
}

impl Default for SignalDelivery {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalDelivery {
    pub fn new() -> SignalDelivery {
        SignalDelivery {
            pending: 0,
            actions: DEFAULT_ACTIONS,
        }
    }

    pub fn get_action(&self, signal: Signal) -> SigAction {
        self.actions[signal as usize]
    }

    pub fn set_action(&mut self, signal: Signal, action: SigAction) -> KResult<()> {
        if signal > SIGMAX {
            return Err(errno!(Errno::EINVAL));
        }

        self.actions[signal as usize] = action;
        Ok(())
    }

    pub fn is_pending(&self) -> bool {
        self.pending != 0
    }

    pub fn signal(&mut self, signal: Signal) {
        self.pending |= 1 << signal
    }

    pub fn pop_pending(&mut self) -> Option<(Signal, SigAction)> {
        if self.pending == 0 {
            return None;
        }

        let signal = self.pending.trailing_zeros();
        self.pending &= !(1 << signal);
        Some((signal as Signal, self.actions[signal as usize]))
    }
}

pub type SigSet = BitArray<[u8; 1024 / 8], LocalBits>;
pub enum SignalMask {
    Block,
    Unblock,
    Set,
}
