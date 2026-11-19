//! Lightweight HTTP/1.1 file-sharing server for transferring files over
//! Wi-Fi / USB-NCM without needing MTP or adb.

use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

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
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let slash = if entry.path().is_dir() { "/" } else { "" };
        let encoded = percent_encode_segment(&name);
        let href = if prefix.is_empty() {
            format!("/{encoded}{slash}")
        } else {
            format!("/{}/{encoded}{slash}", prefix.join("/"))
        };
        let label = escape_html(&name);
        let _ = writeln!(rows, "<li><a href=\"{href}\">{label}{slash}</a></li>");
    }

    let heading = escape_html(&rel.display().to_string());
    let body = format!("<!doctype html><html><body><h2>/{heading}</h2><ul>\n{rows}</ul></body></html>");
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
}
