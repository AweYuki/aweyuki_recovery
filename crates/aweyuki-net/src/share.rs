//! Lightweight HTTP/1.1 file-sharing server for transferring files over
//! Wi-Fi / USB-NCM without needing MTP or adb.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
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
const MAX_INPUT_BYTES: u64 = 4096;

/// Hardware key events accepted by the browser remote mirror (`POST /input`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteKey {
    Power,
    VolumeUp,
    VolumeDown,
    Back,
    Screenshot,
}

/// Normalized pointer or hardware-key input event posted by `/mirror`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteInput {
    Tap { x: f32, y: f32 },
    Swipe { x0: f32, y0: f32, x1: f32, y1: f32 },
    Key { key: RemoteKey },
}

impl RemoteInput {
    fn is_valid(self) -> bool {
        match self {
            RemoteInput::Tap { x, y } => x.is_finite() && y.is_finite(),
            RemoteInput::Swipe { x0, y0, x1, y1 } => {
                x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()
            }
            RemoteInput::Key { .. } => true,
        }
    }
}

/// Optional live framebuffer PNG provider and input sink for `/mirror`,
/// `/screen.png`, and `POST /input`.
#[derive(Clone)]
pub struct MirrorHooks {
    pub size: (u32, u32),
    pub frame_png: Arc<dyn Fn() -> Option<Vec<u8>> + Send + Sync>,
    pub on_input: Arc<dyn Fn(RemoteInput) + Send + Sync>,
}

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
            let _ = handle_client_with_mirror(stream, &root, None);
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
        Self::start_with_mirror(root, addr, None)
    }

    /// Bind `addr` and serve `root` alongside optional remote-mirror endpoints
    /// on a background thread until dropped or stopped.
    ///
    /// # Errors
    ///
    /// Returns an error if binding the socket, configuring non-blocking mode,
    /// creating the wake pipe, or spawning the server thread fails.
    pub fn start_with_mirror(root: PathBuf, addr: SocketAddr, mirror: Option<MirrorHooks>) -> Result<Self> {
        let listener = bind(addr)?;
        let local = listener.local_addr().context("read bound share address")?;
        listener.set_nonblocking(true).context("set share listener non-blocking")?;
        let (wake_read, wake_write) = nonblocking_pipe().context("create share wake pipe")?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        let thread = thread::Builder::new()
            .name("aweyuki-share".into())
            .spawn(move || serve_until_stopped(listener, wake_read, root, mirror, stop_flag))
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

fn serve_until_stopped(
    listener: TcpListener,
    wake_read: OwnedFd,
    root: PathBuf,
    mirror: Option<MirrorHooks>,
    stop: Arc<AtomicBool>,
) {
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
                        let mirror = mirror.clone();
                        thread::spawn(move || {
                            let _ = handle_client_with_mirror(stream, &root, mirror.as_ref());
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

fn write_directory_listing(stream: &mut TcpStream, target: &Path, rel: &Path, has_mirror: bool) -> Result<()> {
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
    let mirror_link = if has_mirror {
        "<a href=\"/mirror\" style=\"margin-left:auto\">Remote Mirror</a>"
    } else {
        ""
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>/{heading}</title>\
<style>body{{font-family:system-ui,sans-serif;max-width:720px;margin:2rem auto;padding:0 1rem}}\
ul{{list-style:none;padding:0}}li{{padding:.35rem 0;border-bottom:1px solid #eee}}\
a{{text-decoration:none;color:#0b57d0}}small{{color:#666;margin-left:.5rem}}\
.up{{margin:1rem 0;padding:.75rem;background:#f5f5f5;border-radius:8px;display:flex;gap:.5rem;flex-wrap:wrap;align-items:center}}</style>\
</head><body><h2>Aweyuki Share: /{heading}</h2>\
<div class=\"up\"><input type=\"file\" id=\"f\"><button type=\"button\" onclick=\"upload()\">Upload</button><span id=\"s\"></span>{mirror_link}</div>\
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

fn write_mirror_page(stream: &mut TcpStream, (w, h): (u32, u32)) -> Result<()> {
    let (aw, ah) = (w.max(1), h.max(1));
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>Aweyuki Mirror</title>\
<style>body{{font-family:system-ui,sans-serif;background:#111;color:#eee;margin:0;padding:1rem;display:flex;flex-direction:column;align-items:center;gap:.75rem}}\
.bar{{display:flex;gap:.5rem;flex-wrap:wrap;align-items:center}}\
button,a.btn{{background:#2b2b2b;color:#fff;border:1px solid #444;border-radius:8px;padding:.45rem .85rem;font-size:.9rem;cursor:pointer;text-decoration:none}}\
button:active{{background:#0b57d0}}\
#scr{{max-height:82vh;max-width:95vw;aspect-ratio:{aw}/{ah};border-radius:14px;border:1px solid #333;touch-action:none;user-select:none;-webkit-user-drag:none;background:#000}}</style>\
</head><body>\
<div class=\"bar\"><button type=\"button\" onclick=\"k('back')\">Back</button>\
<button type=\"button\" onclick=\"k('volume_down')\">Vol-</button>\
<button type=\"button\" onclick=\"k('volume_up')\">Vol+</button>\
<button type=\"button\" onclick=\"k('power')\">Power</button>\
<button type=\"button\" onclick=\"k('screenshot')\">Screenshot</button>\
<a class=\"btn\" href=\"/screen.png\" download=\"aweyuki-screenshot.png\" onclick=\"this.download='aweyuki-'+Date.now()+'.png'\">Save PNG</a>\
<a class=\"btn\" href=\"/\">Files</a></div>\
<img id=\"scr\" src=\"/screen.png\" draggable=\"false\" alt=\"Recovery Screen\">\
<script>const img=document.getElementById('scr');let timer=null,drag=null;\
function sched(ms){{clearTimeout(timer);timer=setTimeout(()=>{{img.src='/screen.png?t='+Date.now();}},ms);}}\
img.onload=()=>sched(400);img.onerror=()=>sched(1000);\
async function post(ev){{try{{await fetch('/input',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify(ev)}});}}catch(_){{}}sched(80);}}\
function k(key){{post({{kind:'key',key}});}}\
function norm(e){{const r=img.getBoundingClientRect();return{{x:Math.min(1,Math.max(0,(e.clientX-r.left)/Math.max(1,r.width))),y:Math.min(1,Math.max(0,(e.clientY-r.top)/Math.max(1,r.height))),cx:e.clientX,cy:e.clientY}};}}\
img.addEventListener('pointerdown',e=>{{e.preventDefault();img.setPointerCapture(e.pointerId);drag=norm(e);}});\
img.addEventListener('pointerup',e=>{{if(!drag)return;const end=norm(e);const d=Math.hypot(end.cx-drag.cx,end.cy-drag.cy);\
if(d<12){{post({{kind:'tap',x:end.x,y:end.y}});}}else{{post({{kind:'swipe',x0:drag.x,y0:drag.y,x1:end.x,y1:end.y}});}}drag=null;}});\
img.addEventListener('pointercancel',()=>{{drag=null;}});</script>\
</body></html>"
    );
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\n\r\n",
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
pub fn handle_client(stream: TcpStream, root: &Path) -> Result<()> {
    handle_client_with_mirror(stream, root, None)
}

/// Handle a single HTTP/1.1 connection rooted at `root` with optional mirror endpoints.
///
/// # Errors
///
/// Returns an error on socket or filesystem I/O failures.
pub fn handle_client_with_mirror(mut stream: TcpStream, root: &Path, mirror: Option<&MirrorHooks>) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_target = parts.next().unwrap_or("/");
    let req_path = raw_target.split('?').next().unwrap_or(raw_target);

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

    if let Some(m) = mirror {
        match (method, req_path) {
            ("GET", "/mirror") => return write_mirror_page(&mut stream, m.size),
            ("GET", "/screen.png") => {
                if let Some(png) = (m.frame_png)().filter(|b| !b.is_empty()) {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nCache-Control: no-store\r\nContent-Length: {}\r\n\r\n",
                        png.len()
                    );
                    stream.write_all(header.as_bytes())?;
                    stream.write_all(&png)?;
                } else {
                    stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 11\r\n\r\nUnavailable")?;
                }
                return Ok(());
            }
            ("POST", "/input") => {
                let Some(len) = content_length.filter(|&n| n <= MAX_INPUT_BYTES) else {
                    stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 18\r\n\r\nInvalid Input Size")?;
                    return Ok(());
                };
                let mut body = vec![0u8; len as usize];
                if reader.read_exact(&mut body).is_err() {
                    stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 14\r\n\r\nTruncated Body")?;
                    return Ok(());
                }
                let Ok(ev) = serde_json::from_slice::<RemoteInput>(&body) else {
                    stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 12\r\n\r\nInvalid JSON")?;
                    return Ok(());
                };
                if !ev.is_valid() {
                    stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 15\r\n\r\nInvalid Coords!")?;
                    return Ok(());
                }
                (m.on_input)(ev);
                stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")?;
                return Ok(());
            }
            _ => {}
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
                write_directory_listing(&mut stream, &target, &rel, mirror.is_some())?;
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

    #[test]
    fn mirror_endpoints_serve_png_and_dispatch_remote_input() {
        use std::sync::Mutex;

        let root = TempDir::new("aweyuki-share-mirror");
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        let hooks = MirrorHooks {
            size: (1440, 3200),
            frame_png: Arc::new(|| Some(vec![0x89, b'P', b'N', b'G', 1, 2, 3])),
            on_input: Arc::new(move |ev| recorded.lock().unwrap().push(ev)),
        };
        let server = Server::start_with_mirror(root.0.clone(), "127.0.0.1:0".parse().unwrap(), Some(hooks)).unwrap();
        let addr = server.local_addr();

        let mut c1 = TcpStream::connect(addr).unwrap();
        c1.write_all(b"GET /mirror HTTP/1.1\r\n\r\n").unwrap();
        let mut resp1 = String::new();
        c1.read_to_string(&mut resp1).unwrap();
        assert!(
            resp1.contains("Aweyuki Mirror")
                && resp1.contains("aspect-ratio:1440/3200")
                && resp1.contains("Save PNG")
                && resp1.contains("k('screenshot')"),
            "{resp1}"
        );

        let mut c2 = TcpStream::connect(addr).unwrap();
        c2.write_all(b"GET /screen.png?t=1 HTTP/1.1\r\n\r\n").unwrap();
        let mut resp2 = Vec::new();
        c2.read_to_end(&mut resp2).unwrap();
        assert!(resp2.ends_with(&[0x89, b'P', b'N', b'G', 1, 2, 3]));

        let payloads = [
            r#"{"kind":"tap","x":0.5,"y":0.25}"#,
            r#"{"kind":"swipe","x0":0.1,"y0":0.8,"x1":0.9,"y1":0.8}"#,
            r#"{"kind":"key","key":"volume_down"}"#,
            r#"{"kind":"key","key":"screenshot"}"#,
        ];
        for body in payloads {
            let mut c = TcpStream::connect(addr).unwrap();
            let req = format!(
                "POST /input HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            c.write_all(req.as_bytes()).unwrap();
            let mut resp = String::new();
            c.read_to_string(&mut resp).unwrap();
            assert!(resp.starts_with("HTTP/1.1 204 No Content"), "{resp}");
        }

        let bad = r#"{"kind":"tap","x":null,"y":0.5}"#;
        let mut c_bad = TcpStream::connect(addr).unwrap();
        let req_bad = format!("POST /input HTTP/1.1\r\nContent-Length: {}\r\n\r\n{bad}", bad.len());
        c_bad.write_all(req_bad.as_bytes()).unwrap();
        let mut resp_bad = String::new();
        c_bad.read_to_string(&mut resp_bad).unwrap();
        assert!(resp_bad.starts_with("HTTP/1.1 400 Bad Request"), "{resp_bad}");

        let got = events.lock().unwrap().clone();
        assert_eq!(
            got,
            vec![
                RemoteInput::Tap { x: 0.5, y: 0.25 },
                RemoteInput::Swipe { x0: 0.1, y0: 0.8, x1: 0.9, y1: 0.8 },
                RemoteInput::Key { key: RemoteKey::VolumeDown },
                RemoteInput::Key { key: RemoteKey::Screenshot },
            ]
        );
        server.stop();
    }
}
