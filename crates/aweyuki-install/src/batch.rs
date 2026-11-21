//! Multi-image batch flashing with partition inference and size/geometry
//! pre-validation (`aera_image_flash_batch` / `aera_image_flash_geometry`).

use crate::image::{self, ImageKind};
#[cfg(target_os = "android")]
use anyhow::ensure;
use anyhow::{Context, Result};
use aweyuki_vab::Slot;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const HEAD: usize = 4096;

/// Size and existence check for one image against its target partition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeometryCheck {
    Ok { partition_bytes: u64 },
    TooLarge { image_bytes: u64, partition_bytes: u64 },
    MissingPartition,
    Unrecognized,
}

/// One candidate image in a batch directory or CLI file list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchItem {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub kind: Option<ImageKind>,
    pub partition: Option<String>,
    pub slotted: bool,
    pub geometry: GeometryCheck,
}

fn device_size(path: &Path) -> Result<u64> {
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    f.seek(SeekFrom::End(0)).with_context(|| format!("find size of {}", path.display()))
}

fn known_raw_stem(stem: &str) -> Option<(&'static str, bool)> {
    const SLOTTED: [&str; 19] = [
        "boot",
        "init_boot",
        "vendor_boot",
        "recovery",
        "dtbo",
        "vbmeta",
        "vbmeta_system",
        "vbmeta_vendor",
        "abl",
        "xbl",
        "modem",
        "dsp",
        "bluetooth",
        "hyp",
        "tz",
        "devcfg",
        "keymaster",
        "qupfw",
        "uefisecapp",
    ];
    if stem == "efisp" {
        return Some(("efisp", false));
    }
    SLOTTED.iter().copied().find(|&k| k == stem).map(|k| (k, true))
}

/// Infer the target partition name and whether it is A/B slotted from the
/// filename stem and inspected image header.
#[must_use]
pub fn infer_partition(file_name: &str, kind: Option<ImageKind>) -> Option<(String, bool)> {
    let raw_stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = raw_stem
        .strip_suffix("_a")
        .or_else(|| raw_stem.strip_suffix("_b"))
        .unwrap_or(&raw_stem);

    match kind {
        Some(ImageKind::Efisp) => Some(("efisp".into(), false)),
        Some(ImageKind::VendorBoot) => Some(("vendor_boot".into(), true)),
        Some(ImageKind::Dtbo) => Some(("dtbo".into(), true)),
        Some(k @ ImageKind::Boot { .. }) => {
            let name = match stem {
                "recovery" => "recovery",
                "init_boot" => "init_boot",
                "boot" => "boot",
                _ => k.partitions()[0],
            };
            Some((name.into(), true))
        }
        Some(ImageKind::Vbmeta) => {
            let name = match stem {
                "vbmeta_system" => "vbmeta_system",
                "vbmeta_vendor" => "vbmeta_vendor",
                _ => "vbmeta",
            };
            Some((name.into(), true))
        }
        None => known_raw_stem(stem).map(|(p, s)| (p.into(), s)),
    }
}

#[cfg(target_os = "android")]
fn resolve_efisp(by_name: &Path) -> Option<(PathBuf, u64)> {
    if by_name == Path::new(aweyuki_block::BY_NAME) {
        return crate::efisp::discover_partition().ok();
    }
    let p = by_name.join("efisp");
    device_size(&p).ok().map(|s| (p, s))
}

#[cfg(not(target_os = "android"))]
fn resolve_efisp(by_name: &Path) -> Option<(PathBuf, u64)> {
    let p = by_name.join("efisp");
    device_size(&p).ok().map(|s| (p, s))
}

fn resolve_target_size(by_name: &Path, partition: &str, slotted: bool, slot: Option<Slot>) -> Option<(bool, u64)> {
    if partition == "efisp" {
        return resolve_efisp(by_name).map(|(_, size)| (false, size));
    }
    if slotted && let Some(s) = slot {
        let suffixed = by_name.join(format!("{partition}{}", s.suffix()));
        if let Ok(size) = device_size(&suffixed) {
            return Some((true, size));
        }
    }
    let plain = by_name.join(partition);
    device_size(&plain).ok().map(|size| (false, size))
}

/// Inspect a single image file and validate its partition target and geometry.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or stat'd.
pub fn inspect_file(path: &Path, by_name: &Path, slot: Option<Slot>) -> Result<BatchItem> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let size = file.seek(SeekFrom::End(0)).with_context(|| format!("stat {}", path.display()))?;
    file.seek(SeekFrom::Start(0))?;
    let mut head = Vec::with_capacity(HEAD);
    (&mut file).take(HEAD as u64).read_to_end(&mut head)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kind = image::detect(&mut file, &head).ok().flatten();
    let Some((partition, default_slotted)) = infer_partition(&file_name, kind) else {
        return Ok(BatchItem {
            path: path.to_path_buf(),
            file_name,
            size,
            kind,
            partition: None,
            slotted: false,
            geometry: GeometryCheck::Unrecognized,
        });
    };
    let (slotted, geometry) = match resolve_target_size(by_name, &partition, default_slotted, slot) {
        Some((is_slotted, partition_bytes)) => {
            let geom = if size <= partition_bytes {
                GeometryCheck::Ok { partition_bytes }
            } else {
                GeometryCheck::TooLarge { image_bytes: size, partition_bytes }
            };
            (is_slotted, geom)
        }
        None => (default_slotted, GeometryCheck::MissingPartition),
    };
    Ok(BatchItem {
        path: path.to_path_buf(),
        file_name,
        size,
        kind,
        partition: Some(partition),
        slotted,
        geometry,
    })
}

/// Scan `dir` for `.img` and `.efi` files sorted alphabetically and validate each
/// against `by_name` and `slot`.
///
/// # Errors
///
/// Returns an error if `dir` cannot be listed.
pub fn plan_directory(dir: &Path, by_name: &Path, slot: Option<Slot>) -> Result<Vec<BatchItem>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let is_candidate = path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("img") || ext.eq_ignore_ascii_case("efi"));
        if is_candidate {
            paths.push(path);
        }
    }
    paths.sort();
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        items.push(inspect_file(&path, by_name, slot)?);
    }
    Ok(items)
}

/// Flash a validated batch of `(path, partition, slotted)` items to `slots`,
/// pre-checking every geometry before writing anything and verifying SHA-256
/// read-back on every partition written.
///
/// # Errors
///
/// Returns an error if any target partition is missing or undersized, if
/// `slots` is empty when writing a slotted image, or if flashing/verification fails.
#[cfg(target_os = "android")]
pub fn execute_batch(
    items: &[(PathBuf, String, bool)],
    slots: &[Slot],
    mut on_progress: impl FnMut(usize, &str, u64, u64),
) -> Result<Vec<String>> {
    ensure!(!items.is_empty(), "no images selected for batch flash");
    let by_name = Path::new(aweyuki_block::BY_NAME);
    let mut plan: Vec<(&Path, &str, u64, Vec<PathBuf>)> = Vec::with_capacity(items.len());
    let mut needs_boot_set = false;

    for (path, partition, slotted) in items {
        let size = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?.len();
        ensure!(size > 0, "{} is empty", path.display());
        if matches!(partition.as_str(), "boot" | "init_boot") {
            needs_boot_set = true;
        }
        let targets = if partition == "efisp" {
            let (dev, cap) = crate::efisp::discover_partition()?;
            let mut src = File::open(path).with_context(|| format!("open {}", path.display()))?;
            crate::efisp::validate_arm64_efi(&mut src, cap)?;
            vec![dev]
        } else if *slotted {
            ensure!(!slots.is_empty(), "no target slot chosen for {partition}");
            let mut list = Vec::with_capacity(slots.len());
            for &slot in slots {
                let dev = by_name.join(format!("{partition}{}", slot.suffix()));
                ensure!(dev.exists(), "missing partition {}", dev.display());
                let cap = device_size(&dev)?;
                ensure!(size <= cap, "{} ({size} bytes) exceeds {} ({cap} bytes)", path.display(), dev.display());
                list.push(dev);
            }
            list
        } else {
            let dev = by_name.join(partition);
            ensure!(dev.exists(), "missing partition {}", dev.display());
            let cap = device_size(&dev)?;
            ensure!(size <= cap, "{} ({size} bytes) exceeds {} ({cap} bytes)", path.display(), dev.display());
            vec![dev]
        };
        plan.push((path.as_path(), partition.as_str(), size, targets));
    }

    if needs_boot_set {
        aweyuki_systems::install::ensure_installed()?;
    }

    let mut flashed = Vec::with_capacity(plan.len());
    for (idx, (path, partition, size, targets)) in plan.into_iter().enumerate() {
        on_progress(idx, partition, 0, size);
        if partition == "efisp" {
            let written = crate::efisp::flash(path)?;
            on_progress(idx, partition, written, written);
        } else {
            let mut src = File::open(path).with_context(|| format!("open {}", path.display()))?;
            image::flash(&mut src, size, &targets, &mut |done, total| {
                on_progress(idx, partition, done, total);
            })?;
        }
        flashed.push(partition.to_string());
    }
    Ok(flashed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_efisp() -> Vec<u8> {
        let mut buf = vec![0u8; 512];
        buf[0..2].copy_from_slice(b"MZ");
        buf[60..64].copy_from_slice(&64u32.to_le_bytes());
        buf[64..68].copy_from_slice(b"PE\0\0");
        buf[68..70].copy_from_slice(&0xaa64u16.to_le_bytes());
        buf[70..72].copy_from_slice(&1u16.to_le_bytes());
        buf[84..86].copy_from_slice(&112u16.to_le_bytes());
        buf[88..90].copy_from_slice(&0x020bu16.to_le_bytes());
        buf[88 + 68..88 + 70].copy_from_slice(&10u16.to_le_bytes());
        buf[200 + 16..200 + 20].copy_from_slice(&64u32.to_le_bytes());
        buf[200 + 20..200 + 24].copy_from_slice(&256u32.to_le_bytes());
        buf
    }

    #[test]
    fn infers_partitions_and_validates_geometry() {
        assert_eq!(infer_partition("vbmeta_system.img", Some(ImageKind::Vbmeta)), Some(("vbmeta_system".into(), true)));
        assert_eq!(infer_partition("abl_a.img", None), Some(("abl".into(), true)));
        assert_eq!(infer_partition("shell.efi", Some(ImageKind::Efisp)), Some(("efisp".into(), false)));
        assert_eq!(infer_partition("notes.img", None), None);

        let tmp = tempfile::tempdir().unwrap();
        let imgs = tmp.path().join("imgs");
        let by_name = tmp.path().join("by-name");
        fs::create_dir_all(&imgs).unwrap();
        fs::create_dir_all(&by_name).unwrap();

        fs::write(by_name.join("dtbo_a"), vec![0u8; 256]).unwrap();
        fs::write(by_name.join("abl_a"), vec![0u8; 64]).unwrap();
        fs::write(by_name.join("efisp"), vec![0u8; 1024]).unwrap();

        let dtbo_path = imgs.join("dtbo.img");
        fs::write(&dtbo_path, [0xd7u8, 0xb7, 0xab, 0x1e, 0, 0, 0, 0]).unwrap();
        let abl_path = imgs.join("abl.img");
        fs::write(&abl_path, vec![0x7fu8; 128]).unwrap();
        let vb_path = imgs.join("vbmeta.img");
        fs::write(&vb_path, b"AVB0\0\0\0\0").unwrap();
        let efi_path = imgs.join("bootaa64.efi");
        fs::write(&efi_path, synthetic_efisp()).unwrap();
        let weird_path = imgs.join("random.img");
        fs::write(&weird_path, b"hello").unwrap();

        let items = plan_directory(&imgs, &by_name, Some(Slot::A)).unwrap();
        assert_eq!(items.len(), 5);
        let by_file = |n: &str| items.iter().find(|i| i.file_name == n).unwrap();
        assert_eq!(by_file("dtbo.img").geometry, GeometryCheck::Ok { partition_bytes: 256 });
        assert_eq!(by_file("abl.img").geometry, GeometryCheck::TooLarge { image_bytes: 128, partition_bytes: 64 });
        assert_eq!(by_file("vbmeta.img").geometry, GeometryCheck::MissingPartition);
        assert_eq!(by_file("bootaa64.efi").geometry, GeometryCheck::Ok { partition_bytes: 1024 });
        assert_eq!(by_file("random.img").geometry, GeometryCheck::Unrecognized);
    }
}
