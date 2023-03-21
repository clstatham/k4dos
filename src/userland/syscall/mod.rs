use alloc::sync::Arc;

use crate::{
    errno,
    fs::{
        opened_file::{FileDesc, OpenFlags},
        path::{Path, PathBuf}, FileMode,
    },
    mem::addr::VirtAddr,
    task::{current_task, get_scheduler, Task, TaskId, vmem::{MMapProt, MMapFlags}},
    userland::syscall::wait4::WaitOptions,
    util::{ctypes::{c_int, c_nfds}, errno::Errno, KResult},
};

use super::buffer::UserCStr;

pub mod arch_prctl;
pub mod execve;
pub mod exit;
pub mod fork;
pub mod ioctl;
pub mod read;
pub mod rt_sigprocmask;
pub mod set_tid_address;
pub mod wait4;
pub mod write;
pub mod writev;
pub mod mmap;
pub mod stat;
pub mod open;
pub mod fcntl;
pub mod getcwd;
pub mod uname;
pub mod poll;

#[repr(packed)]
#[derive(Default, Clone, Debug)]
pub struct SyscallFrame {
    // preserved registers
    pub r15: usize,
    pub r14: usize,
    pub r13: usize,
    pub r12: usize,
    pub rbp: usize,
    pub rbx: usize,

    // scratch registers
    pub r11: usize,
    pub r10: usize,
    pub r9: usize,
    pub r8: usize,
    pub rsi: usize,
    pub rdi: usize,
    pub rdx: usize,
    pub rcx: usize,
    pub rax: usize,
    // // iret regs
    pub rip: usize,
    pub cs: usize,
    pub rflags: usize,
    pub rsp: usize,
    pub ss: usize,
}

pub struct SyscallHandler<'a> {
    pub frame: &'a mut SyscallFrame,
    // pub frame: &'a u8,
}

impl<'a> SyscallHandler<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch(
        &mut self,
        a1: usize,
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        n: usize,
    ) -> KResult<isize> {
        // {
        log::trace!(
            "[{}] SYSCALL #{} {}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x})",
            current_task().pid().as_usize(),
            n,
            syscall_name_by_number(n),
            a1,
            a2,
            a3,
            a4,
            a5,
            a6
        );

        let res = match n {
            SYS_ARCH_PRCTL => self.sys_arch_prctl(a1 as i32, VirtAddr::new(a2)),
            SYS_SET_TID_ADDRESS => self.sys_set_tid_address(VirtAddr::new(a1)),
            SYS_WRITE => self.sys_write(a1 as FileDesc, VirtAddr::new(a2), a3),
            SYS_WRITEV => self.sys_writev(a1 as FileDesc, VirtAddr::new(a2), a3),
            SYS_READ => self.sys_read(a1 as FileDesc, VirtAddr::new(a2), a3),
            SYS_IOCTL => self.sys_ioctl(a1 as FileDesc, a2, a3),
            SYS_RT_SIGPROCMASK => {
                self.sys_rt_sigprocmask(a1, VirtAddr::new(a2), VirtAddr::new(a3), a4)
            }
            SYS_FORK => self.sys_fork(),
            SYS_WAIT4 => self.sys_wait4(
                TaskId::new(a1),
                VirtAddr::new(a2),
                crate::bitflags_from_user!(WaitOptions, a3 as i32),
                VirtAddr::new(a4),
            ),
            SYS_EXECVE => self.sys_execve(&resolve_path(a1)?, VirtAddr::new(a2), VirtAddr::new(a3)),
            SYS_GETTID => self.sys_getpid(), // todo
            SYS_GETPID => self.sys_getpid(),
            SYS_GETPPID => self.sys_getppid(),
            SYS_EXIT => self.sys_exit(a1 as c_int),
            SYS_MMAP => self.sys_mmap(VirtAddr::new(a1), a2, crate::bitflags_from_user!(MMapProt, a3 as u64), crate::bitflags_from_user!(MMapFlags, a4 as u64), a5 as FileDesc, a6),
            SYS_MPROTECT => self.sys_mprotect(VirtAddr::new(a1), a2, crate::bitflags_from_user!(MMapProt, a3 as u64)),
            SYS_MUNMAP => self.sys_munmap(VirtAddr::new(a1), a2),
            SYS_RT_SIGACTION => self.sys_rt_sigaction(a1 as c_int, VirtAddr::new(a2), VirtAddr::new(a3)),
            SYS_GETUID => Ok(0),    // TODO:
            SYS_GETEUID => Ok(0),   // TODO:
            SYS_SETUID => Ok(0),    // TODO:
            SYS_SETGID => Ok(0),    // TODO:
            SYS_SETGROUPS => Ok(0), // TODO:
            SYS_STAT => self.sys_stat(&resolve_path(a1)?, VirtAddr::new(a2)),
            SYS_FSTAT => self.sys_fstat(a1 as FileDesc, VirtAddr::new(a2)),
            SYS_OPEN => self.sys_open(&resolve_path(a1)?, crate::bitflags_from_user!(OpenFlags, a2 as i32), FileMode::new(a3 as u32)),
            SYS_GETCWD => self.sys_getcwd(VirtAddr::new(a1), a2 as u64),
            SYS_GETDENTS64 => self.sys_getdents64(a1 as FileDesc, VirtAddr::new(a2), a3),
            SYS_FCNTL => self.sys_fcntl(a1 as FileDesc, a2 as c_int, a3),
            SYS_UNAME => self.sys_uname(VirtAddr::new(a1)),
            SYS_CLOSE => self.sys_close(a1 as FileDesc),
            SYS_POLL => self.sys_poll(VirtAddr::new(a1), a2 as c_nfds, a3 as c_int),
            SYS_CHDIR => self.sys_chdir(&resolve_path(a1)?),
            _ => Err(errno!(Errno::ENOSYS)),
        };
        // }
        if let Err(err) = get_scheduler().try_delivering_signal(self.frame) {
            log::error!("Failed to send signal: {:?}", err);
        }
        res
    }
}

#[macro_export]
macro_rules! bitflags_from_user {
    ($st:tt, $input:expr) => {{
        let bits = $input;
        $st::from_bits(bits).unwrap_or_else(|| {
            log::warn!(
                concat!("unsupported bitflags for ", stringify!($st), ": {:x}"),
                bits
            );
            $st::from_bits_truncate(bits)

            // $crate::errno!($crate::util::errno::Errno::ENOSYS)
        })
    }};
}

fn resolve_path(uaddr: usize) -> KResult<PathBuf> {
    Ok(Path::new(UserCStr::new(VirtAddr::new(uaddr), 512)?.as_str()).into())
}

fn syscall_name_by_number(n: usize) -> &'static str {
    match n {
        0 => "read",
        1 => "write",
        2 => "open",
        3 => "close",
        4 => "stat",
        5 => "fstat",
        6 => "lstat",
        7 => "poll",
        8 => "lseek",
        9 => "mmap",
        10 => "mprotect",
        11 => "munmap",
        12 => "brk",
        13 => "rt_sigaction",
        14 => "rt_sigprocmask",
        15 => "rt_sigreturn",
        16 => "ioctl",
        17 => "pread64",
        18 => "pwrite64",
        19 => "readv",
        20 => "writev",
        21 => "access",
        22 => "pipe",
        23 => "select",
        24 => "sched_yield",
        25 => "mremap",
        26 => "msync",
        27 => "mincore",
        28 => "madvise",
        29 => "shmget",
        30 => "shmat",
        31 => "shmctl",
        32 => "dup",
        33 => "dup2",
        34 => "pause",
        35 => "nanosleep",
        36 => "getitimer",
        37 => "alarm",
        38 => "setitimer",
        39 => "getpid",
        40 => "sendfile",
        41 => "socket",
        42 => "connect",
        43 => "accept",
        44 => "sendto",
        45 => "recvfrom",
        46 => "sendmsg",
        47 => "recvmsg",
        48 => "shutdown",
        49 => "bind",
        50 => "listen",
        51 => "getsockname",
        52 => "getpeername",
        53 => "socketpair",
        54 => "setsockopt",
        55 => "getsockopt",
        56 => "clone",
        57 => "fork",
        58 => "vfork",
        59 => "execve",
        60 => "exit",
        61 => "wait4",
        62 => "kill",
        63 => "uname",
        64 => "semget",
        65 => "semop",
        66 => "semctl",
        67 => "shmdt",
        68 => "msgget",
        69 => "msgsnd",
        70 => "msgrcv",
        71 => "msgctl",
        72 => "fcntl",
        73 => "flock",
        74 => "fsync",
        75 => "fdatasync",
        76 => "truncate",
        77 => "ftruncate",
        78 => "getdents",
        79 => "getcwd",
        80 => "chdir",
        81 => "fchdir",
        82 => "rename",
        83 => "mkdir",
        84 => "rmdir",
        85 => "creat",
        86 => "link",
        87 => "unlink",
        88 => "symlink",
        89 => "readlink",
        90 => "chmod",
        91 => "fchmod",
        92 => "chown",
        93 => "fchown",
        94 => "lchown",
        95 => "umask",
        96 => "gettimeofday",
        97 => "getrlimit",
        98 => "getrusage",
        99 => "sysinfo",
        100 => "times",
        101 => "ptrace",
        102 => "getuid",
        103 => "syslog",
        104 => "getgid",
        105 => "setuid",
        106 => "setgid",
        107 => "geteuid",
        108 => "getegid",
        109 => "setpgid",
        110 => "getppid",
        111 => "getpgrp",
        112 => "setsid",
        113 => "setreuid",
        114 => "setregid",
        115 => "getgroups",
        116 => "setgroups",
        117 => "setresuid",
        118 => "getresuid",
        119 => "setresgid",
        120 => "getresgid",
        121 => "getpgid",
        122 => "setfsuid",
        123 => "setfsgid",
        124 => "getsid",
        125 => "capget",
        126 => "capset",
        127 => "rt_sigpending",
        128 => "rt_sigtimedwait",
        129 => "rt_sigqueueinfo",
        130 => "rt_sigsuspend",
        131 => "sigaltstack",
        132 => "utime",
        133 => "mknod",
        134 => "uselib",
        135 => "personality",
        136 => "ustat",
        137 => "statfs",
        138 => "fstatfs",
        139 => "sysfs",
        140 => "getpriority",
        141 => "setpriority",
        142 => "sched_setparam",
        143 => "sched_getparam",
        144 => "sched_setscheduler",
        145 => "sched_getscheduler",
        146 => "sched_get_priority_max",
        147 => "sched_get_priority_min",
        148 => "sched_rr_get_interval",
        149 => "mlock",
        150 => "munlock",
        151 => "mlockall",
        152 => "munlockall",
        153 => "vhangup",
        154 => "modify_ldt",
        155 => "pivot_root",
        156 => "_sysctl",
        157 => "prctl",
        158 => "arch_prctl",
        159 => "adjtimex",
        160 => "setrlimit",
        161 => "chroot",
        162 => "sync",
        163 => "acct",
        164 => "settimeofday",
        165 => "mount",
        166 => "umount2",
        167 => "swapon",
        168 => "swapoff",
        169 => "reboot",
        170 => "sethostname",
        171 => "setdomainname",
        172 => "iopl",
        173 => "ioperm",
        174 => "create_module",
        175 => "init_module",
        176 => "delete_module",
        177 => "get_kernel_syms",
        178 => "query_module",
        179 => "quotactl",
        180 => "nfsservctl",
        181 => "getpmsg",
        182 => "putpmsg",
        183 => "afs_syscall",
        184 => "tuxcall",
        185 => "security",
        186 => "gettid",
        187 => "readahead",
        188 => "setxattr",
        189 => "lsetxattr",
        190 => "fsetxattr",
        191 => "getxattr",
        192 => "lgetxattr",
        193 => "fgetxattr",
        194 => "listxattr",
        195 => "llistxattr",
        196 => "flistxattr",
        197 => "removexattr",
        198 => "lremovexattr",
        199 => "fremovexattr",
        200 => "tkill",
        201 => "time",
        202 => "futex",
        203 => "sched_setaffinity",
        204 => "sched_getaffinity",
        205 => "set_thread_area",
        206 => "io_setup",
        207 => "io_destroy",
        208 => "io_getevents",
        209 => "io_submit",
        210 => "io_cancel",
        211 => "get_thread_area",
        212 => "lookup_dcookie",
        213 => "epoll_create",
        214 => "epoll_ctl_old",
        215 => "epoll_wait_old",
        216 => "remap_file_pages",
        217 => "getdents64",
        218 => "set_tid_address",
        219 => "restart_syscall",
        220 => "semtimedop",
        221 => "fadvise64",
        222 => "timer_create",
        223 => "timer_settime",
        224 => "timer_gettime",
        225 => "timer_getoverrun",
        226 => "timer_delete",
        227 => "clock_settime",
        228 => "clock_gettime",
        229 => "clock_getres",
        230 => "clock_nanosleep",
        231 => "exit_group",
        232 => "epoll_wait",
        233 => "epoll_ctl",
        234 => "tgkill",
        235 => "utimes",
        236 => "vserver",
        237 => "mbind",
        238 => "set_mempolicy",
        239 => "get_mempolicy",
        240 => "mq_open",
        241 => "mq_unlink",
        242 => "mq_timedsend",
        243 => "mq_timedreceive",
        244 => "mq_notify",
        245 => "mq_getsetattr",
        246 => "kexec_load",
        247 => "waitid",
        248 => "add_key",
        249 => "request_key",
        250 => "keyctl",
        251 => "ioprio_set",
        252 => "ioprio_get",
        253 => "inotify_init",
        254 => "inotify_add_watch",
        255 => "inotify_rm_watch",
        256 => "migrate_pages",
        257 => "openat",
        258 => "mkdirat",
        259 => "mknodat",
        260 => "fchownat",
        261 => "futimesat",
        262 => "newfstatat",
        263 => "unlinkat",
        264 => "renameat",
        265 => "linkat",
        266 => "symlinkat",
        267 => "readlinkat",
        268 => "fchmodat",
        269 => "faccessat",
        270 => "pselect6",
        271 => "ppoll",
        272 => "unshare",
        273 => "set_robust_list",
        274 => "get_robust_list",
        275 => "splice",
        276 => "tee",
        277 => "sync_file_range",
        278 => "vmsplice",
        279 => "move_pages",
        280 => "utimensat",
        281 => "epoll_pwait",
        282 => "signalfd",
        283 => "timerfd_create",
        284 => "eventfd",
        285 => "fallocate",
        286 => "timerfd_settime",
        287 => "timerfd_gettime",
        288 => "accept4",
        289 => "signalfd4",
        290 => "eventfd2",
        291 => "epoll_create1",
        292 => "dup3",
        293 => "pipe2",
        294 => "inotify_init1",
        295 => "preadv",
        296 => "pwritev",
        297 => "rt_tgsigqueueinfo",
        298 => "perf_event_open",
        299 => "recvmmsg",
        300 => "fanotify_init",
        301 => "fanotify_mark",
        302 => "prlimit64",
        303 => "name_to_handle_at",
        304 => "open_by_handle_at",
        305 => "clock_adjtime",
        306 => "syncfs",
        307 => "sendmmsg",
        308 => "setns",
        309 => "getcpu",
        310 => "process_vm_readv",
        311 => "process_vm_writev",
        312 => "kcmp",
        313 => "finit_module",
        314 => "sched_setattr",
        315 => "sched_getattr",
        316 => "renameat2",
        317 => "seccomp",
        318 => "getrandom",
        319 => "memfd_create",
        320 => "kexec_file_load",
        321 => "bpf",
        322 => "execveat",
        323 => "userfaultfd",
        324 => "membarrier",
        325 => "mlock2",
        326 => "copy_file_range",
        327 => "preadv2",
        328 => "pwritev2",
        329 => "pkey_mprotect",
        330 => "pkey_alloc",
        331 => "pkey_free",
        332 => "statx",
        333 => "io_pgetevents",
        334 => "rseq",

        51729 => "kados_debug",
        _ => "(unknown)",
    }
}

const SYS_READ: usize = 0;
const SYS_WRITE: usize = 1;
const SYS_OPEN: usize = 2;
const SYS_CLOSE: usize = 3;
const SYS_STAT: usize = 4;
const SYS_FSTAT: usize = 5;
const SYS_LSTAT: usize = 6;
const SYS_POLL: usize = 7;
const SYS_MMAP: usize = 9;
const SYS_MPROTECT: usize = 10;
const SYS_MUNMAP: usize = 11;
const SYS_BRK: usize = 12;
const SYS_RT_SIGACTION: usize = 13;
const SYS_RT_SIGPROCMASK: usize = 14;
const SYS_RT_SIGRETURN: usize = 15;
const SYS_IOCTL: usize = 16;
const SYS_WRITEV: usize = 20;
const SYS_PIPE: usize = 22;
const SYS_SELECT: usize = 23;
const SYS_DUP2: usize = 33;
const SYS_GETPID: usize = 39;
const SYS_SOCKET: usize = 41;
const SYS_CONNECT: usize = 42;
const SYS_ACCEPT: usize = 43;
const SYS_SENDTO: usize = 44;
const SYS_RECVFROM: usize = 45;
const SYS_SHUTDOWN: usize = 48;
const SYS_BIND: usize = 49;
const SYS_LISTEN: usize = 50;
const SYS_GETSOCKNAME: usize = 51;
const SYS_GETPEERNAME: usize = 52;
const SYS_GETSOCKOPT: usize = 55;
const SYS_CLONE: usize = 56;
const SYS_FORK: usize = 57;
const SYS_EXECVE: usize = 59;
const SYS_EXIT: usize = 60;
const SYS_WAIT4: usize = 61;
const SYS_KILL: usize = 62;
const SYS_UNAME: usize = 63;
const SYS_FCNTL: usize = 72;
const SYS_FSYNC: usize = 74;
const SYS_GETCWD: usize = 79;
const SYS_CHDIR: usize = 80;
const SYS_MKDIR: usize = 83;
const SYS_LINK: usize = 86;
const SYS_READLINK: usize = 89;
const SYS_CHMOD: usize = 90;
const SYS_CHOWN: usize = 92;
const SYS_GETUID: usize = 102;
const SYS_SYSLOG: usize = 103;
const SYS_SETUID: usize = 105;
const SYS_SETGID: usize = 106;
const SYS_GETEUID: usize = 107;
const SYS_SETPGID: usize = 109;
const SYS_GETPPID: usize = 110;
const SYS_GETPGRP: usize = 111;
const SYS_GETPGID: usize = 121;
const SYS_SETGROUPS: usize = 116;
const SYS_ARCH_PRCTL: usize = 158;
const SYS_REBOOT: usize = 169;
const SYS_GETTID: usize = 186;
const SYS_GETDENTS64: usize = 217;
const SYS_SET_TID_ADDRESS: usize = 218;
const SYS_CLOCK_GETTIME: usize = 228;
const SYS_EXIT_GROUP: usize = 231;
const SYS_UTIMES: usize = 235;
const SYS_LINKAT: usize = 265;
const SYS_GETRANDOM: usize = 318;
