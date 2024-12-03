use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    errno,
    fs::{File, FsNode},
    mem::addr::VirtAddr,
    util::{errno::Errno, KError, KResult},
};

pub fn init() {}

#[repr(C)]
#[non_exhaustive]
pub enum Domain {
    Unix = 0,
    Inet = 2,
}

impl TryFrom<usize> for Domain {
    type Error = KError<'static>;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Unix),
            2 => Ok(Self::Inet),
            _ => Err(errno!(Errno::EINVAL, "invalid socket domain")),
        }
    }
}

#[repr(C)]
#[non_exhaustive]
pub enum SocketType {
    Stream = 1,
    Datagram = 2,
    Raw = 3,
    SeqPacket = 5,
    Packet = 10,
}

impl TryFrom<usize> for SocketType {
    type Error = KError<'static>;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Stream),
            2 => Ok(Self::Datagram),
            3 => Ok(Self::Raw),
            5 => Ok(Self::SeqPacket),
            10 => Ok(Self::Packet),
            _ => Err(errno!(Errno::EINVAL, "invalid socket type")),
        }
    }
}

#[repr(C)]
#[non_exhaustive]
pub enum Protocol {
    Ipv4 = 4,
    Tcp = 6,
    Udp = 17,
}

impl TryFrom<usize> for Protocol {
    type Error = KError<'static>;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            4 => Ok(Self::Ipv4),
            6 => Ok(Self::Tcp),
            17 => Ok(Self::Udp),
            _ => Err(errno!(Errno::EINVAL, "invalid socket protocol")),
        }
    }
}

pub struct Socket {
    pub id: usize,
    // pub handle: SocketHandle,
    pub domain: Domain,
    pub typ: SocketType,
    pub protocol: Protocol,
}

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

impl Socket {
    pub fn alloc_id() -> usize {
        NEXT_ID.fetch_add(1, Ordering::SeqCst)
    }

    // pub fn new() -> Self {
    //     Self {
    //         id: Self::alloc_id(),
    //         handle: (),
    //         domain: (),
    //         typ: (),
    //         protocol: (),
    //     }
    // }
}

impl FsNode for Socket {
    fn get_name(&self) -> alloc::string::String {
        alloc::format!("socket{}", self.id)
    }
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct SockAddrInet {
    family: u16,
    port: [u8; 2],
    addr: [u8; 4],
    zero: [u8; 8],
}

pub fn read_sockaddr(addr: VirtAddr, len: usize) -> KResult<SockAddrInet> {
    let family = unsafe { addr.read_volatile::<u16>()? };
    let sockaddr = match Domain::try_from(family as usize)? {
        Domain::Inet => {
            if len < core::mem::size_of::<SockAddrInet>() {
                return Err(errno!(Errno::EINVAL, "read_sockaddr(): buffer overflow"));
            }

            unsafe { addr.read_volatile::<SockAddrInet>()? }
        }
        Domain::Unix => {
            todo!()
        }
    };
    Ok(sockaddr)
}

pub fn write_sockaddr(
    sockaddr: SockAddrInet,
    dst: Option<VirtAddr>,
    socklen: Option<VirtAddr>,
) -> KResult<()> {
    if let Some(dst) = dst {
        dst.write_volatile(sockaddr)?;
    }
    if let Some(socklen) = socklen {
        socklen.write_volatile(core::mem::size_of::<SockAddrInet>() as u32)?;
    }
    Ok(())
}

impl File for Socket {
    fn ioctl(&self, cmd: usize, _arg: usize) -> KResult<isize> {
        const FIONBIO: usize = 0x5421;
        match cmd {
            FIONBIO => {
                // todo: set/clear non block flag
            }
            _ => return Err(errno!(Errno::EINVAL, "ioctl(): unknown cmd for socket")),
        }
        Ok(0)
    }
}
