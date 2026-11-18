//! Lightweight HTTP/1.1 file-sharing server for transferring files over
//! Wi-Fi / USB-NCM without needing MTP or adb.

use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

/// Normalize a URL request path into a safe relative path under `root`,
/// rejecting `..` traversal or invalid components.
pub fn sanitize_rel_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.split('?').next().unwrap_or("").trim_start_matches('/');
    let mut out = PathBuf::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." || seg.contains('\0') {
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

/// Start an HTTP file-sharing server rooted at `root` on `addr`.
pub fn serve(root: &Path, addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(addr).with_context(|| format!("bind {addr}"))?;
    let local = listener.local_addr()?;
    println!("aweyuki-share listening on http://{local} (root: {})", root.display());
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let root = root.to_path_buf();
                std::thread::spawn(move || {
                    let _ = handle_client(s, &root);
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    Ok(())
}

pub fn handle_client(mut stream: TcpStream, root: &Path) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_target = parts.next().unwrap_or("/");

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':')
            && k.trim().eq_ignore_ascii_case("Content-Length")
        {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }

    let rel = match sanitize_rel_path(raw_target) {
        Ok(r) => r,
        Err(_) => {
            stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nBad Request")?;
            return Ok(());
        }
    };
    let target = root.join(&rel);

    match method {
        "GET" => {
            if target.is_dir() {
                let mut rows = String::new();
                let mut entries: Vec<_> = fs::read_dir(&target)?.filter_map(Result::ok).collect();
                entries.sort_by_key(|e| e.file_name());
                for e in entries {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let slash = if e.path().is_dir() { "/" } else { "" };
                    let href = if rel.as_os_str().is_empty() {
                        format!("/{name}{slash}")
                    } else {
                        format!("/{}/{name}{slash}", rel.display())
                    };
                    rows.push_str(&format!("<li><a href=\"{href}\">{name}{slash}</a></li>\n"));
                }
                let body = format!("<!doctype html><html><body><h2>/{}</h2><ul>\n{rows}</ul></body></html>", rel.display());
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                stream.write_all(header.as_bytes())?;
                stream.write_all(body.as_bytes())?;
            } else if target.is_file() {
                let mut file = File::open(&target)?;
                let len = file.metadata()?.len();
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {len}\r\n\r\n"
                );
                stream.write_all(header.as_bytes())?;
                let mut buf = [0u8; 64 * 1024];
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    stream.write_all(&buf[..n])?;
                }
            } else {
                stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found")?;
            }
        }
        "PUT" => {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&target)?;
            let mut remaining = content_length;
            let mut buf = [0u8; 64 * 1024];
            while remaining > 0 {
                let to_read = remaining.min(buf.len());
                let n = reader.read(&mut buf[..to_read])?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
                remaining -= n;
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

    #[test]
    fn path_traversal_is_rejected() {
        assert!(sanitize_rel_path("/../etc/passwd").is_err());
        assert!(sanitize_rel_path("a/../../b").is_err());
        assert_eq!(sanitize_rel_path("/Download/rom.zip").unwrap(), PathBuf::from("Download/rom.zip"));
    }

    #[test]
    fn http_put_and_get_round_trip_over_loopback() {
        let root = std::env::temp_dir().join("aweyuki-share-tests");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let srv_root = root.clone();
        std::thread::spawn(move || {
            for s in listener.incoming().take(2).flatten() {
                let _ = handle_client(s, &srv_root);
            }
        });

        let mut c1 = TcpStream::connect(addr).unwrap();
        c1.write_all(b"PUT /hello.txt HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello").unwrap();
        let mut resp1 = String::new();
        c1.read_to_string(&mut resp1).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 201 Created"));

        let mut c2 = TcpStream::connect(addr).unwrap();
        c2.write_all(b"GET /hello.txt HTTP/1.1\r\n\r\n").unwrap();
        let mut resp2 = String::new();
        c2.read_to_string(&mut resp2).unwrap();
        assert!(resp2.ends_with("\r\n\r\nhello"));
    }
}
