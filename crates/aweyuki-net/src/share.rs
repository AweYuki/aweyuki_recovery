//! Lightweight HTTP/1.1 file-sharing server for transferring files over
//! Wi-Fi / USB-NCM without needing MTP or adb.

use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub const DEFAULT_PORT: u16 = 8080;
const MAX_HEADERS: usize = 64;

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(raw: &str) -> Result<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                bail!("truncated percent escape");
            }
            let hi = hex_nibble(bytes[i + 1]).context("invalid percent hex")?;
            let lo = hex_nibble(bytes[i + 2]).context("invalid percent hex")?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).context("invalid UTF-8 in URL path")
}

fn percent_encode_segment(seg: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(seg.len());
    for &b in seg.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Normalize a URL request path into a safe relative path under `root`,
/// decoding `%XX` sequences and rejecting `..`, `\`, or invalid components.
///
/// # Errors
///
/// Returns an error if percent-decoding fails or any segment attempts path
/// traversal (`..`, NUL, backslash).
pub fn sanitize_rel_path(raw: &str) -> Result<PathBuf> {
    let path_only = raw.split('?').next().unwrap_or("");
    let decoded = percent_decode(path_only)?;
    let trimmed = decoded.trim_start_matches('/');
    let mut out = PathBuf::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." || seg.contains('\0') || seg.contains('\\') {
            bail!("invalid path segment {seg:?}");
        }
        out.push(seg);
    }
    for c in out.components() {
        if !matches!(c, Component::Normal(_)) {
            bail!("invalid path component");
        }
    }
    Ok(out)
}

/// Bind a TCP listener for the file-sharing server.
///
/// # Errors
///
/// Returns an error if binding `addr` fails.
pub fn bind(addr: SocketAddr) -> Result<TcpListener> {
    TcpListener::bind(addr).with_context(|| format!("bind {addr}"))
}

/// Serve requests from `listener` using `root` as the document root.
///
/// # Errors
///
/// Returns an error only if accepting connections fails fatally.
pub fn serve_listener(listener: TcpListener, root: &Path) -> Result<()> {
    for stream in listener.incoming().flatten() {
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let _ = handle_client(stream, &root);
        });
    }
    Ok(())
}

/// Bind `addr` and serve files under `root`.
///
/// # Errors
///
/// Returns an error if binding fails.
pub fn serve(root: &Path, addr: SocketAddr) -> Result<()> {
    serve_listener(bind(addr)?, root)
}

fn nonblocking_pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let ends = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for end in [&ends.0, &ends.1] {
        let fd = end.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0
            || flags < 0
            || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(ends)
}

/// Stoppable HTTP/1.1 file-sharing server running on a background thread.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    wake_write: OwnedFd,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Bind `addr` and serve `root` on a background thread until dropped or stopped.
    ///
    /// # Errors
    ///
    /// Returns an error if binding the socket, configuring non-blocking mode,
    /// creating the wake pipe, or spawning the server thread fails.
    pub fn start(root: PathBuf, addr: SocketAddr) -> Result<Self> {
        let listener = bind(addr)?;
        let local = listener.local_addr().context("read bound share address")?;
        listener.set_nonblocking(true).context("set share listener non-blocking")?;
        let (wake_read, wake_write) = nonblocking_pipe().context("create share wake pipe")?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        let thread = thread::Builder::new()
            .name("aweyuki-share".into())
            .spawn(move || serve_until_stopped(listener, wake_read, root, stop_flag))
            .context("start share server thread")?;

        Ok(Server {
            addr: local,
            stop,
            wake_write,
            thread: Some(thread),
        })
    }

    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    fn stop_inner(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = unsafe { libc::write(self.wake_write.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    /// Gracefully stop the background server and release its listening socket.
    pub fn stop(mut self) {
        self.stop_inner();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

fn serve_until_stopped(listener: TcpListener, wake_read: OwnedFd, root: PathBuf, stop: Arc<AtomicBool>) {
    let mut pollfds = [
        libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: wake_read.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        for p in &mut pollfds {
            p.revents = 0;
        }
        let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if stop.load(Ordering::Acquire) || (pollfds[1].revents & libc::POLLIN) != 0 {
            break;
        }
        if (pollfds[0].revents & libc::POLLIN) != 0 {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let root = root.clone();
                        thread::spawn(move || {
                            let _ = handle_client(stream, &root);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => return,
                }
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn write_directory_listing(stream: &mut TcpStream, target: &Path, rel: &Path) -> Result<()> {
    use std::fmt::Write as _;

    let mut entries: Vec<_> = fs::read_dir(target)?.filter_map(Result::ok).collect();
    entries.sort_by_key(fs::DirEntry::file_name);

    let prefix: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(percent_encode_segment(&s.to_string_lossy())),
            _ => None,
        })
        .collect();

    let mut rows = String::new();
    if !prefix.is_empty() {
        let up = if prefix.len() == 1 {
            "/".to_string()
        } else {
            format!("/{}/", prefix[..prefix.len() - 1].join("/"))
        };
        let _ = writeln!(rows, "<li><a href=\"{up}\">../</a></li>");
    }
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().is_some_and(fs::Metadata::is_dir) || entry.path().is_dir();
        let slash = if is_dir { "/" } else { "" };
        let size = if is_dir {
            String::new()
        } else {
            meta.map(|m| format!(" <small>({})</small>", format_size(m.len()))).unwrap_or_default()
        };
        let encoded = percent_encode_segment(&name);
        let href = if prefix.is_empty() {
            format!("/{encoded}{slash}")
        } else {
            format!("/{}/{encoded}{slash}", prefix.join("/"))
        };
        let label = escape_html(&name);
        let _ = writeln!(rows, "<li><a href=\"{href}\">{label}{slash}</a>{size}</li>");
    }

    let heading = escape_html(&rel.display().to_string());
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>/{heading}</title>\
<style>body{{font-family:system-ui,sans-serif;max-width:720px;margin:2rem auto;padding:0 1rem}}\
ul{{list-style:none;padding:0}}li{{padding:.35rem 0;border-bottom:1px solid #eee}}\
a{{text-decoration:none;color:#0b57d0}}small{{color:#666;margin-left:.5rem}}\
.up{{margin:1rem 0;padding:.75rem;background:#f5f5f5;border-radius:8px;display:flex;gap:.5rem;flex-wrap:wrap;align-items:center}}</style>\
</head><body><h2>Aweyuki Share: /{heading}</h2>\
<div class=\"up\"><input type=\"file\" id=\"f\"><button type=\"button\" onclick=\"upload()\">Upload</button><span id=\"s\"></span></div>\
<ul>\n{rows}</ul>\
<script>async function upload(){{const f=document.getElementById('f').files[0];const s=document.getElementById('s');\
if(!f){{s.textContent='Pick a file';return;}}\
const b=location.pathname.endsWith('/')?location.pathname:location.pathname+'/';\
s.textContent='Uploading...';const r=await fetch(b+encodeURIComponent(f.name),{{method:'PUT',body:f}});\
if(r.ok){{location.reload();}}else{{s.textContent='Error '+r.status;}}}}</script>\
</body></html>"
    );
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    Ok(())
}

/// Handle a single HTTP/1.1 `GET` or `PUT` connection rooted at `root`.
///
/// # Errors
///
/// Returns an error on socket or filesystem I/O failures.
pub fn handle_client(mut stream: TcpStream, root: &Path) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_target = parts.next().unwrap_or("/");

    let mut content_length: Option<u64> = None;
    for count in 0..=MAX_HEADERS {
        if count == MAX_HEADERS {
            stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 16\r\n\r\nToo Many Headers")?;
            return Ok(());
        }
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':')
            && k.trim().eq_ignore_ascii_case("Content-Length")
        {
            let Ok(parsed) = v.trim().parse::<u64>() else {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 22\r\n\r\nInvalid Content-Length")?;
                return Ok(());
            };
            content_length = Some(parsed);
        }
    }

    let Ok(rel) = sanitize_rel_path(raw_target) else {
        stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nBad Request")?;
        return Ok(());
    };
    let target = root.join(&rel);

    match method {
        "GET" => {
            if target.is_dir() {
                write_directory_listing(&mut stream, &target, &rel)?;
            } else if target.is_file() {
                let mut file = File::open(&target)?;
                let len = file.metadata()?.len();
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {len}\r\n\r\n"
                );
                stream.write_all(header.as_bytes())?;
                std::io::copy(&mut file, &mut stream)?;
            } else {
                stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found")?;
            }
        }
        "PUT" => {
            let Some(expected) = content_length else {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 22\r\n\r\nMissing Content-Length")?;
                return Ok(());
            };
            if rel.as_os_str().is_empty() || target.is_dir() {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nBad Request")?;
                return Ok(());
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&target)?;
            let mut limited = reader.take(expected);
            let copied = std::io::copy(&mut limited, &mut file)?;
            file.sync_all()?;
            if copied != expected {
                drop(file);
                let _ = fs::remove_file(&target);
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 14\r\n\r\nTruncated Body")?;
                return Ok(());
            }
            stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 7\r\n\r\nCreated")?;
        }
        _ => {
            stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 18\r\n\r\nMethod Not Allowed")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn path_traversal_and_percent_escapes_are_validated() {
        assert!(sanitize_rel_path("/../etc/passwd").is_err());
        assert!(sanitize_rel_path("/%2e%2e/etc/passwd").is_err());
        assert!(sanitize_rel_path("/a%2f..%2fb").is_err());
        assert_eq!(
            sanitize_rel_path("/Download/my%20rom.zip").unwrap(),
            PathBuf::from("Download/my rom.zip")
        );
    }

    #[test]
    fn http_put_get_listing_and_errors_over_loopback() {
        let root = TempDir::new("aweyuki-share-tests");
        let listener = bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();
        let srv_root = root.0.clone();
        let handle = std::thread::spawn(move || {
            for s in listener.incoming().take(4).flatten() {
                let _ = handle_client(s, &srv_root);
            }
        });

        let mut c1 = TcpStream::connect(addr).unwrap();
        c1.write_all(b"PUT /a<b>.txt HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello").unwrap();
        let mut resp1 = String::new();
        c1.read_to_string(&mut resp1).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 201 Created"), "{resp1}");

        let mut c2 = TcpStream::connect(addr).unwrap();
        c2.write_all(b"GET /a%3Cb%3E.txt HTTP/1.1\r\n\r\n").unwrap();
        let mut resp2 = String::new();
        c2.read_to_string(&mut resp2).unwrap();
        assert!(resp2.ends_with("\r\n\r\nhello"), "{resp2}");

        let mut c3 = TcpStream::connect(addr).unwrap();
        c3.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        let mut resp3 = String::new();
        c3.read_to_string(&mut resp3).unwrap();
        assert!(resp3.contains("href=\"/a%3Cb%3E.txt\">a&lt;b&gt;.txt</a>"), "{resp3}");

        let mut c4 = TcpStream::connect(addr).unwrap();
        c4.write_all(b"PUT /bad.txt HTTP/1.1\r\nContent-Length: nope\r\n\r\n").unwrap();
        let mut resp4 = String::new();
        c4.read_to_string(&mut resp4).unwrap();
        assert!(resp4.starts_with("HTTP/1.1 400 Bad Request"), "{resp4}");

        handle.join().unwrap();
    }

    #[test]
    fn server_starts_serves_and_stops_cleanly() {
        let root = TempDir::new("aweyuki-share-server");
        let server = Server::start(root.0.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr();

        let mut c1 = TcpStream::connect(addr).unwrap();
        c1.write_all(b"PUT /ping.txt HTTP/1.1\r\nContent-Length: 4\r\n\r\npong").unwrap();
        let mut resp1 = String::new();
        c1.read_to_string(&mut resp1).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 201 Created"), "{resp1}");

        let mut c2 = TcpStream::connect(addr).unwrap();
        c2.write_all(b"GET /ping.txt HTTP/1.1\r\n\r\n").unwrap();
        let mut resp2 = String::new();
        c2.read_to_string(&mut resp2).unwrap();
        assert!(resp2.ends_with("\r\n\r\npong"), "{resp2}");

        server.stop();
        assert!(TcpStream::connect(addr).is_err(), "stopped server releases its listening socket");
    }
}
