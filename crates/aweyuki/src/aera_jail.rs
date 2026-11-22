//! Pure-Rust AERA sandbox launcher and parent IPC supervisors for
//! `aera-browser-worker` and `aera-telegram`.

use anyhow::{ensure, Context, Result};
use aweyuki_install::aerap::browser_proto::{self, BrowserKind, BrowserMsg, SHARED_BYTES};
use aweyuki_install::aerap::telegram_proto::{self, TgMsg};
use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{chown, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::ptr::null;
use std::thread;
use std::time::{Duration, Instant};

const UID_BROWSER: libc::uid_t = 99_090;
const UID_TELEGRAM: libc::uid_t = 99_091;
const GID_MEDIA_RW: libc::gid_t = 1023;
const GID_INET: libc::gid_t = 3003;

const AUDIT_ARCH_AARCH64: u32 = 0xC000_00B7;
const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_ALU: u16 = 0x04;
const BPF_AND: u16 = 0x50;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
const BPF_JGE: u16 = 0x30;
const BPF_K: u16 = 0x00;
const BPF_RET: u16 = 0x06;

const SYS_STATFS_NR: u32 = 43;
const SYS_FSTATFS_NR: u32 = 44;
const SYS_FTRUNCATE_NR: u32 = 46;
const SYS_NEWFSTATAT_NR: u32 = 79;
const SYS_FSTAT_NR: u32 = 80;
const SYS_FADVISE64_NR: u32 = 223;
const SYS_RSEQ_NR: u32 = 293;

const CAP_V3: u32 = 0x2008_0522;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JailKind {
    Browser,
    Telegram,
}

fn cstr(s: &str) -> Result<CString> {
    CString::new(s).with_context(|| format!("NUL byte in {s:?}"))
}

fn cpath(p: &Path) -> Result<CString> {
    cstr(&p.to_string_lossy())
}

fn check(rc: libc::c_int, what: &str) -> Result<()> {
    if rc < 0 {
        return Err(io::Error::last_os_error()).context(what.to_string());
    }
    Ok(())
}

fn set_cloexec(fd: RawFd, on: bool) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let next = if on { flags | libc::FD_CLOEXEC } else { flags & !libc::FD_CLOEXEC };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, next) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_nonblock(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn dup_high(fd: RawFd) -> Result<OwnedFd> {
    let duped = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if duped < 0 {
        return Err(io::Error::last_os_error()).context("F_DUPFD_CLOEXEC");
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duped) })
}

fn seqpacket_pair() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1i32; 2];
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr()) };
    check(rc, "socketpair SOCK_SEQPACKET")?;
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn send_packet(fd: RawFd, data: &[u8]) -> Result<()> {
    let rc = unsafe { libc::send(fd, data.as_ptr().cast(), data.len(), libc::MSG_NOSIGNAL) };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock {
            return Ok(());
        }
        return Err(err).context("send AERA IPC packet");
    }
    ensure!(rc as usize == data.len(), "short SEQPACKET write ({rc} != {})", data.len());
    Ok(())
}

fn recv_packets<const N: usize>(fd: RawFd) -> Result<Vec<[u8; N]>> {
    let mut out = Vec::new();
    let mut buf = [0u8; N];
    loop {
        let rc = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), N, 0) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if err.kind() == io::ErrorKind::WouldBlock {
                break;
            }
            return Err(err).context("recv AERA IPC packet");
        }
        if rc == 0 {
            break;
        }
        if rc as usize == N {
            out.push(buf);
        }
    }
    Ok(out)
}

fn reap_and_clean(child: &mut Option<Child>, sock: RawFd, runtime: &Path, close_pkt: &[u8]) {
    let _ = unsafe { libc::send(sock, close_pkt.as_ptr().cast(), close_pkt.len(), libc::MSG_NOSIGNAL) };
    if let Some(mut c) = child.take() {
        let deadline = Instant::now() + Duration::from_millis(450);
        let mut exited = matches!(c.try_wait(), Ok(Some(_)));
        if !exited {
            let _ = unsafe { libc::kill(c.id() as libc::pid_t, libc::SIGTERM) };
            while Instant::now() < deadline {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    exited = true;
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        if !exited {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    let _ = fs::remove_dir_all(runtime);
}

/// Parent-side handle for an isolated AERA Browser worker (`--isolated-ipc-v1`).
pub struct BrowserProcess {
    child: Option<Child>,
    sock: OwnedFd,
    shared: *const u8,
    runtime: PathBuf,
}

impl BrowserProcess {
    /// Start an isolated WPE WebKit worker from unpacked `runtime` directory.
    ///
    /// # Errors
    ///
    /// Returns an error if creating `memfd`, `socketpair`, or spawning fails.
    pub fn start(runtime: PathBuf, dns: &[std::net::Ipv4Addr]) -> Result<Self> {
        let name = cstr("aera-browser-pixels")?;
        let memfd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
        if memfd < 0 {
            return Err(io::Error::last_os_error()).context("memfd_create aera-browser-pixels");
        }
        let frame_fd = unsafe { OwnedFd::from_raw_fd(memfd) };
        check(unsafe { libc::ftruncate(frame_fd.as_raw_fd(), SHARED_BYTES as libc::off_t) }, "ftruncate memfd")?;
        check(
            unsafe { libc::fcntl(frame_fd.as_raw_fd(), libc::F_ADD_SEALS, libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL) },
            "seal memfd",
        )?;

        let shared = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                SHARED_BYTES,
                libc::PROT_READ,
                libc::MAP_SHARED,
                frame_fd.as_raw_fd(),
                0,
            )
        };
        if shared == libc::MAP_FAILED {
            return Err(io::Error::last_os_error()).context("mmap browser shared pixels");
        }

        let (parent_sock, child_sock) = seqpacket_pair()?;
        set_nonblock(parent_sock.as_raw_fd()).context("nonblock browser socket")?;

        let child_frame = dup_high(frame_fd.as_raw_fd())?;
        let child_ctrl = dup_high(child_sock.as_raw_fd())?;
        let raw_frame = child_frame.as_raw_fd();
        let raw_ctrl = child_ctrl.as_raw_fd();

        let mut cmd = Command::new("/proc/self/exe");
        cmd.arg("aera-jail").arg("--browser");
        for ip in dns {
            cmd.arg("--dns").arg(ip.to_string());
        }
        cmd.arg(&runtime);
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(raw_frame, 3) < 0 || libc::dup2(raw_ctrl, 4) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                unsafe { libc::munmap(shared, SHARED_BYTES) };
                return Err(e).context("spawn aweyuki aera-jail --browser");
            }
        };

        Ok(Self {
            child: Some(child),
            sock: parent_sock,
            shared: shared.cast(),
            runtime,
        })
    }

    #[must_use]
    pub fn fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }

    /// Send a control/input message to the isolated browser worker.
    ///
    /// # Errors
    ///
    /// Returns an error if the IPC socket send fails.
    pub fn send(&self, msg: &BrowserMsg) -> Result<()> {
        send_packet(self.sock.as_raw_fd(), &msg.encode())
    }

    /// Drain all currently buffered messages from the browser worker.
    ///
    /// # Errors
    ///
    /// Returns an error if reading the socket fails unexpectedly.
    pub fn recv(&self) -> Result<Vec<BrowserMsg>> {
        let pkts = recv_packets::<{ browser_proto::MSG_BYTES }>(self.sock.as_raw_fd())?;
        Ok(pkts.into_iter().filter_map(|p| BrowserMsg::decode(&p)).collect())
    }

    /// Borrow the BGRA pixel buffer slice for frame `sequence` from the sealed memfd.
    #[must_use]
    pub fn frame_bgra(&self, sequence: u32) -> &[u8] {
        let offset = browser_proto::frame_slot(sequence) * browser_proto::FRAME_BYTES;
        unsafe { std::slice::from_raw_parts(self.shared.add(offset), browser_proto::FRAME_BYTES) }
    }

    pub fn is_running(&mut self) -> bool {
        let Some(c) = self.child.as_mut() else {
            return false;
        };
        !matches!(c.try_wait(), Ok(Some(_)) | Err(_))
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        let pkt = BrowserMsg::cmd(BrowserKind::Close).encode();
        reap_and_clean(&mut self.child, self.sock.as_raw_fd(), &self.runtime, &pkt);
        unsafe { libc::munmap(self.shared.cast_mut().cast(), SHARED_BYTES) };
    }
}

/// Parent-side handle for an isolated AERA Telegram TDLib worker (`--isolated-ipc-v1`).
pub struct TelegramProcess {
    child: Option<Child>,
    sock: OwnedFd,
    runtime: PathBuf,
}

impl TelegramProcess {
    /// Start an isolated TDLib worker from unpacked `runtime` directory.
    ///
    /// # Errors
    ///
    /// Returns an error if creating `socketpair` or spawning fails.
    pub fn start(runtime: PathBuf, dns: &[std::net::Ipv4Addr]) -> Result<Self> {
        let (parent_sock, child_sock) = seqpacket_pair()?;
        set_nonblock(parent_sock.as_raw_fd()).context("nonblock telegram socket")?;

        let child_ctrl = dup_high(child_sock.as_raw_fd())?;
        let raw_ctrl = child_ctrl.as_raw_fd();

        let mut cmd = Command::new("/proc/self/exe");
        cmd.arg("aera-jail").arg("--telegram");
        for ip in dns {
            cmd.arg("--dns").arg(ip.to_string());
        }
        cmd.arg(&runtime);
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(raw_ctrl, 4) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().context("spawn aweyuki aera-jail --telegram")?;
        Ok(Self {
            child: Some(child),
            sock: parent_sock,
            runtime,
        })
    }

    #[must_use]
    pub fn fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }

    /// Send a Telegram worker command message.
    ///
    /// # Errors
    ///
    /// Returns an error if the IPC socket send fails.
    pub fn send(&self, msg: &TgMsg) -> Result<()> {
        send_packet(self.sock.as_raw_fd(), &msg.encode())
    }

    /// Drain all currently buffered messages from the Telegram worker.
    ///
    /// # Errors
    ///
    /// Returns an error if reading the socket fails unexpectedly.
    pub fn recv(&self) -> Result<Vec<TgMsg>> {
        let pkts = recv_packets::<{ telegram_proto::MSG_BYTES }>(self.sock.as_raw_fd())?;
        Ok(pkts.into_iter().filter_map(|p| TgMsg::decode(&p)).collect())
    }

    pub fn is_running(&mut self) -> bool {
        let Some(c) = self.child.as_mut() else {
            return false;
        };
        !matches!(c.try_wait(), Ok(Some(_)) | Err(_))
    }
}

impl Drop for TelegramProcess {
    fn drop(&mut self) {
        let pkt = TgMsg::simple(telegram_proto::TgKind::Close, "").encode();
        reap_and_clean(&mut self.child, self.sock.as_raw_fd(), &self.runtime, &pkt);
    }
}

fn close_unrelated_fds() {
    let Ok(entries) = fs::read_dir("/proc/self/fd") else {
        return;
    };
    let mut to_close = Vec::new();
    for entry in entries.flatten() {
        if let Ok(fd) = entry.file_name().to_string_lossy().parse::<RawFd>()
            && fd >= 5
        {
            to_close.push(fd);
        }
    }
    for fd in to_close {
        let _ = unsafe { libc::close(fd) };
    }
}

fn prepare_storage(kind: JailKind) {
    match kind {
        JailKind::Browser => {
            if Path::new("/sdcard").is_dir() {
                let dl = Path::new("/sdcard/Aweyuki/Downloads");
                if fs::create_dir_all(dl).is_ok() {
                    let _ = chown(dl, Some(UID_BROWSER), Some(GID_MEDIA_RW));
                    let _ = fs::set_permissions(dl, fs::Permissions::from_mode(0o2770));
                }
            }
            if Path::new("/data").is_dir() {
                let root = Path::new("/data/aweyuki-recovery");
                let parent = root.join("browser");
                let profile = parent.join("profile");
                if fs::create_dir_all(&profile).is_ok() {
                    let _ = chown(root, Some(0), Some(0));
                    let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
                    let _ = chown(&parent, Some(0), Some(0));
                    let _ = fs::set_permissions(&parent, fs::Permissions::from_mode(0o700));
                    let _ = chown(&profile, Some(UID_BROWSER), Some(UID_BROWSER));
                    let _ = fs::set_permissions(&profile, fs::Permissions::from_mode(0o700));
                }
            }
        }
        JailKind::Telegram => {
            if Path::new("/data").is_dir() {
                let root = Path::new("/data/aweyuki-recovery");
                let state = root.join("telegram");
                if fs::create_dir_all(&state).is_ok() {
                    let _ = chown(root, Some(0), Some(0));
                    let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
                    let _ = chown(&state, Some(UID_TELEGRAM), Some(UID_TELEGRAM));
                    let _ = fs::set_permissions(&state, fs::Permissions::from_mode(0o700));
                }
            }
            if Path::new("/sdcard").is_dir() {
                let dl = Path::new("/sdcard/Aweyuki/Telegram");
                if fs::create_dir_all(dl).is_ok() {
                    let _ = chown(dl, Some(UID_TELEGRAM), Some(GID_MEDIA_RW));
                    let _ = fs::set_permissions(dl, fs::Permissions::from_mode(0o770));
                }
            }
        }
    }
}

fn write_resolv_conf(runtime: &Path, dns: &[String]) -> Result<()> {
    let etc = runtime.join("etc");
    fs::create_dir_all(&etc)?;
    let mut lines = String::new();
    let mut add_ip = |raw: &str| {
        let trimmed = raw.trim();
        if (trimmed.parse::<std::net::Ipv4Addr>().is_ok() || trimmed.parse::<std::net::Ipv6Addr>().is_ok())
            && !lines.contains(trimmed)
        {
            lines.push_str("nameserver ");
            lines.push_str(trimmed);
            lines.push('\n');
        }
    };
    for ip in dns {
        add_ip(ip);
    }
    for key in ["net.dns1", "net.dns2"] {
        if let Some(val) = aweyuki_sys::prop(key) {
            add_ip(&val);
        }
    }
    if lines.is_empty() {
        lines.push_str("nameserver 1.1.1.1\nnameserver 8.8.8.8\n");
    }
    let resolv = etc.join("resolv.conf");
    fs::write(&resolv, lines)?;
    fs::set_permissions(&resolv, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

fn mount_raw(src: Option<&Path>, dst: &Path, fstype: Option<&str>, flags: libc::c_ulong, data: Option<&str>) -> Result<()> {
    let c_src = src.map(cpath).transpose()?;
    let c_dst = cpath(dst)?;
    let c_ty = fstype.map(cstr).transpose()?;
    let c_data = data.map(cstr).transpose()?;
    let rc = unsafe {
        libc::mount(
            c_src.as_ref().map_or(null(), |c| c.as_ptr()),
            c_dst.as_ptr(),
            c_ty.as_ref().map_or(null(), |c| c.as_ptr()),
            flags,
            c_data.as_ref().map_or(null(), |c| c.as_ptr().cast()),
        )
    };
    check(rc, &format!("mount {}", dst.display()))
}

fn bind_dir(src: &Path, dst: &Path, read_only: bool) -> Result<()> {
    fs::create_dir_all(dst)?;
    mount_raw(Some(src), dst, None, libc::MS_BIND, None)?;
    if read_only {
        mount_raw(Some(src), dst, None, libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY, None)?;
    }
    Ok(())
}

fn bind_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if !dst.exists() {
        OpenOptions::new().write(true).create(true).truncate(false).open(dst)?;
    }
    mount_raw(Some(src), dst, None, libc::MS_BIND, None)
}

fn bpf_stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter { code, jt: 0, jf: 0, k }
}

fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

fn build_seccomp_policy() -> Vec<libc::sock_filter> {
    let mut p = Vec::new();
    let denied = libc::SECCOMP_RET_ERRNO | (libc::EPERM as u32);
    let allow = libc::SECCOMP_RET_ALLOW;

    // 1. Validate architecture == aarch64
    p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 4));
    p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_AARCH64, 1, 0));
    p.push(bpf_stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_KILL_PROCESS));

    // 2. Load syscall number
    p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 0));

    let push_special = |p: &mut Vec<libc::sock_filter>, nr: u32, body: &dyn Fn(&mut Vec<libc::sock_filter>)| {
        let at = p.len();
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, nr, 0, 0));
        body(p);
        let skip = p.len() - at - 1;
        p[at].jf = skip as u8;
    };

    // SYS_clone: allow threads/fork/vfork only, forbid namespaces & ptrace.
    push_special(&mut p, libc::SYS_clone as u32, &|p| {
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 20));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 0, 1, 0));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 16));
        let allowed_flags: u32 = 0x017d_4fff;
        p.push(bpf_stmt(BPF_ALU | BPF_AND | BPF_K, !allowed_flags));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 0, 1, 0));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
    });

    // SYS_clone3 -> ENOSYS so libc falls back to clone.
    push_special(&mut p, libc::SYS_clone3 as u32, &|p| {
        p.push(bpf_stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ERRNO | (libc::ENOSYS as u32)));
    });

    // SYS_socket: AF_UNIX STREAM, AF_NETLINK, AF_INET/AF_INET6 STREAM/DGRAM.
    push_special(&mut p, libc::SYS_socket as u32, &|p| {
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 16));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_UNIX as u32, 0, 5));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 24));
        p.push(bpf_stmt(BPF_ALU | BPF_AND | BPF_K, !(libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK) as u32));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SOCK_STREAM as u32, 0, 1));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_NETLINK as u32, 0, 1));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_INET as u32, 1, 0));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_INET6 as u32, 0, 4));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 24));
        p.push(bpf_stmt(BPF_ALU | BPF_AND | BPF_K, !(libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK) as u32));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SOCK_STREAM as u32, 2, 0));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SOCK_DGRAM as u32, 1, 0));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
    });

    // SYS_socketpair: AF_UNIX only.
    push_special(&mut p, libc::SYS_socketpair as u32, &|p| {
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 16));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_UNIX as u32, 1, 0));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
    });

    // SYS_prctl: safe read queries + PR_CAPBSET_READ cap>=38 returning 0 on older kernels.
    push_special(&mut p, libc::SYS_prctl as u32, &|p| {
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 16));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::PR_CAPBSET_READ as u32, 0, 7));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 28));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 0, 1, 0));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 24));
        p.push(bpf_jump(BPF_JMP | BPF_JGE | BPF_K, 38, 1, 0));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
        p.push(bpf_stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ERRNO));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 16));
        for opt in [
            libc::PR_GET_NO_NEW_PRIVS,
            libc::PR_GET_SECCOMP,
            libc::PR_GET_DUMPABLE,
            libc::PR_GET_NAME,
            libc::PR_SET_NAME,
            libc::PR_GET_PDEATHSIG,
            libc::PR_GET_TIMERSLACK,
            libc::PR_SET_TIMERSLACK,
        ] {
            p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, opt as u32, 0, 1));
            p.push(bpf_stmt(BPF_RET | BPF_K, allow));
        }
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::PR_SET_DUMPABLE as u32, 0, 4));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 24));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 0, 0, 1));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::PR_SET_NO_NEW_PRIVS as u32, 0, 4));
        p.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 24));
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 1, 0, 1));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
        p.push(bpf_stmt(BPF_RET | BPF_K, denied));
    });

    let allowed_syscalls: &[u32] = &[
        libc::SYS_read as u32,
        libc::SYS_write as u32,
        libc::SYS_readv as u32,
        libc::SYS_writev as u32,
        libc::SYS_pread64 as u32,
        libc::SYS_pwrite64 as u32,
        libc::SYS_openat as u32,
        libc::SYS_close as u32,
        libc::SYS_lseek as u32,
        SYS_FSTAT_NR,
        SYS_NEWFSTATAT_NR,
        SYS_FSTATFS_NR,
        SYS_STATFS_NR,
        libc::SYS_faccessat as u32,
        libc::SYS_readlinkat as u32,
        libc::SYS_getdents64 as u32,
        libc::SYS_fcntl as u32,
        libc::SYS_ioctl as u32,
        libc::SYS_dup as u32,
        libc::SYS_dup3 as u32,
        libc::SYS_pipe2 as u32,
        SYS_FTRUNCATE_NR,
        libc::SYS_fallocate as u32,
        SYS_FADVISE64_NR,
        libc::SYS_fsync as u32,
        libc::SYS_fdatasync as u32,
        libc::SYS_mkdirat as u32,
        libc::SYS_unlinkat as u32,
        libc::SYS_renameat as u32,
        libc::SYS_linkat as u32,
        libc::SYS_symlinkat as u32,
        libc::SYS_fchmod as u32,
        libc::SYS_fchmodat as u32,
        libc::SYS_umask as u32,
        libc::SYS_utimensat as u32,
        libc::SYS_chdir as u32,
        libc::SYS_fchdir as u32,
        libc::SYS_getcwd as u32,
        libc::SYS_mmap as u32,
        libc::SYS_mprotect as u32,
        libc::SYS_munmap as u32,
        libc::SYS_mremap as u32,
        libc::SYS_madvise as u32,
        libc::SYS_brk as u32,
        libc::SYS_msync as u32,
        libc::SYS_mlock as u32,
        libc::SYS_munlock as u32,
        libc::SYS_memfd_create as u32,
        libc::SYS_rt_sigaction as u32,
        libc::SYS_rt_sigprocmask as u32,
        libc::SYS_rt_sigreturn as u32,
        libc::SYS_sigaltstack as u32,
        libc::SYS_rt_sigsuspend as u32,
        libc::SYS_rt_sigtimedwait as u32,
        libc::SYS_restart_syscall as u32,
        libc::SYS_getpid as u32,
        libc::SYS_getppid as u32,
        libc::SYS_gettid as u32,
        libc::SYS_getuid as u32,
        libc::SYS_geteuid as u32,
        libc::SYS_getgid as u32,
        libc::SYS_getegid as u32,
        libc::SYS_getresuid as u32,
        libc::SYS_getresgid as u32,
        libc::SYS_getgroups as u32,
        libc::SYS_getpgid as u32,
        libc::SYS_getsid as u32,
        libc::SYS_uname as u32,
        libc::SYS_sysinfo as u32,
        libc::SYS_getrandom as u32,
        libc::SYS_getcpu as u32,
        libc::SYS_capget as u32,
        libc::SYS_futex as u32,
        libc::SYS_set_tid_address as u32,
        libc::SYS_set_robust_list as u32,
        libc::SYS_get_robust_list as u32,
        libc::SYS_sched_yield as u32,
        libc::SYS_sched_getaffinity as u32,
        libc::SYS_sched_setaffinity as u32,
        libc::SYS_sched_setscheduler as u32,
        libc::SYS_sched_getparam as u32,
        libc::SYS_sched_getscheduler as u32,
        libc::SYS_sched_get_priority_max as u32,
        libc::SYS_sched_get_priority_min as u32,
        libc::SYS_getpriority as u32,
        libc::SYS_setpriority as u32,
        libc::SYS_clock_gettime as u32,
        libc::SYS_clock_getres as u32,
        libc::SYS_gettimeofday as u32,
        libc::SYS_nanosleep as u32,
        libc::SYS_clock_nanosleep as u32,
        libc::SYS_getrusage as u32,
        libc::SYS_getrlimit as u32,
        libc::SYS_prlimit64 as u32,
        libc::SYS_timer_create as u32,
        libc::SYS_timer_settime as u32,
        libc::SYS_timer_gettime as u32,
        libc::SYS_timer_delete as u32,
        libc::SYS_timerfd_create as u32,
        libc::SYS_timerfd_settime as u32,
        libc::SYS_timerfd_gettime as u32,
        libc::SYS_eventfd2 as u32,
        libc::SYS_epoll_create1 as u32,
        libc::SYS_epoll_ctl as u32,
        libc::SYS_epoll_pwait as u32,
        libc::SYS_ppoll as u32,
        libc::SYS_pselect6 as u32,
        libc::SYS_signalfd4 as u32,
        libc::SYS_bind as u32,
        libc::SYS_connect as u32,
        libc::SYS_sendto as u32,
        libc::SYS_recvfrom as u32,
        libc::SYS_sendmsg as u32,
        libc::SYS_recvmsg as u32,
        libc::SYS_sendmmsg as u32,
        libc::SYS_recvmmsg as u32,
        libc::SYS_shutdown as u32,
        libc::SYS_getsockopt as u32,
        libc::SYS_setsockopt as u32,
        libc::SYS_getsockname as u32,
        libc::SYS_getpeername as u32,
        libc::SYS_execve as u32,
        libc::SYS_wait4 as u32,
        libc::SYS_waitid as u32,
        libc::SYS_exit as u32,
        libc::SYS_exit_group as u32,
        libc::SYS_statx as u32,
        libc::SYS_faccessat2 as u32,
        libc::SYS_renameat2 as u32,
        SYS_RSEQ_NR,
        libc::SYS_membarrier as u32,
        libc::SYS_epoll_pwait2 as u32,
        libc::SYS_kill as u32,
        libc::SYS_tgkill as u32,
        libc::SYS_tkill as u32,
    ];
    for &nr in allowed_syscalls {
        p.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, nr, 0, 1));
        p.push(bpf_stmt(BPF_RET | BPF_K, allow));
    }
    p.push(bpf_stmt(BPF_RET | BPF_K, denied));
    p
}

/// Set up the unprivileged AERA mount/UTS/chroot/UID/seccomp jail and `execve`
/// `aera-browser-worker` or `aera-telegram` with `--isolated-ipc-v1`.
///
/// # Errors
///
/// Returns an error if setting up namespaces, mounts, credentials, seccomp, or
/// executing the isolated runtime binary fails.
pub fn exec_jail(kind: JailKind, runtime: &Path, dns: &[String]) -> Result<()> {
    close_unrelated_fds();
    let _ = set_cloexec(3, false);
    let _ = set_cloexec(4, false);
    prepare_storage(kind);

    for sub in ["etc", "proc", "tmp", "run", "dev", "dev/shm", "downloads"] {
        fs::create_dir_all(runtime.join(sub))?;
    }
    if kind == JailKind::Browser {
        fs::create_dir_all(runtime.join("profile"))?;
    } else {
        fs::create_dir_all(runtime.join("state"))?;
        fs::create_dir_all(runtime.join("sdcard"))?;
    }
    write_resolv_conf(runtime, dns)?;
    chown(runtime, Some(0), Some(0))?;
    fs::set_permissions(runtime, fs::Permissions::from_mode(0o755))?;

    let limits = [
        (libc::RLIMIT_NOFILE, 512u64),
        (libc::RLIMIT_NPROC, 192u64),
        (libc::RLIMIT_CORE, 0u64),
        (libc::RLIMIT_FSIZE, 16u64 << 30),
    ];
    for (res, val) in limits {
        let rlim = libc::rlimit { rlim_cur: val, rlim_max: val };
        check(unsafe { libc::setrlimit(res, &rlim) }, "setrlimit")?;
    }

    check(unsafe { libc::unshare(libc::CLONE_NEWNS | libc::CLONE_NEWUTS) }, "unshare mount+uts")?;
    mount_raw(None, Path::new("/"), None, libc::MS_REC | libc::MS_PRIVATE, None)?;
    let hostname = match kind {
        JailKind::Browser => "aera-browser",
        JailKind::Telegram => "aera-telegram",
    };
    check(unsafe { libc::sethostname(hostname.as_ptr().cast(), hostname.len()) }, "sethostname")?;

    mount_raw(Some(runtime), runtime, None, libc::MS_BIND, None)?;

    let nosuid_nodev_noexec = libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC;
    let proc_dst = runtime.join("proc");
    if mount_raw(
        Some(Path::new("proc")),
        &proc_dst,
        Some("proc"),
        nosuid_nodev_noexec | libc::MS_RDONLY,
        Some("hidepid=2,subset=pid"),
    )
    .is_err()
    {
        mount_raw(
            Some(Path::new("proc")),
            &proc_dst,
            Some("proc"),
            nosuid_nodev_noexec | libc::MS_RDONLY,
            Some("hidepid=2"),
        )?;
    }

    let uid = if kind == JailKind::Browser { UID_BROWSER } else { UID_TELEGRAM };
    mount_raw(Some(Path::new("tmpfs")), &runtime.join("tmp"), Some("tmpfs"), nosuid_nodev_noexec, Some("size=512M,mode=1777"))?;
    mount_raw(
        Some(Path::new("tmpfs")),
        &runtime.join("run"),
        Some("tmpfs"),
        nosuid_nodev_noexec,
        Some(&format!("size=16M,mode=0755,uid={uid},gid={uid}")),
    )?;
    mount_raw(
        Some(Path::new("tmpfs")),
        &runtime.join("dev"),
        Some("tmpfs"),
        libc::MS_NOSUID | libc::MS_NOEXEC,
        Some("size=1M,mode=0755"),
    )?;
    for dev in ["/dev/null", "/dev/zero", "/dev/urandom", "/dev/random"] {
        let p = Path::new(dev);
        bind_file(p, &runtime.join(dev.trim_start_matches('/')))?;
    }
    fs::create_dir_all(runtime.join("dev/shm"))?;
    mount_raw(Some(Path::new("tmpfs")), &runtime.join("dev/shm"), Some("tmpfs"), nosuid_nodev_noexec, Some("size=128M,mode=1777"))?;

    match kind {
        JailKind::Browser => {
            if Path::new("/dev/kgsl-3d0").exists() {
                bind_file(Path::new("/dev/kgsl-3d0"), &runtime.join("dev/kgsl-3d0"))?;
            }
            if Path::new("/dev/dma_heap/system").exists() {
                bind_file(Path::new("/dev/dma_heap/system"), &runtime.join("dev/dma_heap/system"))?;
            } else if Path::new("/dev/ion").exists() {
                bind_file(Path::new("/dev/ion"), &runtime.join("dev/ion"))?;
            }
            if Path::new("/sdcard/Aweyuki/Downloads").is_dir() {
                bind_dir(Path::new("/sdcard/Aweyuki/Downloads"), &runtime.join("downloads"), false)?;
            } else {
                mount_raw(
                    Some(Path::new("tmpfs")),
                    &runtime.join("downloads"),
                    Some("tmpfs"),
                    nosuid_nodev_noexec,
                    Some(&format!("size=64M,mode=0770,uid={uid},gid={uid}")),
                )?;
            }
            if Path::new("/data/aweyuki-recovery/browser/profile").is_dir() {
                bind_dir(Path::new("/data/aweyuki-recovery/browser/profile"), &runtime.join("profile"), false)?;
            } else {
                mount_raw(
                    Some(Path::new("tmpfs")),
                    &runtime.join("profile"),
                    Some("tmpfs"),
                    nosuid_nodev_noexec,
                    Some(&format!("size=64M,mode=0700,uid={uid},gid={uid}")),
                )?;
            }
        }
        JailKind::Telegram => {
            if Path::new("/data/aweyuki-recovery/telegram").is_dir() {
                bind_dir(Path::new("/data/aweyuki-recovery/telegram"), &runtime.join("state"), false)?;
            } else {
                mount_raw(
                    Some(Path::new("tmpfs")),
                    &runtime.join("state"),
                    Some("tmpfs"),
                    nosuid_nodev_noexec,
                    Some(&format!("size=64M,mode=0700,uid={uid},gid={uid}")),
                )?;
            }
            if Path::new("/sdcard/Aweyuki/Telegram").is_dir() {
                bind_dir(Path::new("/sdcard/Aweyuki/Telegram"), &runtime.join("downloads"), false)?;
            } else {
                mount_raw(
                    Some(Path::new("tmpfs")),
                    &runtime.join("downloads"),
                    Some("tmpfs"),
                    nosuid_nodev_noexec,
                    Some(&format!("size=64M,mode=0770,uid={uid},gid={uid}")),
                )?;
            }
            if Path::new("/sdcard").is_dir() {
                bind_dir(Path::new("/sdcard"), &runtime.join("sdcard"), true)?;
            }
        }
    }

    mount_raw(Some(runtime), runtime, None, libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY, None)?;

    let c_root = cpath(runtime)?;
    check(unsafe { libc::chroot(c_root.as_ptr()) }, "chroot runtime")?;
    let c_slash = cstr("/")?;
    check(unsafe { libc::chdir(c_slash.as_ptr()) }, "chdir /")?;

    for cap in 0..=40u32 {
        let rc = unsafe { libc::prctl(libc::PR_CAPBSET_DROP, libc::c_ulong::from(cap), 0, 0, 0) };
        if rc != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
            break;
        }
    }

    let groups: &[libc::gid_t] = match kind {
        JailKind::Browser => &[GID_INET],
        JailKind::Telegram => &[GID_INET, GID_MEDIA_RW],
    };
    check(unsafe { libc::setgroups(groups.len(), groups.as_ptr()) }, "setgroups")?;
    check(unsafe { libc::setresgid(uid, uid, uid) }, "setresgid")?;
    check(unsafe { libc::setresuid(uid, uid, uid) }, "setresuid")?;

    #[repr(C)]
    struct CapHdr {
        version: u32,
        pid: libc::c_int,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CapData {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    let hdr = CapHdr { version: CAP_V3, pid: 0 };
    let data = [CapData { effective: 0, permitted: 0, inheritable: 0 }; 2];
    if unsafe { libc::syscall(libc::SYS_capset, &hdr, data.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error()).context("capset clear");
    }

    check(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }, "PR_SET_NO_NEW_PRIVS")?;

    let mut policy = build_seccomp_policy();
    let fprog = libc::sock_fprog {
        len: policy.len() as libc::c_ushort,
        filter: policy.as_mut_ptr(),
    };
    check(
        unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER as libc::c_ulong, (&raw const fprog) as libc::c_ulong, 0, 0) },
        "PR_SET_SECCOMP",
    )?;
    check(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) }, "PR_SET_DUMPABLE")?;

    let prog = match kind {
        JailKind::Browser => "/usr/bin/aera-browser-worker",
        JailKind::Telegram => "/usr/bin/aera-telegram",
    };
    let c_prog = cstr(prog)?;
    let c_arg1 = cstr("--isolated-ipc-v1")?;
    let argv = [c_prog.as_ptr(), c_arg1.as_ptr(), null()];

    let env_strs: Vec<CString> = match kind {
        JailKind::Browser => [
            "PATH=/usr/bin",
            "HOME=/tmp",
            "TMPDIR=/tmp",
            "LANG=en.UTF-8",
            "AERA_LOCALE=en",
            "XDG_RUNTIME_DIR=/run",
            "XDG_CACHE_HOME=/tmp/cache",
            "XDG_DATA_HOME=/tmp/data",
            "XDG_CONFIG_HOME=/tmp/config",
            "XDG_DATA_DIRS=/usr/share",
            "GSETTINGS_BACKEND=memory",
            "GIO_MODULE_DIR=/usr/lib/gio/modules",
            "GIO_USE_PROXY_RESOLVER=dummy",
            "GIO_USE_NETWORK_MONITOR=base",
            "VK_DRIVER_FILES=/usr/share/vulkan/icd.d/freedreno_icd.json",
            "MESA_LOADER_DRIVER_OVERRIDE=zink",
            "LIBGL_DRIVERS_PATH=/usr/lib/dri",
            "EGL_PLATFORM=surfaceless",
            "ZINK_DESCRIPTORS=lazy",
            "ZINK_DEBUG=sync,flushsync",
            "MESA_SHADER_CACHE_DISABLE=true",
            "GST_PLUGIN_SYSTEM_PATH=/usr/lib/gstreamer-1.0",
            "GST_PLUGIN_SCANNER=/usr/libexec/gstreamer-1.0/gst-plugin-scanner",
            "GST_REGISTRY=/tmp/gstreamer-registry.bin",
            "GST_REGISTRY_REUSE_PLUGIN_SCANNER=no",
            "WEBKIT_CAIRO_PAINTING_THREADS=6",
            "ICU_DATA=/usr/share/icu/76.1",
            "FONTCONFIG_PATH=/etc/fonts",
            "XKB_CONFIG_ROOT=/usr/share/X11/xkb",
        ]
        .iter()
        .map(|s| cstr(s))
        .collect::<Result<Vec<_>>>()?,
        JailKind::Telegram => [
            "PATH=/usr/bin",
            "HOME=/state",
            "TMPDIR=/tmp",
            "LANG=en.UTF-8",
            "AERA_LOCALE=en",
        ]
        .iter()
        .map(|s| cstr(s))
        .collect::<Result<Vec<_>>>()?,
    };
    let mut envp: Vec<*const libc::c_char> = env_strs.iter().map(|s| s.as_ptr()).collect();
    envp.push(null());

    unsafe { libc::execve(c_prog.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
    Err(io::Error::last_os_error()).with_context(|| format!("execve {prog}"))
}
