//! Root Manager helpers ported from AERA Recovery:
//! - Kernel KMI detection (`5.10`, `5.15`, `6.1`, `6.6`, `6.12`, `6.18`)
//! - Detection of Magisk, KernelSU, and APatch installations under `/data/adb`
//! - Inspection and safe enable/disable/remove toggling of `/data/adb/modules/*`

use anyhow::{ensure, Context, Result};
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_ADB_ROOT: &str = "/data/adb";

pub const SUPPORTED_KMIS: &[&str] = &[
    "android12-5.10",
    "android13-5.10",
    "android13-5.15",
    "android14-5.15",
    "android14-6.1",
    "android15-6.6",
    "android16-6.12",
    "android17-6.18",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    Magisk,
    KernelSu,
    APatch,
}

impl Provider {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Provider::Magisk => "Magisk",
            Provider::KernelSu => "KernelSU",
            Provider::APatch => "APatch",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub enabled: bool,
    pub remove_pending: bool,
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Parse `<major>.<minor>.<patch>-...-android<N>...` into `android<N>-<major>.<minor>`.
#[must_use]
pub fn kmi_from_release(release: &str) -> Option<String> {
    let (ver_part, rest) = release.split_once('-')?;
    let mut nums = ver_part.split('.');
    let major = nums.next()?;
    let minor = nums.next()?;
    let patch = nums.next()?;
    if !is_digits(major) || !is_digits(minor) || !patch.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let after_android = rest
        .strip_prefix("android")
        .or_else(|| rest.split_once("-android").map(|(_, tail)| tail))?;
    let digits_len = after_android.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 {
        return None;
    }
    let android_ver = &after_android[..digits_len];
    Some(format!("android{android_ver}-{major}.{minor}"))
}

/// Resolve a kernel release string to one of `supported_kmis`, falling back
/// to unique `<major>.<minor>` matching only when `android` is absent.
#[must_use]
pub fn resolve_kmi(release: &str, supported: &[&str]) -> Option<String> {
    if let Some(exact) = kmi_from_release(release) {
        return Some(exact);
    }
    if release.contains("android") {
        return None;
    }
    let mut nums = release.split(['.', '-', '+']);
    let major = nums.next()?;
    let minor = nums.next()?;
    if !is_digits(major) || !is_digits(minor) {
        return None;
    }
    let ver = format!("-{major}.{minor}");
    let mut candidate: Option<&str> = None;
    for &kmi in supported {
        if !kmi.ends_with(&ver) {
            continue;
        }
        match candidate {
            Some(prev) if prev != kmi => return None,
            _ => candidate = Some(kmi),
        }
    }
    candidate.map(str::to_string)
}

/// Scan raw kernel/image bytes for `"Linux version <release> ..."` and resolve KMI.
#[must_use]
pub fn kmi_from_banner(bytes: &[u8], supported: &[&str]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut offset = 0;
    while let Some(pos) = text[offset..].find("Linux version ") {
        let start = offset + pos + "Linux version ".len();
        let rest = &text[start..];
        let end = rest.find(['\0', ' ', '\t', '\r', '\n']).unwrap_or(rest.len());
        if let Some(kmi) = resolve_kmi(&rest[..end], supported) {
            return Some(kmi);
        }
        offset = start + end;
    }
    None
}

/// Detect installed root frameworks under `adb_root` (typically `/data/adb`).
#[must_use]
pub fn detect_providers(adb_root: &Path) -> Vec<Provider> {
    let mut found = Vec::new();
    if adb_root.join("magisk").exists() || adb_root.join("magisk.db").exists() {
        found.push(Provider::Magisk);
    }
    if adb_root.join("ksu").exists() || adb_root.join("ksud").exists() {
        found.push(Provider::KernelSu);
    }
    if adb_root.join("ap").exists() || adb_root.join("apd").exists() {
        found.push(Provider::APatch);
    }
    found
}

/// List all modules under `<adb_root>/modules`, sorted by id.
///
/// # Errors
///
/// Returns an error if `<adb_root>/modules` exists but cannot be read.
pub fn list_modules(adb_root: &Path) -> Result<Vec<ModuleInfo>> {
    let modules_dir = adb_root.join("modules");
    if !modules_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut modules = Vec::new();
    for entry in fs::read_dir(&modules_dir).with_context(|| format!("read {}", modules_dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.starts_with('.') {
            continue;
        }
        let mut info = ModuleInfo {
            id: id.clone(),
            name: id,
            version: String::new(),
            author: String::new(),
            description: String::new(),
            enabled: !path.join("disable").exists(),
            remove_pending: path.join("remove").exists(),
        };
        if let Ok(prop) = fs::read_to_string(path.join("module.prop")) {
            for line in prop.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k.trim() {
                        "name" => info.name = v.trim().to_string(),
                        "version" => info.version = v.trim().to_string(),
                        "author" => info.author = v.trim().to_string(),
                        "description" => info.description = v.trim().to_string(),
                        _ => {}
                    }
                }
            }
        }
        modules.push(info);
    }
    modules.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(modules)
}

fn module_dir(adb_root: &Path, id: &str) -> Result<PathBuf> {
    let mut comps = Path::new(id).components();
    ensure!(
        !id.is_empty()
            && !id.contains('\0')
            && matches!(comps.next(), Some(Component::Normal(_)))
            && comps.next().is_none(),
        "invalid module id {id:?}"
    );
    let dir = adb_root.join("modules").join(id);
    ensure!(dir.is_dir(), "module {id:?} not found under {}", adb_root.display());
    Ok(dir)
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("remove {}", path.display())),
    }
}

/// Enable module `id` by removing `<adb_root>/modules/<id>/disable`.
///
/// # Errors
///
/// Returns an error if `id` is invalid, the module directory does not exist,
/// or the marker file cannot be removed.
pub fn enable_module(adb_root: &Path, id: &str) -> Result<()> {
    remove_if_present(&module_dir(adb_root, id)?.join("disable"))
}

/// Disable module `id` by creating `<adb_root>/modules/<id>/disable`.
///
/// # Errors
///
/// Returns an error if `id` is invalid, the module directory does not exist,
/// or the marker file cannot be written.
pub fn disable_module(adb_root: &Path, id: &str) -> Result<()> {
    let marker = module_dir(adb_root, id)?.join("disable");
    fs::write(&marker, b"").with_context(|| format!("write {}", marker.display()))
}

/// Mark module `id` for removal on next boot.
///
/// # Errors
///
/// Returns an error if `id` is invalid, the module directory does not exist,
/// or the marker file cannot be written.
pub fn mark_module_for_removal(adb_root: &Path, id: &str) -> Result<()> {
    let marker = module_dir(adb_root, id)?.join("remove");
    fs::write(&marker, b"").with_context(|| format!("write {}", marker.display()))
}

/// Clear a pending removal marker for module `id`.
///
/// # Errors
///
/// Returns an error if `id` is invalid, the module directory does not exist,
/// or the marker file cannot be removed.
pub fn clear_module_removal(adb_root: &Path, id: &str) -> Result<()> {
    remove_if_present(&module_dir(adb_root, id)?.join("remove"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kmi_parses_standard_and_fallback_releases() {
        assert_eq!(
            kmi_from_release("6.12.69-android16-6-g43b5ab7e247d-abogki561813157-4k").as_deref(),
            Some("android16-6.12")
        );
        assert_eq!(
            kmi_from_release("5.15.178-android13-8-g123456").as_deref(),
            Some("android13-5.15")
        );
        assert_eq!(
            kmi_from_release("5.10.198-android12-9-gabcdef").as_deref(),
            Some("android12-5.10")
        );
        assert_eq!(resolve_kmi("6.6.30-g123456", SUPPORTED_KMIS).as_deref(), Some("android15-6.6"));
        assert_eq!(resolve_kmi("5.15.178-g123456", SUPPORTED_KMIS), None, "ambiguous 5.15 fallback refuses to guess");
        assert_eq!(kmi_from_release("..1-android16"), None);
        assert_eq!(kmi_from_release("6.1.75-gmyandroid14"), None);
        assert_eq!(
            kmi_from_banner(b"\0\0Linux version 6.1.75-android14-11-gabcdef (root@host)\n", SUPPORTED_KMIS).as_deref(),
            Some("android14-6.1")
        );
    }

    #[test]
    fn modules_and_providers_are_inspected_and_toggled() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("ksu")).unwrap();
        fs::create_dir_all(base.join("modules/zygisksu")).unwrap();
        fs::write(
            base.join("modules/zygisksu/module.prop"),
            "id=zygisksu\nname=Zygisk Next\nversion=v1.2.0\nauthor=Dr-TSNG\ndescription=Standalone Zygisk\n",
        )
        .unwrap();

        assert_eq!(detect_providers(base), vec![Provider::KernelSu]);
        let mods = list_modules(base).unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "Zygisk Next");
        assert!(mods[0].enabled);

        disable_module(base, "zygisksu").unwrap();
        assert!(!list_modules(base).unwrap()[0].enabled);
        enable_module(base, "zygisksu").unwrap();
        assert!(list_modules(base).unwrap()[0].enabled);
        mark_module_for_removal(base, "zygisksu").unwrap();
        assert!(list_modules(base).unwrap()[0].remove_pending);
        clear_module_removal(base, "zygisksu").unwrap();
        assert!(!list_modules(base).unwrap()[0].remove_pending);

        assert!(disable_module(base, "../evil").is_err());
        assert!(disable_module(base, "missing-mod").is_err());
    }
}
