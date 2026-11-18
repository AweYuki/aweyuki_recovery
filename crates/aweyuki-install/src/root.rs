//! Root Manager helpers ported from AERA Recovery:
//! - Kernel KMI detection (`android14-6.1`, `android15-6.6`, `android16-6.12`, `android17-6.18`)
//! - Detection of Magisk, KernelSU, and APatch installations under `/data/adb`
//! - Inspection and safe enable/disable/remove toggling of `/data/adb/modules/*`

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub const DEFAULT_ADB_ROOT: &str = "/data/adb";

pub const SUPPORTED_KMIS: &[&str] = &[
    "android14-6.1",
    "android15-6.6",
    "android16-6.12",
    "android17-6.18",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Magisk,
    KernelSu,
    APatch,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Magisk => "Magisk",
            Provider::KernelSu => "KernelSU",
            Provider::APatch => "APatch",
        }
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

/// Parse `<major>.<minor>.<patch>-...-android<N>...` into `android<N>-<major>.<minor>`.
pub fn kmi_from_release(release: &str) -> Option<String> {
    let (ver_part, rest) = release.split_once('-')?;
    let mut nums = ver_part.split('.');
    let major = nums.next()?;
    let minor = nums.next()?;
    let _patch = nums.next()?;
    if !major.bytes().all(|b| b.is_ascii_digit()) || !minor.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let android_pos = rest.find("android")?;
    let after_android = &rest[android_pos + "android".len()..];
    let digits_len = after_android.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }
    let android_ver = &after_android[..digits_len];
    Some(format!("android{android_ver}-{major}.{minor}"))
}

/// Resolve a kernel release string to one of `supported_kmis`, falling back
/// to unique `<major>.<minor>` matching only when `android` is absent.
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
    if !major.bytes().all(|b| b.is_ascii_digit()) || !minor.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ver = format!("-{major}.{minor}");
    let mut candidate: Option<&str> = None;
    for &kmi in supported {
        if kmi.ends_with(&ver) {
            if let Some(prev) = candidate {
                if prev != kmi {
                    return None;
                }
            } else {
                candidate = Some(kmi);
            }
        }
    }
    candidate.map(str::to_string)
}

/// Scan raw kernel/image bytes for `"Linux version <release> ..."` and resolve KMI.
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

/// Enable or disable a module by creating/removing `<adb_root>/modules/<id>/disable`.
pub fn set_module_enabled(adb_root: &Path, id: &str, enabled: bool) -> Result<()> {
    let marker = adb_root.join("modules").join(id).join("disable");
    if enabled {
        if marker.exists() {
            fs::remove_file(&marker).with_context(|| format!("remove {}", marker.display()))?;
        }
    } else {
        fs::write(&marker, b"").with_context(|| format!("write {}", marker.display()))?;
    }
    Ok(())
}

/// Mark or unmark a module for removal on next boot (`<adb_root>/modules/<id>/remove`).
pub fn set_module_remove(adb_root: &Path, id: &str, remove: bool) -> Result<()> {
    let marker = adb_root.join("modules").join(id).join("remove");
    if remove {
        fs::write(&marker, b"").with_context(|| format!("write {}", marker.display()))?;
    } else if marker.exists() {
        fs::remove_file(&marker).with_context(|| format!("remove {}", marker.display()))?;
    }
    Ok(())
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
            resolve_kmi("6.6.30-g123456", SUPPORTED_KMIS).as_deref(),
            Some("android15-6.6")
        );
        assert_eq!(
            kmi_from_banner(b"\0\0Linux version 6.1.75-android14-11-gabcdef (root@host)\n", SUPPORTED_KMIS).as_deref(),
            Some("android14-6.1")
        );
    }

    #[test]
    fn modules_and_providers_are_inspected_and_toggled() {
        let base = std::env::temp_dir().join("aweyuki-root-tests");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("ksu")).unwrap();
        fs::create_dir_all(base.join("modules/zygisksu")).unwrap();
        fs::write(
            base.join("modules/zygisksu/module.prop"),
            "id=zygisksu\nname=Zygisk Next\nversion=v1.2.0\nauthor=Dr-TSNG\ndescription=Standalone Zygisk\n",
        )
        .unwrap();

        assert_eq!(detect_providers(&base), vec![Provider::KernelSu]);
        let mods = list_modules(&base).unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "Zygisk Next");
        assert!(mods[0].enabled);

        set_module_enabled(&base, "zygisksu", false).unwrap();
        assert!(!list_modules(&base).unwrap()[0].enabled);
        set_module_remove(&base, "zygisksu", true).unwrap();
        assert!(list_modules(&base).unwrap()[0].remove_pending);
    }
}
