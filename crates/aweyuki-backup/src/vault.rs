//! Per-app `.tar.lz4` backup & restore vault (`aera_app_vault`).
//!
//! Discovers installed third-party apps from plain-text `packages.xml` (when
//! available) and `/data/app/**/<pkg>-*/base.apk`, paired with `/data/data/<pkg>`,
//! packing APK + app data (excluding `cache` and `code_cache`) into self-contained
//! `<pkg>-<made>.tar.lz4` archives with SHA-256 `.toml` manifests.

use anyhow::{bail, ensure, Context, Result};
use aweyuki_files::{Cancelled, Ownership};
use lz4_flex::frame::{BlockSize, FrameDecoder, FrameEncoder, FrameInfo};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tar::{Archive, Builder, EntryType, Header};

/// Vault folder relative to the storage root (`/sdcard`).
pub const VAULT_FOLDER: &str = "Aweyuki/app-vault";

/// Manifest schema version read and written by this module.
pub const VAULT_FORMAT: u32 = 1;

const COPY_BUF: usize = 256 * 1024;
const CACHE_DIRS: [&str; 2] = ["cache", "code_cache"];

/// Discovered installed application with its APK directory and optional private data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    pub package: String,
    pub apk_dir: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub apk_bytes: u64,
    pub data_bytes: u64,
}

/// On-disk TOML manifest describing one `.tar.lz4` app backup archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultManifest {
    pub format: u32,
    pub package: String,
    pub made: i64,
    pub device: String,
    pub apk_rel_dir: String,
    pub has_data: bool,
    pub apk_bytes: u64,
    pub data_bytes: u64,
    pub archive_file: String,
    pub archive_bytes: u64,
    pub sha256: String,
}

fn dir_size(root: &Path, skip_top_caches: bool) -> u64 {
    fn walk(dir: &Path, depth: usize, skip_top_caches: bool, sum: &mut u64) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                if skip_top_caches
                    && depth == 0
                    && entry.file_name().to_str().is_some_and(|n| CACHE_DIRS.contains(&n))
                {
                    continue;
                }
                walk(&path, depth + 1, skip_top_caches, sum);
            } else if meta.is_file() {
                *sum = sum.saturating_add(meta.len());
            }
        }
    }
    let mut sum = 0u64;
    if root.is_dir() {
        walk(root, 0, skip_top_caches, &mut sum);
    }
    sum
}

fn package_from_apk_dir(dir_name: &str) -> Option<&str> {
    let pkg = dir_name.split_once('-').map_or(dir_name, |(p, _)| p);
    if !pkg.is_empty()
        && pkg.contains('.')
        && pkg.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_'))
    {
        Some(pkg)
    } else {
        None
    }
}

fn has_apk_file(dir: &Path) -> bool {
    dir.join("base.apk").is_file()
        || fs::read_dir(dir).is_ok_and(|it| {
            it.flatten().any(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"))
            })
        })
}

fn extract_xml_attr<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=\"");
    let idx = line.find(&needle)?;
    let rest = &line[idx + needle.len()..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn scan_packages_xml(data_root: &Path, out: &mut BTreeMap<String, PathBuf>) {
    let xml_path = data_root.join("system/packages.xml");
    let Ok(bytes) = fs::read(&xml_path) else {
        return;
    };
    // Android 12+ often writes ABX binary XML starting with `ABX\0`; skip cleanly.
    if bytes.starts_with(b"ABX") {
        return;
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return;
    };
    let app_root = data_root.join("app");
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("<package ") {
            continue;
        }
        let Some(pkg) = extract_xml_attr(trimmed, "name") else {
            continue;
        };
        let Some(code_path) = extract_xml_attr(trimmed, "codePath") else {
            continue;
        };
        let Some(rel) = code_path.strip_prefix("/data/app/") else {
            continue;
        };
        let candidate = app_root.join(rel.trim_start_matches('/'));
        if candidate.is_dir() && has_apk_file(&candidate) {
            out.entry(pkg.to_string()).or_insert(candidate);
        }
    }
}

fn scan_app_tree(data_root: &Path, out: &mut BTreeMap<String, PathBuf>) {
    let app_root = data_root.join("app");
    let Ok(entries) = fs::read_dir(&app_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("~~") {
            if let Ok(inner) = fs::read_dir(&path) {
                for sub in inner.flatten() {
                    let sub_path = sub.path();
                    if sub_path.is_dir()
                        && has_apk_file(&sub_path)
                        && let Some(pkg) = package_from_apk_dir(&sub.file_name().to_string_lossy())
                    {
                        out.entry(pkg.to_string()).or_insert(sub_path);
                    }
                }
            }
        } else if has_apk_file(&path)
            && let Some(pkg) = package_from_apk_dir(&name)
        {
            out.entry(pkg.to_string()).or_insert(path);
        }
    }
}

fn locate_data_dir(data_root: &Path, pkg: &str) -> Option<PathBuf> {
    let primary = data_root.join("data").join(pkg);
    if primary.is_dir() {
        return Some(primary);
    }
    let user0 = data_root.join("user/0").join(pkg);
    user0.is_dir().then_some(user0)
}

/// Discover installed user apps under `data_root` (`/data`), reading plain-text
/// `system/packages.xml` when present and scanning `/data/app` for Android 11+
/// and legacy APK directories.
///
/// # Errors
///
/// Returns an error only if `data_root` does not exist.
pub fn discover_apps(data_root: &Path) -> Result<Vec<AppEntry>> {
    ensure!(data_root.is_dir(), "{} is not accessible", data_root.display());
    let mut apks = BTreeMap::new();
    scan_packages_xml(data_root, &mut apks);
    scan_app_tree(data_root, &mut apks);

    let apps = apks
        .into_iter()
        .map(|(package, apk_dir)| {
            let apk_bytes = dir_size(&apk_dir, false);
            let data_dir = locate_data_dir(data_root, &package);
            let data_bytes = data_dir.as_deref().map_or(0, |d| dir_size(d, true));
            AppEntry {
                package,
                apk_dir,
                data_dir,
                apk_bytes,
                data_bytes,
            }
        })
        .collect();
    Ok(apps)
}

struct HashWriter<W> {
    inner: W,
    hasher: Sha256,
    written: u64,
}

impl<W: Write> Write for HashWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn pack_tree<W: Write>(
    builder: &mut Builder<W>,
    src_root: &Path,
    archive_prefix: &Path,
    skip_top_caches: bool,
    done: &mut u64,
    total: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    fn walk<W: Write>(
        builder: &mut Builder<W>,
        dir: &Path,
        rel_dir: &Path,
        depth: usize,
        skip_top_caches: bool,
        done: &mut u64,
        total: u64,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let mut entries: Vec<_> = fs::read_dir(dir)
            .with_context(|| format!("read {}", dir.display()))?
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("read {}", dir.display()))?;
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return Err(Cancelled.into());
            }
            let name = entry.file_name();
            if skip_top_caches
                && depth == 0
                && name.to_str().is_some_and(|n| CACHE_DIRS.contains(&n))
            {
                continue;
            }
            let src = entry.path();
            let meta = fs::symlink_metadata(&src).with_context(|| format!("stat {}", src.display()))?;
            let ft = meta.file_type();
            if ft.is_socket() {
                continue;
            }
            let rel = rel_dir.join(&name);
            let mut hdr = Header::new_gnu();
            hdr.set_mode(meta.permissions().mode() & 0o7777);
            hdr.set_uid(u64::from(meta.uid()));
            hdr.set_gid(u64::from(meta.gid()));
            if ft.is_dir() {
                hdr.set_entry_type(EntryType::Directory);
                hdr.set_size(0);
                builder.append_data(&mut hdr, &rel, io::empty())?;
                walk(builder, &src, &rel, depth + 1, skip_top_caches, done, total, cancel, progress)?;
            } else if ft.is_symlink() {
                let target = fs::read_link(&src).with_context(|| format!("readlink {}", src.display()))?;
                hdr.set_entry_type(EntryType::Symlink);
                hdr.set_size(0);
                builder.append_link(&mut hdr, &rel, target)?;
            } else if ft.is_file() {
                hdr.set_entry_type(EntryType::Regular);
                hdr.set_size(meta.len());
                let file = File::open(&src).with_context(|| format!("open {}", src.display()))?;
                struct Reader<'a> {
                    file: File,
                    done: &'a mut u64,
                    total: u64,
                    cancel: &'a AtomicBool,
                    progress: &'a mut dyn FnMut(u64, u64),
                }
                impl Read for Reader<'_> {
                    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                        if self.cancel.load(Ordering::Relaxed) {
                            return Err(io::Error::other(Cancelled));
                        }
                        let n = self.file.read(buf)?;
                        *self.done += n as u64;
                        (self.progress)(*self.done, self.total);
                        Ok(n)
                    }
                }
                builder.append_data(
                    &mut hdr,
                    &rel,
                    Reader { file, done, total, cancel, progress },
                )?;
            }
        }
        Ok(())
    }

    let mut root_hdr = Header::new_gnu();
    root_hdr.set_entry_type(EntryType::Directory);
    root_hdr.set_mode(0o755);
    root_hdr.set_size(0);
    builder.append_data(&mut root_hdr, archive_prefix, io::empty())?;
    walk(builder, src_root, archive_prefix, 0, skip_top_caches, done, total, cancel, progress)
}

fn apk_relative_dir(entry: &AppEntry) -> String {
    let comps: Vec<String> = entry
        .apk_dir
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if let Some(pos) = comps.iter().rposition(|c| c == "app")
        && pos + 1 < comps.len()
    {
        return comps[pos + 1..].join("/");
    }
    entry
        .apk_dir
        .file_name()
        .map_or_else(|| entry.package.clone(), |n| n.to_string_lossy().into_owned())
}

/// Pack `entry` (its APK folder and non-cache app data) into a verified `.tar.lz4`
/// archive and `.toml` manifest inside `vault_dir`.
///
/// # Errors
///
/// Returns an error if cancelled, reading app files fails, or writing the vault
/// archive/manifest fails.
pub fn backup_app(
    entry: &AppEntry,
    vault_dir: &Path,
    device: &str,
    made: i64,
    ownership: Ownership,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<VaultManifest> {
    ensure!(entry.apk_dir.is_dir(), "{} does not exist", entry.apk_dir.display());
    aweyuki_files::make_folders(vault_dir, ownership)?;

    let stem = format!("{}-{made}", entry.package);
    let archive_file = format!("{stem}.tar.lz4");
    let archive_path = vault_dir.join(&archive_file);
    let part_path = vault_dir.join(format!(".{stem}.tar.lz4.part"));
    let _ = fs::remove_file(&part_path);

    let total = entry.apk_bytes.saturating_add(entry.data_bytes).max(1);
    let mut done = 0u64;
    progress(0, total);

    let pack_res = (|| -> Result<(u64, String)> {
        let out_file = aweyuki_files::make_file(&part_path, ownership)?;
        let hw = HashWriter {
            inner: out_file,
            hasher: Sha256::new(),
            written: 0,
        };
        let frame = FrameInfo::new().block_size(BlockSize::Max4MB).content_checksum(true);
        let enc = FrameEncoder::with_frame_info(frame, hw);
        let mut tar = Builder::new(enc);

        pack_tree(
            &mut tar,
            &entry.apk_dir,
            Path::new("apk"),
            false,
            &mut done,
            total,
            cancel,
            progress,
        )?;
        if let Some(data_dir) = entry.data_dir.as_deref().filter(|p| p.is_dir()) {
            pack_tree(
                &mut tar,
                data_dir,
                Path::new("data"),
                true,
                &mut done,
                total,
                cancel,
                progress,
            )?;
        }

        let enc = tar.into_inner().context("finish tar archive")?;
        let mut hw = enc.finish().context("finish lz4 archive")?;
        hw.flush().context("flush vault archive")?;
        let archive_bytes = hw.written;
        let sha256 = hex::encode(hw.hasher.finalize());
        hw.inner.sync_all().context("sync vault archive")?;
        Ok((archive_bytes, sha256))
    })();

    let (archive_bytes, sha256) = match pack_res {
        Ok(res) => res,
        Err(e) => {
            let _ = fs::remove_file(&part_path);
            return Err(e);
        }
    };
    fs::rename(&part_path, &archive_path).with_context(|| format!("finalize {}", archive_path.display()))?;

    let manifest = VaultManifest {
        format: VAULT_FORMAT,
        package: entry.package.clone(),
        made,
        device: device.to_string(),
        apk_rel_dir: apk_relative_dir(entry),
        has_data: entry.data_dir.as_deref().is_some_and(Path::is_dir),
        apk_bytes: entry.apk_bytes,
        data_bytes: entry.data_bytes,
        archive_file,
        archive_bytes,
        sha256,
    };
    let manifest_path = vault_dir.join(format!("{stem}.toml"));
    let text = toml::to_string(&manifest).context("encode vault manifest")?;
    let mut mf = aweyuki_files::make_file(&manifest_path, ownership)?;
    mf.write_all(text.as_bytes()).with_context(|| format!("write {}", manifest_path.display()))?;
    mf.sync_all().with_context(|| format!("sync {}", manifest_path.display()))?;
    progress(total, total);
    Ok(manifest)
}

/// List valid saved app backups in `vault_dir`, newest first.
///
/// # Errors
///
/// Returns an error if `vault_dir` exists but cannot be read.
pub fn list_vault(vault_dir: &Path) -> Result<Vec<VaultManifest>> {
    if !vault_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(vault_dir).with_context(|| format!("read {}", vault_dir.display()))? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(m) = toml::from_str::<VaultManifest>(&text) else {
            continue;
        };
        if m.format == VAULT_FORMAT && vault_dir.join(&m.archive_file).is_file() {
            out.push(m);
        }
    }
    out.sort_by(|a, b| b.made.cmp(&a.made).then_with(|| a.package.cmp(&b.package)));
    Ok(out)
}

fn validate_safe_rel(path: &Path) -> Result<()> {
    ensure!(!path.as_os_str().is_empty(), "empty relative path");
    for c in path.components() {
        ensure!(matches!(c, Component::Normal(_)), "unsafe path component in {}", path.display());
    }
    Ok(())
}

fn verify_file_sha256(path: &Path, expected_hex: &str, cancel: &AtomicBool) -> Result<()> {
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let n = f.read(&mut buf).with_context(|| format!("read {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    ensure!(actual == expected_hex, "vault archive SHA-256 mismatch ({actual} != {expected_hex})");
    Ok(())
}

/// Verify and restore an app backup from `vault_dir` into `data_root` (`/data`).
///
/// # Errors
///
/// Returns an error if SHA-256 verification fails, the archive contains unsafe
/// paths, the operation is cancelled, or writing target files fails.
pub fn restore_app(
    manifest: &VaultManifest,
    vault_dir: &Path,
    data_root: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    ensure!(manifest.format == VAULT_FORMAT, "unsupported vault format {}", manifest.format);
    validate_safe_rel(Path::new(&manifest.archive_file))?;
    validate_safe_rel(Path::new(&manifest.apk_rel_dir))?;
    validate_safe_rel(Path::new(&manifest.package))?;

    let archive_path = vault_dir.join(&manifest.archive_file);
    verify_file_sha256(&archive_path, &manifest.sha256, cancel)?;

    let apk_base = data_root.join("app").join(&manifest.apk_rel_dir);
    let data_base = data_root.join("data").join(&manifest.package);
    let total = manifest.apk_bytes.saturating_add(manifest.data_bytes).max(1);
    let mut done = 0u64;
    progress(0, total);

    // Pre-scan entries to reject any path traversal before modifying `/data`.
    {
        let f = File::open(&archive_path)?;
        let dec = FrameDecoder::new(BufReader::new(f));
        let mut tar = Archive::new(dec);
        let mut prefixes = HashSet::new();
        for entry in tar.entries().context("inspect vault archive")? {
            if cancel.load(Ordering::Relaxed) {
                return Err(Cancelled.into());
            }
            let entry = entry.context("read vault entry")?;
            let rel = entry.path().context("read vault entry path")?.into_owned();
            validate_safe_rel(&rel)?;
            let top = rel
                .components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
                .context("missing vault prefix")?;
            ensure!(top == "apk" || top == "data", "unexpected archive prefix {top:?}");
            prefixes.insert(top.to_string());
        }
        ensure!(prefixes.contains("apk"), "vault archive does not contain apk/");
    }

    fs::create_dir_all(&apk_base).with_context(|| format!("create {}", apk_base.display()))?;
    if manifest.has_data {
        fs::create_dir_all(&data_base).with_context(|| format!("create {}", data_base.display()))?;
    }

    let f = File::open(&archive_path)?;
    let dec = FrameDecoder::new(BufReader::new(f));
    let mut tar = Archive::new(dec);
    let mut buf = vec![0u8; COPY_BUF];

    for entry in tar.entries().context("read vault archive")? {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let mut entry = entry.context("read vault entry")?;
        let rel = entry.path()?.into_owned();
        let (base, sub) = if let Ok(s) = rel.strip_prefix("apk") {
            (&apk_base, s)
        } else if let Ok(s) = rel.strip_prefix("data") {
            (&data_base, s)
        } else {
            bail!("unexpected entry path {}", rel.display());
        };
        let target = if sub.as_os_str().is_empty() { base.clone() } else { base.join(sub) };
        let header = entry.header().clone();
        let mode = (header.mode()? & 0o7777) as u32;

        match header.entry_type() {
            EntryType::Directory => {
                fs::create_dir_all(&target).with_context(|| format!("create {}", target.display()))?;
                fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
                #[cfg(target_os = "android")]
                let _ = std::os::unix::fs::lchown(&target, Some(header.uid()? as u32), Some(header.gid()? as u32));
            }
            EntryType::Regular => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let _ = fs::remove_file(&target);
                let mut out = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(mode)
                    .open(&target)
                    .with_context(|| format!("create {}", target.display()))?;
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(Cancelled.into());
                    }
                    let n = entry.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    out.write_all(&buf[..n])?;
                    done += n as u64;
                    progress(done.min(total), total);
                }
                out.sync_all()?;
                fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
                #[cfg(target_os = "android")]
                let _ = std::os::unix::fs::lchown(&target, Some(header.uid()? as u32), Some(header.gid()? as u32));
            }
            EntryType::Symlink => {
                let link_target = entry.link_name()?.context("symlink without target")?.into_owned();
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let _ = fs::remove_file(&target);
                std::os::unix::fs::symlink(link_target, &target)?;
            }
            _ => {}
        }
    }
    progress(total, total);
    Ok(())
}

/// Delete a saved app backup archive and its `.toml` manifest from `vault_dir`.
///
/// # Errors
///
/// Returns an error if deleting an existing file fails.
pub fn delete_vault_backup(manifest: &VaultManifest, vault_dir: &Path) -> Result<()> {
    validate_safe_rel(Path::new(&manifest.archive_file))?;
    let archive_path = vault_dir.join(&manifest.archive_file);
    let manifest_name = manifest
        .archive_file
        .strip_suffix(".tar.lz4")
        .map_or_else(|| format!("{}-{}.toml", manifest.package, manifest.made), |s| format!("{s}.toml"));
    let manifest_path = vault_dir.join(manifest_name);

    for path in [&manifest_path, &archive_path] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("remove {}", path.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("aweyuki-vault-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn discovers_backs_up_verifies_restores_and_deletes_app() {
        let s = Scratch::new("round");
        let data = s.0.join("data");
        let vault = s.0.join("vault");

        let apk_dir = data.join("app/~~rnd1/com.example.demo-xyz===");
        let priv_data = data.join("data/com.example.demo");
        fs::create_dir_all(&apk_dir).unwrap();
        fs::create_dir_all(priv_data.join("files")).unwrap();
        fs::create_dir_all(priv_data.join("cache")).unwrap();
        fs::write(apk_dir.join("base.apk"), b"PK-FAKE-APK").unwrap();
        fs::write(priv_data.join("files/save.dat"), b"player-score-99").unwrap();
        fs::write(priv_data.join("cache/temp.bin"), b"ignored-cache-bytes").unwrap();

        let apps = discover_apps(&data).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].package, "com.example.demo");
        assert_eq!(apps[0].data_bytes, b"player-score-99".len() as u64, "cache is excluded");

        let stop = AtomicBool::new(false);
        let manifest = backup_app(&apps[0], &vault, "ishtar", 1_792_600_000, Ownership::Kept, &stop, &mut |_, _| {}).unwrap();
        assert_eq!(manifest.apk_rel_dir, "~~rnd1/com.example.demo-xyz===");
        let listed = list_vault(&vault).unwrap();
        assert_eq!(listed, vec![manifest.clone()]);

        // Tampered archive is caught by SHA-256 verification before extracting anything.
        let restore_root = s.0.join("restored");
        fs::create_dir_all(&restore_root).unwrap();
        let archive_path = vault.join(&manifest.archive_file);
        let mut orig = fs::read(&archive_path).unwrap();
        orig[0] ^= 0xff;
        fs::write(&archive_path, &orig).unwrap();
        assert!(restore_app(&manifest, &vault, &restore_root, &stop, &mut |_, _| {}).is_err());
        orig[0] ^= 0xff;
        fs::write(&archive_path, &orig).unwrap();

        restore_app(&manifest, &vault, &restore_root, &stop, &mut |_, _| {}).unwrap();
        assert_eq!(fs::read(restore_root.join("app/~~rnd1/com.example.demo-xyz===/base.apk")).unwrap(), b"PK-FAKE-APK");
        assert_eq!(fs::read(restore_root.join("data/com.example.demo/files/save.dat")).unwrap(), b"player-score-99");
        assert!(!restore_root.join("data/com.example.demo/cache/temp.bin").exists());

        delete_vault_backup(&manifest, &vault).unwrap();
        assert!(list_vault(&vault).unwrap().is_empty());
    }

    #[test]
    fn skips_abx_binary_packages_xml_and_reads_plain_xml_legacy_apps() {
        let s = Scratch::new("xml");
        let data = s.0.join("data");
        fs::create_dir_all(data.join("system")).unwrap();
        fs::write(data.join("system/packages.xml"), b"ABX\0binary").unwrap();

        let leg = data.join("app/org.legacy.app-1");
        fs::create_dir_all(&leg).unwrap();
        fs::write(leg.join("base.apk"), b"apk").unwrap();

        let apps = discover_apps(&data).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].package, "org.legacy.app");

        fs::write(
            data.join("system/packages.xml"),
            "<packages>\n<package name=\"org.legacy.app\" codePath=\"/data/app/org.legacy.app-1\"/>\n</packages>",
        )
        .unwrap();
        let apps2 = discover_apps(&data).unwrap();
        assert_eq!(apps2.len(), 1);
    }
}
