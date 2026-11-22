//! Native AERA `.aerap` plugin bundle parser, installer, `AERAWEB1` XZ
//! runtime unpacker, and wire protocol definitions for AERA Browser & Telegram.

use anyhow::{bail, ensure, Context, Result};
use liblzma::read::XzDecoder;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use zip::ZipArchive;

/// Official AERA Browser (v1.5.6) release bundle URL and filename.
pub const AERA_BROWSER_URL: &str =
    "https://github.com/AERA-Plugins/browser/releases/download/v1.5.6/AERA-Browser-1.5.6.aerap";
pub const AERA_BROWSER_FILE: &str = "AERA-Browser-1.5.6.aerap";

/// Official AERA Telegram (v1.3.5) release bundle URL and filename.
pub const AERA_TELEGRAM_URL: &str =
    "https://github.com/AERA-Plugins/telegram/releases/download/v1.3.5/AERA-Telegram-1.3.5.aerap";
pub const AERA_TELEGRAM_FILE: &str = "AERA-Telegram-1.3.5.aerap";

/// Default locations scanned for installed `.aerap` directories or bundles.
pub const DEFAULT_AERAP_DIRS: &[&str] = &[
    "/sdcard/Aweyuki/plugins",
    "/sdcard/AERA/plugins",
    "/aweyuki/plugins",
    "/sdcard/Download",
];

/// Persistent directory where installed AERA plugin payloads live.
pub const INSTALLED_AERAP_DIR: &str = "/sdcard/Aweyuki/plugins";

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MEMBERS: u32 = 4096;
const COPY_BUF: usize = 128 * 1024;

/// High-level runtime kind supported by Aweyuki's isolated host bridges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AerapKind {
    Browser,
    Telegram,
    Other,
}

/// Parsed top-level `plugin.json` metadata from an AERA `.aerap` plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AerapManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub plugin_type: String,
    pub entry: String,
    pub kind: AerapKind,
    pub payload: String,
    pub payload_size: u64,
    pub payload_sha256: String,
    pub expanded_size: u64,
    pub expanded_sha256: String,
    pub member_count: u32,
}

/// Where an [`AerapPlugin`] was discovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AerapSource {
    InstalledDir(PathBuf),
    BundleFile(PathBuf),
}

/// Discovered AERA `.aerap` plugin ready to install or launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AerapPlugin {
    pub manifest: AerapManifest,
    pub source: AerapSource,
}

struct JsonCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonCursor<'a> {
    fn new(src: &'a str) -> Self {
        Self { bytes: src.as_bytes(), pos: 0 }
    }

    fn skip_ws(&mut self) {
        while let Some(&b) = self.bytes.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, want: u8) -> Result<()> {
        self.skip_ws();
        let got = self.bytes.get(self.pos).copied().context("unexpected end of JSON")?;
        ensure!(got == want, "expected '{}' in JSON near byte {}", char::from(want), self.pos);
        self.pos += 1;
        Ok(())
    }

    fn read_hex4(&mut self) -> Result<u16> {
        ensure!(self.pos + 4 <= self.bytes.len(), "truncated \\uXXXX escape in JSON");
        let hex_str = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])?;
        self.pos += 4;
        u16::from_str_radix(hex_str, 16).context("invalid \\uXXXX hex digit in JSON")
    }

    fn read_string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(out);
            }
            if b == b'\\' {
                self.pos += 1;
                let esc = *self.bytes.get(self.pos).context("unterminated JSON escape")?;
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let code = self.read_hex4()?;
                        if (0xD800..=0xDBFF).contains(&code)
                            && self.bytes.get(self.pos..self.pos + 2) == Some(b"\\u")
                        {
                            self.pos += 2;
                            let low = self.read_hex4()?;
                            ensure!((0xDC00..=0xDFFF).contains(&low), "invalid UTF-16 surrogate pair");
                            let cp = 0x10000 + ((u32::from(code - 0xD800) << 10) | u32::from(low - 0xDC00));
                            let ch = char::from_u32(cp).context("invalid Unicode scalar value")?;
                            out.push(ch);
                        } else {
                            let ch = char::from_u32(u32::from(code)).context("unpaired UTF-16 code unit")?;
                            out.push(ch);
                        }
                    }
                    other => bail!("unsupported JSON escape '\\{}'", char::from(other)),
                }
            } else if b < 0x80 {
                ensure!(b >= 0x20, "unescaped control byte in JSON string");
                out.push(char::from(b));
                self.pos += 1;
            } else {
                let tail = std::str::from_utf8(&self.bytes[self.pos..]).context("invalid UTF-8 in JSON")?;
                let ch = tail.chars().next().context("unexpected EOF in UTF-8 char")?;
                out.push(ch);
                self.pos += ch.len_utf8();
            }
        }
        bail!("unterminated JSON string");
    }

    fn read_u64(&mut self) -> Result<u64> {
        self.skip_ws();
        let start = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        ensure!(self.pos > start, "expected unsigned integer in JSON at byte {}", self.pos);
        std::str::from_utf8(&self.bytes[start..self.pos])?.parse::<u64>().context("integer overflow")
    }

    fn skip_value(&mut self) -> Result<()> {
        match self.peek().context("unexpected EOF in JSON value")? {
            b'"' => {
                let _ = self.read_string()?;
            }
            b'{' => {
                self.pos += 1;
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(());
                }
                loop {
                    let _ = self.read_string()?;
                    self.expect(b':')?;
                    self.skip_value()?;
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            break;
                        }
                        _ => bail!("malformed nested JSON object near byte {}", self.pos),
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Ok(());
                }
                loop {
                    self.skip_value()?;
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {
                            self.pos += 1;
                            break;
                        }
                        _ => bail!("malformed JSON array near byte {}", self.pos),
                    }
                }
            }
            _ => {
                while let Some(&b) = self.bytes.get(self.pos) {
                    if matches!(b, b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n') {
                        break;
                    }
                    self.pos += 1;
                }
            }
        }
        Ok(())
    }
}

fn valid_aerap_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=64).contains(&bytes.len())
        && !id.contains("..")
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && (bytes[bytes.len() - 1].is_ascii_lowercase() || bytes[bytes.len() - 1].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
}

fn valid_sha256_hex(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parse top-level AERA `plugin.json` metadata without requiring an external
/// JSON parser crate.
///
/// # Errors
///
/// Returns an error if the JSON is malformed, `schema != 1`, the plugin `id`,
/// `payload`, size bounds, or SHA-256 digests fail validation.
pub fn parse_plugin_json(text: &str) -> Result<AerapManifest> {
    let mut c = JsonCursor::new(text);
    c.expect(b'{')?;
    let mut schema = None;
    let mut id = String::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut description = String::new();
    let mut plugin_type = String::new();
    let mut entry = String::new();
    let mut payload = String::new();
    let mut payload_size = 0u64;
    let mut payload_sha256 = String::new();
    let mut expanded_size = 0u64;
    let mut expanded_sha256 = String::new();
    let mut member_count = 0u32;

    if c.peek() != Some(b'}') {
        loop {
            let key = c.read_string()?;
            c.expect(b':')?;
            match key.as_str() {
                "schema" => schema = Some(c.read_u64()?),
                "id" => id = c.read_string()?,
                "name" => name = c.read_string()?,
                "version" => version = c.read_string()?,
                "description" => description = c.read_string()?,
                "type" => plugin_type = c.read_string()?,
                "entry" => entry = c.read_string()?,
                "payload" => payload = c.read_string()?,
                "payload_size" => payload_size = c.read_u64()?,
                "payload_sha256" => payload_sha256 = c.read_string()?.to_ascii_lowercase(),
                "expanded_size" => expanded_size = c.read_u64()?,
                "expanded_sha256" => expanded_sha256 = c.read_string()?.to_ascii_lowercase(),
                "member_count" => {
                    let v = c.read_u64()?;
                    member_count = u32::try_from(v).context("member_count exceeds u32")?;
                }
                _ => c.skip_value()?,
            }
            match c.peek() {
                Some(b',') => c.pos += 1,
                Some(b'}') => break,
                _ => bail!("malformed top-level plugin.json near byte {}", c.pos),
            }
        }
    }
    c.expect(b'}')?;

    ensure!(schema == Some(1), "unsupported AERA plugin schema {schema:?}");
    ensure!(valid_aerap_id(&id), "unsafe AERA plugin id {id:?}");
    ensure!(!name.is_empty() && !entry.is_empty(), "plugin.json missing name or entry");
    ensure!(payload == "runtime.xz", "unsupported AERA payload {payload:?}");
    ensure!(
        payload_size > 0 && payload_size <= MAX_PAYLOAD_BYTES,
        "invalid payload_size {payload_size}"
    );
    ensure!(
        expanded_size > 0 && expanded_size <= MAX_PAYLOAD_BYTES,
        "invalid expanded_size {expanded_size}"
    );
    ensure!(
        member_count > 0 && member_count <= MAX_MEMBERS,
        "invalid member_count {member_count}"
    );
    ensure!(valid_sha256_hex(&payload_sha256), "invalid payload_sha256");
    ensure!(valid_sha256_hex(&expanded_sha256), "invalid expanded_sha256");

    let kind = if plugin_type == "browser-runtime" && entry == "browser" {
        AerapKind::Browser
    } else if plugin_type == "app-runtime" && entry == "telegram" {
        AerapKind::Telegram
    } else {
        AerapKind::Other
    };

    Ok(AerapManifest {
        id,
        name,
        version,
        description,
        plugin_type,
        entry,
        kind,
        payload,
        payload_size,
        payload_sha256,
        expanded_size,
        expanded_sha256,
        member_count,
    })
}

/// Read and validate `plugin.json` inside a `.aerap` ZIP archive.
///
/// # Errors
///
/// Returns an error if the ZIP cannot be opened or `plugin.json` is missing/invalid.
pub fn inspect_bundle(path: &Path) -> Result<AerapManifest> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut zip = ZipArchive::new(BufReader::new(f)).with_context(|| format!("read zip {}", path.display()))?;
    let entry = zip.by_name("plugin.json").context("plugin.json missing in .aerap archive")?;
    ensure!(entry.size() <= MAX_MANIFEST_BYTES as u64, "plugin.json exceeds 64 KiB cap");
    let mut text = String::new();
    entry
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .context("read plugin.json from archive")?;
    parse_plugin_json(&text)
}

/// Read and validate an already-installed plugin directory (`plugin.json` + `runtime.xz`).
///
/// # Errors
///
/// Returns an error if `plugin.json` is invalid or `runtime.xz` does not exist.
pub fn inspect_dir(dir: &Path) -> Result<AerapManifest> {
    let manifest_path = dir.join("plugin.json");
    let meta = fs::metadata(&manifest_path).with_context(|| format!("stat {}", manifest_path.display()))?;
    ensure!(meta.len() <= MAX_MANIFEST_BYTES as u64, "plugin.json exceeds 64 KiB cap");
    let text = fs::read_to_string(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest = parse_plugin_json(&text)?;
    ensure!(dir.join(&manifest.payload).is_file(), "{} missing in {}", manifest.payload, dir.display());
    Ok(manifest)
}

/// Discover installed `.aerap` plugin directories and `.aerap` ZIP bundles across `dirs`,
/// preferring installed directories over unextracted bundles and earlier dirs over later ones.
#[must_use]
pub fn discover(dirs: &[&Path]) -> Vec<AerapPlugin> {
    let mut by_id: BTreeMap<String, AerapPlugin> = BTreeMap::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Ok(manifest) = inspect_dir(&path)
            {
                let plugin = AerapPlugin { manifest, source: AerapSource::InstalledDir(path) };
                by_id
                    .entry(plugin.manifest.id.clone())
                    .and_modify(|cur| {
                        if matches!(cur.source, AerapSource::BundleFile(_)) {
                            *cur = plugin.clone();
                        }
                    })
                    .or_insert(plugin);
            } else if path.is_file()
                && path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("aerap"))
                && let Ok(manifest) = inspect_bundle(&path)
            {
                by_id
                    .entry(manifest.id.clone())
                    .or_insert_with(|| AerapPlugin { manifest, source: AerapSource::BundleFile(path) });
            }
        }
    }
    by_id.into_values().collect()
}

/// Verify and copy `plugin.json` + `runtime.xz` from `bundle` into `<plugins_root>/<id>/`.
///
/// # Errors
///
/// Returns an error if cancelled, the archive is malformed, or the SHA-256 digest mismatches.
pub fn install_bundle(
    bundle: &Path,
    plugins_root: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<AerapPlugin> {
    let f = File::open(bundle).with_context(|| format!("open {}", bundle.display()))?;
    let mut zip = ZipArchive::new(BufReader::new(f)).with_context(|| format!("read zip {}", bundle.display()))?;

    let manifest_bytes = {
        let mut entry = zip.by_name("plugin.json").context("plugin.json missing in .aerap")?;
        ensure!(entry.size() <= MAX_MANIFEST_BYTES as u64, "plugin.json exceeds 64 KiB cap");
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).context("read plugin.json")?;
        buf
    };
    let manifest_text = std::str::from_utf8(&manifest_bytes).context("plugin.json is not UTF-8")?;
    let manifest = parse_plugin_json(manifest_text)?;

    let sig_bytes = match zip.by_name("plugin.json.sig") {
        Ok(mut sig) if sig.size() <= 4096 => {
            let mut buf = Vec::new();
            sig.read_to_end(&mut buf)?;
            Some(buf)
        }
        _ => None,
    };

    let dest_dir = plugins_root.join(&manifest.id);
    fs::create_dir_all(&dest_dir).with_context(|| format!("create {}", dest_dir.display()))?;

    let part_payload = dest_dir.join("runtime.xz.part");
    let final_payload = dest_dir.join("runtime.xz");
    let _ = fs::remove_file(&part_payload);

    let copy_res = (|| -> Result<()> {
        let mut entry = zip.by_name("runtime.xz").context("runtime.xz missing in .aerap")?;
        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .open(&part_payload)
            .with_context(|| format!("create {}", part_payload.display()))?;

        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; COPY_BUF];
        let mut written = 0u64;
        progress(0, manifest.payload_size);

        loop {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let n = entry.read(&mut buf).context("read runtime.xz from bundle")?;
            if n == 0 {
                break;
            }
            written += n as u64;
            ensure!(written <= manifest.payload_size, "runtime.xz larger than payload_size");
            out.write_all(&buf[..n]).context("write runtime.xz")?;
            hasher.update(&buf[..n]);
            progress(written, manifest.payload_size);
        }
        out.sync_all().context("sync runtime.xz")?;
        ensure!(written == manifest.payload_size, "runtime.xz truncated ({written} != {})", manifest.payload_size);
        let actual_sha = crate::abl::hex_lower(&hasher.finalize());
        ensure!(
            actual_sha == manifest.payload_sha256,
            "runtime.xz SHA-256 mismatch ({actual_sha} != {})",
            manifest.payload_sha256
        );
        Ok(())
    })();

    if let Err(e) = copy_res {
        let _ = fs::remove_file(&part_payload);
        return Err(e);
    }

    fs::rename(&part_payload, &final_payload).with_context(|| format!("finalize {}", final_payload.display()))?;
    fs::write(dest_dir.join("plugin.json"), &manifest_bytes).context("write plugin.json")?;
    if let Some(sig) = sig_bytes {
        fs::write(dest_dir.join("plugin.json.sig"), sig).context("write plugin.json.sig")?;
    }

    Ok(AerapPlugin {
        manifest,
        source: AerapSource::InstalledDir(dest_dir),
    })
}

struct HashLimitReader<'a, R> {
    inner: R,
    hasher: Sha256,
    read_bytes: u64,
    max_bytes: u64,
    cancel: &'a AtomicBool,
}

impl<R: Read> Read for HashLimitReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = self.inner.read(buf)?;
        self.read_bytes += n as u64;
        if self.read_bytes > self.max_bytes {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "compressed runtime exceeded payload_size"));
        }
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

struct ExpandedReader<'a, R> {
    inner: R,
    hasher: Sha256,
    total: u64,
    expected_bytes: u64,
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(u64, u64),
}

impl<R: Read> Read for ExpandedReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = self.inner.read(buf)?;
        self.total += n as u64;
        if self.total > self.expected_bytes {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "expanded runtime exceeded expanded_size"));
        }
        self.hasher.update(&buf[..n]);
        (self.progress)(self.total, self.expected_bytes);
        Ok(n)
    }
}

fn validate_member_rel(rel: &str) -> Result<&Path> {
    ensure!(!rel.is_empty() && rel.len() < 240 && !rel.starts_with('/') && !rel.ends_with('/'), "unsafe path {rel:?}");
    for part in rel.split('/') {
        ensure!(!part.is_empty() && part != "." && part != "..", "unsafe path component in {rel:?}");
    }
    Ok(Path::new(rel))
}

struct CleanupDir(Option<PathBuf>);

impl Drop for CleanupDir {
    fn drop(&mut self) {
        if let Some(dir) = self.0.take() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn unpack_stream<R: Read>(
    source: R,
    manifest: &AerapManifest,
    dest_dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let comp_reader = HashLimitReader {
        inner: source,
        hasher: Sha256::new(),
        read_bytes: 0,
        max_bytes: manifest.payload_size,
        cancel,
    };
    let xz = XzDecoder::new(comp_reader);
    let mut reader = ExpandedReader {
        inner: xz,
        hasher: Sha256::new(),
        total: 0,
        expected_bytes: manifest.expanded_size,
        cancel,
        progress,
    };

    let mut hdr = [0u8; 12];
    reader.read_exact(&mut hdr).context("read AERAWEB1 header")?;
    ensure!(&hdr[..8] == b"AERAWEB1", "invalid runtime magic (expected AERAWEB1)");
    let count = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    ensure!(count == manifest.member_count, "member_count mismatch ({count} != {})", manifest.member_count);

    let mut seen = HashSet::with_capacity(count as usize);
    let mut aliases: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut chunk = vec![0u8; COPY_BUF];

    for _ in 0..count {
        reader.read_exact(&mut hdr).context("read AERAWEB1 member header")?;
        let name_len = usize::from(u16::from_le_bytes([hdr[0], hdr[1]]));
        let mode = u16::from_le_bytes([hdr[2], hdr[3]]);
        let size = u64::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7], hdr[8], hdr[9], hdr[10], hdr[11]]);
        ensure!(name_len > 0 && name_len < 240, "invalid member name length {name_len}");
        ensure!(matches!(mode, 0 | 0o644 | 0o755), "unsupported member mode 0o{mode:o}");
        ensure!(size <= manifest.expanded_size, "oversize member {size}");

        let mut name_buf = vec![0u8; name_len];
        reader.read_exact(&mut name_buf).context("read member path")?;
        let name = std::str::from_utf8(&name_buf).context("non-UTF8 member path")?;
        let rel = validate_member_rel(name)?.to_path_buf();
        ensure!(seen.insert(name.to_string()), "duplicate member {name:?}");

        let pad = (4 - (reader.total % 4) as usize) % 4;
        if pad > 0 {
            let mut zeros = [0u8; 3];
            reader.read_exact(&mut zeros[..pad]).context("read member alignment")?;
            ensure!(zeros[..pad].iter().all(|&b| b == 0), "non-zero member alignment padding");
        }

        if mode == 0 {
            ensure!(size > 0 && size < 240, "invalid alias target length {size}");
            let mut tgt_buf = vec![0u8; size as usize];
            reader.read_exact(&mut tgt_buf).context("read alias target")?;
            let tgt = std::str::from_utf8(&tgt_buf).context("non-UTF8 alias target")?;
            let tgt_rel = validate_member_rel(tgt)?.to_path_buf();
            aliases.push((rel, tgt_rel));
            continue;
        }

        let target = dest_dir.join(&rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(u32::from(mode))
            .open(&target)
            .with_context(|| format!("create {}", target.display()))?;
        let mut rem = size;
        while rem > 0 {
            let want = usize::try_from(rem.min(chunk.len() as u64)).expect("bounded by COPY_BUF");
            reader.read_exact(&mut chunk[..want]).context("read member payload")?;
            out.write_all(&chunk[..want]).with_context(|| format!("write {}", target.display()))?;
            rem -= want as u64;
        }
        out.sync_data().ok();
        fs::set_permissions(&target, fs::Permissions::from_mode(u32::from(mode)))?;
    }

    let mut trailing = [0u8; 1];
    ensure!(reader.read(&mut trailing)? == 0, "unexpected trailing data in runtime stream");
    ensure!(
        reader.total == manifest.expanded_size,
        "expanded size mismatch ({} != {})",
        reader.total,
        manifest.expanded_size
    );
    let expanded_sha = crate::abl::hex_lower(&reader.hasher.finalize());
    ensure!(
        expanded_sha == manifest.expanded_sha256,
        "expanded SHA-256 mismatch ({expanded_sha} != {})",
        manifest.expanded_sha256
    );

    let comp = reader.inner.into_inner();
    ensure!(
        comp.read_bytes == manifest.payload_size,
        "compressed size mismatch ({} != {})",
        comp.read_bytes,
        manifest.payload_size
    );
    let comp_sha = crate::abl::hex_lower(&comp.hasher.finalize());
    ensure!(
        comp_sha == manifest.payload_sha256,
        "compressed SHA-256 mismatch ({comp_sha} != {})",
        manifest.payload_sha256
    );

    for (alias, target_rel) in aliases {
        let src = dest_dir.join(&target_rel);
        let dst = dest_dir.join(&alias);
        let meta = fs::symlink_metadata(&src).with_context(|| format!("missing alias target {}", src.display()))?;
        ensure!(meta.file_type().is_file(), "alias target {} is not a regular file", src.display());
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::hard_link(&src, &dst).with_context(|| format!("hard link {} -> {}", dst.display(), src.display()))?;
    }
    Ok(())
}

/// Verify and unpack `plugin`'s `runtime.xz` payload into a private `0700` staging
/// folder inside `staging_parent`.
///
/// # Errors
///
/// Returns an error if cancelled, compressed/expanded SHA-256 or byte counts fail
/// verification, or unpacking fails (removing the partial directory automatically).
pub fn unpack_runtime(
    plugin: &AerapPlugin,
    staging_parent: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf> {
    fs::create_dir_all(staging_parent).with_context(|| format!("create {}", staging_parent.display()))?;
    let prefix = match plugin.manifest.kind {
        AerapKind::Browser => "aera-web",
        AerapKind::Telegram => "aera-tg",
        AerapKind::Other => "aera-plug",
    };
    let dest = staging_parent.join(format!("{prefix}-{:06x}", std::process::id() & 0xff_ffff));
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).with_context(|| format!("create {}", dest.display()))?;
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o700))?;
    let mut guard = CleanupDir(Some(dest.clone()));

    progress(0, plugin.manifest.expanded_size);
    match &plugin.source {
        AerapSource::InstalledDir(dir) => {
            let f = File::open(dir.join(&plugin.manifest.payload))
                .with_context(|| format!("open {}/{}", dir.display(), plugin.manifest.payload))?;
            unpack_stream(BufReader::new(f), &plugin.manifest, &dest, cancel, progress)?;
        }
        AerapSource::BundleFile(bundle) => {
            let f = File::open(bundle).with_context(|| format!("open {}", bundle.display()))?;
            let mut zip = ZipArchive::new(BufReader::new(f)).with_context(|| format!("read zip {}", bundle.display()))?;
            let entry = zip
                .by_name(&plugin.manifest.payload)
                .with_context(|| format!("{} missing in {}", plugin.manifest.payload, bundle.display()))?;
            unpack_stream(entry, &plugin.manifest, &dest, cancel, progress)?;
        }
    }

    guard.0 = None;
    Ok(dest)
}

/// Wire protocol constants and frames for the AERA Browser worker (`--isolated-ipc-v1`).
pub mod browser_proto {
    pub const MAGIC: u32 = 0x4152_5032;
    pub const WIDTH: u32 = 1080;
    pub const HEIGHT: u32 = 2100;
    pub const VIEW_WIDTH: i32 = 360;
    pub const VIEW_HEIGHT: i32 = 700;
    pub const FRAME_BYTES: usize = (WIDTH as usize) * (HEIGHT as usize) * 4;
    pub const FRAME_SLOTS: usize = 2;
    pub const SHARED_BYTES: usize = FRAME_BYTES * FRAME_SLOTS;
    pub const MSG_BYTES: usize = 2072;
    pub const TEXT_BYTES: usize = 2048;

    #[repr(u32)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum BrowserKind {
        Open = 1,
        Back = 2,
        Forward = 3,
        Reload = 4,
        Stop = 5,
        TouchDown = 6,
        TouchMove = 7,
        TouchUp = 8,
        Key = 9,
        Ack = 10,
        Close = 11,
        SetZoom = 12,
        DownloadCancel = 13,
        SetCookiePolicy = 14,
        ClearBrowsingData = 15,
        Frame = 32,
        Status = 33,
        Error = 34,
        KeyboardShow = 35,
        KeyboardHide = 36,
        DownloadStarted = 37,
        DownloadProgress = 38,
        DownloadFinished = 39,
        DownloadFailed = 40,
        DownloadCancelled = 41,
        BrowsingDataCleared = 42,
        BrowsingDataFailed = 43,
    }

    impl BrowserKind {
        fn from_u32(v: u32) -> Option<Self> {
            Some(match v {
                1 => Self::Open,
                2 => Self::Back,
                3 => Self::Forward,
                4 => Self::Reload,
                5 => Self::Stop,
                6 => Self::TouchDown,
                7 => Self::TouchMove,
                8 => Self::TouchUp,
                9 => Self::Key,
                10 => Self::Ack,
                11 => Self::Close,
                12 => Self::SetZoom,
                13 => Self::DownloadCancel,
                14 => Self::SetCookiePolicy,
                15 => Self::ClearBrowsingData,
                32 => Self::Frame,
                33 => Self::Status,
                34 => Self::Error,
                35 => Self::KeyboardShow,
                36 => Self::KeyboardHide,
                37 => Self::DownloadStarted,
                38 => Self::DownloadProgress,
                39 => Self::DownloadFinished,
                40 => Self::DownloadFailed,
                41 => Self::DownloadCancelled,
                42 => Self::BrowsingDataCleared,
                43 => Self::BrowsingDataFailed,
                _ => return None,
            })
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct BrowserMsg {
        pub kind: BrowserKind,
        pub sequence: u32,
        pub x: i32,
        pub y: i32,
        pub value: u32,
        pub text: String,
    }

    impl BrowserMsg {
        #[must_use]
        pub fn cmd(kind: BrowserKind) -> Self {
            Self { kind, sequence: 0, x: 0, y: 0, value: 0, text: String::new() }
        }

        #[must_use]
        pub fn open(url: &str) -> Self {
            Self { kind: BrowserKind::Open, sequence: 0, x: 0, y: 0, value: 0, text: url.to_string() }
        }

        #[must_use]
        pub fn ack(sequence: u32) -> Self {
            Self { kind: BrowserKind::Ack, sequence, x: 0, y: 0, value: 0, text: String::new() }
        }

        #[must_use]
        pub fn encode(&self) -> [u8; MSG_BYTES] {
            let mut out = [0u8; MSG_BYTES];
            out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
            out[4..8].copy_from_slice(&(self.kind as u32).to_le_bytes());
            out[8..12].copy_from_slice(&self.sequence.to_le_bytes());
            out[12..16].copy_from_slice(&self.x.to_le_bytes());
            out[16..20].copy_from_slice(&self.y.to_le_bytes());
            out[20..24].copy_from_slice(&self.value.to_le_bytes());
            let mut len = self.text.len().min(TEXT_BYTES - 1);
            while !self.text.is_char_boundary(len) {
                len -= 1;
            }
            out[24..24 + len].copy_from_slice(&self.text.as_bytes()[..len]);
            out
        }

        #[must_use]
        pub fn decode(buf: &[u8]) -> Option<Self> {
            if buf.len() != MSG_BYTES {
                return None;
            }
            let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            if magic != MAGIC {
                return None;
            }
            let kind = BrowserKind::from_u32(u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]))?;
            let sequence = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
            let x = i32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
            let y = i32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]);
            let value = u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]);
            let raw_text = &buf[24..MSG_BYTES];
            let nul = raw_text.iter().position(|&b| b == 0)?;
            let text = String::from_utf8_lossy(&raw_text[..nul]).into_owned();
            Some(Self { kind, sequence, x, y, value, text })
        }
    }

    #[must_use]
    pub fn frame_slot(sequence: u32) -> usize {
        (sequence as usize) % FRAME_SLOTS
    }

    /// Normalize a user-entered browser address safely, rejecting `file:`,
    /// `javascript:`, control bytes, and authority userinfo.
    #[must_use]
    pub fn normalize_url(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 2040 {
            return None;
        }
        if matches!(trimmed, "aera://start" | "aera://test") {
            return Some(trimmed.to_string());
        }
        if trimmed.bytes().any(|b| b <= 32 || b == 127 || b == b'\\') {
            return None;
        }
        let with_scheme = if trimmed.contains("://") {
            trimmed.to_string()
        } else {
            if trimmed.contains(':') {
                return None;
            }
            format!("https://{trimmed}")
        };
        let rest = with_scheme
            .strip_prefix("https://")
            .or_else(|| with_scheme.strip_prefix("http://"))?;
        if with_scheme.len() >= TEXT_BYTES {
            return None;
        }
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        if authority.is_empty() || authority.contains('@') {
            return None;
        }
        Some(with_scheme)
    }
}

/// Wire protocol constants and messages for the AERA Telegram TDLib worker.
pub mod telegram_proto {
    pub const MAGIC: u32 = 0x4154_474D;
    pub const VERSION: u32 = 1;
    pub const MSG_BYTES: usize = 2080;
    pub const TEXT_BYTES: usize = 2048;

    #[repr(u32)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TgKind {
        Configure = 1,
        Phone = 2,
        Code = 3,
        Password = 4,
        Email = 5,
        EmailCode = 6,
        Register = 7,
        LoadChats = 8,
        OpenChat = 9,
        SendText = 10,
        Close = 11,
        SendFile = 12,
        DownloadFile = 13,
        EditMessage = 14,
        DeleteMessage = 15,
        ChangeVaultPassword = 16,
        State = 64,
        Status = 65,
        Error = 66,
        Chat = 67,
        ChatsDone = 68,
        Message = 69,
        MessagesDone = 70,
        Attachment = 71,
        FileProgress = 72,
        FileReady = 73,
        PhotoReady = 74,
        AvatarReady = 75,
        MessageStatus = 76,
        MessageDeleted = 77,
        VaultPasswordResult = 78,
    }

    impl TgKind {
        fn from_u32(v: u32) -> Option<Self> {
            Some(match v {
                1 => Self::Configure,
                2 => Self::Phone,
                3 => Self::Code,
                4 => Self::Password,
                5 => Self::Email,
                6 => Self::EmailCode,
                7 => Self::Register,
                8 => Self::LoadChats,
                9 => Self::OpenChat,
                10 => Self::SendText,
                11 => Self::Close,
                12 => Self::SendFile,
                13 => Self::DownloadFile,
                14 => Self::EditMessage,
                15 => Self::DeleteMessage,
                16 => Self::ChangeVaultPassword,
                64 => Self::State,
                65 => Self::Status,
                66 => Self::Error,
                67 => Self::Chat,
                68 => Self::ChatsDone,
                69 => Self::Message,
                70 => Self::MessagesDone,
                71 => Self::Attachment,
                72 => Self::FileProgress,
                73 => Self::FileReady,
                74 => Self::PhotoReady,
                75 => Self::AvatarReady,
                76 => Self::MessageStatus,
                77 => Self::MessageDeleted,
                78 => Self::VaultPasswordResult,
                _ => return None,
            })
        }
    }

    #[repr(u32)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum TgAuthState {
        #[default]
        Starting = 0,
        NeedConfiguration = 1,
        NeedPhone = 2,
        NeedCode = 3,
        NeedPassword = 4,
        NeedEmail = 5,
        NeedEmailCode = 6,
        NeedRegistration = 7,
        ConfirmElsewhere = 8,
        Ready = 9,
        Closing = 10,
        NeedVault = 11,
    }

    impl TgAuthState {
        #[must_use]
        pub fn from_u32(v: u32) -> Self {
            match v {
                1 => Self::NeedConfiguration,
                2 => Self::NeedPhone,
                3 => Self::NeedCode,
                4 => Self::NeedPassword,
                5 => Self::NeedEmail,
                6 => Self::NeedEmailCode,
                7 => Self::NeedRegistration,
                8 => Self::ConfirmElsewhere,
                9 => Self::Ready,
                10 => Self::Closing,
                11 => Self::NeedVault,
                _ => Self::Starting,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TgMsg {
        pub kind: TgKind,
        pub value: u32,
        pub primary: i64,
        pub secondary: i64,
        pub text: String,
    }

    impl TgMsg {
        #[must_use]
        pub fn simple(kind: TgKind, text: impl Into<String>) -> Self {
            Self { kind, value: 0, primary: 0, secondary: 0, text: text.into() }
        }

        #[must_use]
        pub fn encode(&self) -> [u8; MSG_BYTES] {
            let mut out = [0u8; MSG_BYTES];
            out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
            out[4..8].copy_from_slice(&VERSION.to_le_bytes());
            out[8..12].copy_from_slice(&(self.kind as u32).to_le_bytes());
            out[12..16].copy_from_slice(&self.value.to_le_bytes());
            out[16..24].copy_from_slice(&self.primary.to_le_bytes());
            out[24..32].copy_from_slice(&self.secondary.to_le_bytes());
            let mut len = self.text.len().min(TEXT_BYTES - 1);
            while !self.text.is_char_boundary(len) {
                len -= 1;
            }
            out[32..32 + len].copy_from_slice(&self.text.as_bytes()[..len]);
            out
        }

        #[must_use]
        pub fn decode(buf: &[u8]) -> Option<Self> {
            if buf.len() != MSG_BYTES {
                return None;
            }
            let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            let version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
            if magic != MAGIC || version != VERSION {
                return None;
            }
            let kind = TgKind::from_u32(u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]))?;
            let value = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
            let primary = i64::from_le_bytes([
                buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23],
            ]);
            let secondary = i64::from_le_bytes([
                buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31],
            ]);
            let raw_text = &buf[32..MSG_BYTES];
            let nul = raw_text.iter().position(|&b| b == 0)?;
            let text = String::from_utf8_lossy(&raw_text[..nul]).into_owned();
            Some(Self { kind, value, primary, secondary, text })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::browser_proto::*;
    use super::telegram_proto::*;
    use super::*;
    use liblzma::write::XzEncoder;
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    fn build_aeraweb1(files: &[(&str, u16, &[u8])]) -> (Vec<u8>, String, u64) {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"AERAWEB1");
        raw.extend_from_slice(&(files.len() as u32).to_le_bytes());
        for (name, mode, data) in files {
            raw.extend_from_slice(&(name.len() as u16).to_le_bytes());
            raw.extend_from_slice(&mode.to_le_bytes());
            raw.extend_from_slice(&(data.len() as u64).to_le_bytes());
            raw.extend_from_slice(name.as_bytes());
            let pad = (4 - (raw.len() % 4)) % 4;
            raw.resize(raw.len() + pad, 0);
            raw.extend_from_slice(data);
        }
        let expanded_size = raw.len() as u64;
        let expanded_sha = crate::abl::hex_lower(&Sha256::digest(&raw));

        let mut enc = XzEncoder::new(Vec::new(), 1);
        enc.write_all(&raw).unwrap();
        let compressed = enc.finish().unwrap();
        (compressed, expanded_sha, expanded_size)
    }

    #[test]
    fn parses_plugin_json_with_nested_localizations() {
        let json = r#"{
            "schema": 1,
            "id": "browser",
            "name": "AERA Browser",
            "version": "1.5.6",
            "description": "WebKit browser",
            "type": "browser-runtime",
            "entry": "browser",
            "min_host_api": 1,
            "payload": "runtime.xz",
            "payload_size": 123,
            "payload_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "expanded_size": 456,
            "expanded_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
            "member_count": 3,
            "permissions": ["network", "display"],
            "localizations": {
                "vi_VN": { "name": "AERA Browser", "description": "Trình duyệt \"WebKit\"" }
            }
        }"#;
        let m = parse_plugin_json(json).unwrap();
        assert_eq!(m.id, "browser");
        assert_eq!(m.kind, AerapKind::Browser);
        assert_eq!(m.member_count, 3);
    }

    #[test]
    fn installs_unpacks_with_aliases_and_rejects_corrupted_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let (xz, exp_sha, exp_len) = build_aeraweb1(&[
            ("usr/bin/aera-browser-worker", 0o755, b"#!/bin/sh\necho ok\n"),
            ("usr/lib/libc.so", 0o644, b"ELF-MOCK"),
            ("usr/lib/ld-musl.so.1", 0, b"usr/lib/libc.so"),
        ]);
        let comp_sha = crate::abl::hex_lower(&Sha256::digest(&xz));
        let manifest_json = format!(
            r#"{{
                "schema": 1,
                "id": "browser",
                "name": "AERA Browser",
                "version": "1.5.6",
                "description": "Test bundle",
                "type": "browser-runtime",
                "entry": "browser",
                "payload": "runtime.xz",
                "payload_size": {},
                "payload_sha256": "{comp_sha}",
                "expanded_size": {exp_len},
                "expanded_sha256": "{exp_sha}",
                "member_count": 3
            }}"#,
            xz.len()
        );

        let bundle = tmp.path().join("AERA-Browser.aerap");
        {
            let f = File::create(&bundle).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("plugin.json", opts).unwrap();
            zw.write_all(manifest_json.as_bytes()).unwrap();
            zw.start_file("runtime.xz", opts).unwrap();
            zw.write_all(&xz).unwrap();
            zw.finish().unwrap();
        }

        let cancel = AtomicBool::new(false);
        let plugins_root = tmp.path().join("installed");
        let installed = install_bundle(&bundle, &plugins_root, &cancel, &mut |_, _| {}).unwrap();
        assert!(matches!(installed.source, AerapSource::InstalledDir(_)));

        let stage = tmp.path().join("stage");
        let unpacked = unpack_runtime(&installed, &stage, &cancel, &mut |_, _| {}).unwrap();
        assert_eq!(fs::read(unpacked.join("usr/lib/ld-musl.so.1")).unwrap(), b"ELF-MOCK");
        assert!(unpacked.join("usr/bin/aera-browser-worker").is_file());

        // Corrupted payload must fail SHA-256 check and clean up.
        let mut broken = installed.clone();
        broken.manifest.expanded_sha256 = "f".repeat(64);
        assert!(unpack_runtime(&broken, &stage, &cancel, &mut |_, _| {}).is_err());

        let found = discover(&[plugins_root.as_path(), tmp.path()]);
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].source, AerapSource::InstalledDir(_)));
    }

    #[test]
    fn encodes_and_decodes_browser_and_telegram_wire_frames() {
        let bm = BrowserMsg {
            kind: BrowserKind::TouchMove,
            sequence: 7,
            x: 180,
            y: 420,
            value: 0,
            text: "https://example.org".into(),
        };
        let enc_b = bm.encode();
        assert_eq!(enc_b.len(), 2072);
        assert_eq!(BrowserMsg::decode(&enc_b), Some(bm));
        assert_eq!(normalize_url("example.com"), Some("https://example.com".into()));
        assert_eq!(normalize_url("aera://start"), Some("aera://start".into()));
        assert_eq!(normalize_url("javascript:alert(1)"), None);

        let tm = TgMsg {
            kind: TgKind::SendText,
            value: 1,
            primary: -1001234567890,
            secondary: 42,
            text: "hello".into(),
        };
        let enc_t = tm.encode();
        assert_eq!(enc_t.len(), 2080);
        assert_eq!(TgMsg::decode(&enc_t), Some(tm));
    }
}
