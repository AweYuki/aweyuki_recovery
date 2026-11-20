//! Dedicated Factory Reset Protection (`frp`) partition discovery and
//! verified zero-fill wiping, ported from AERA Recovery (`aera_frp_wipe.hpp`).

#[cfg(target_os = "android")]
use anyhow::{bail, ensure, Context, Result};
#[cfg(target_os = "android")]
use std::fs::{self, OpenOptions};
#[cfg(target_os = "android")]
use std::io::{Seek, SeekFrom};
#[cfg(target_os = "android")]
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
#[cfg(target_os = "android")]
use std::path::PathBuf;

/// Return `true` if `path` lives strictly beneath `/dev/block`.
#[must_use]
pub fn is_block_path(path: &Path) -> bool {
    path.starts_with("/dev/block") && path != Path::new("/dev/block")
}

#[cfg(target_os = "android")]
fn inspect(path: &Path) -> Result<(PathBuf, u64, u64)> {
    ensure!(is_block_path(path), "refusing non-block FRP path {}", path.display());
    let resolved = fs::canonicalize(path).with_context(|| format!("resolve {}", path.display()))?;
    ensure!(is_block_path(&resolved), "resolved FRP path {} is outside /dev/block", resolved.display());

    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&resolved)
        .with_context(|| format!("open {}", resolved.display()))?;
    let meta = file.metadata().with_context(|| format!("stat {}", resolved.display()))?;
    ensure!(meta.file_type().is_block_device(), "{} is not a block device", resolved.display());

    let raw_rdev = meta.rdev();
    let dev = raw_rdev as libc::dev_t;
    let major = libc::major(dev) as u32;
    let minor = libc::minor(dev) as u32;
    let sysfs_part = format!("/sys/dev/block/{major}:{minor}/partition");
    ensure!(Path::new(&sysfs_part).exists(), "{} is not a partition block device", resolved.display());

    let size = file.seek(SeekFrom::End(0)).with_context(|| format!("find size of {}", resolved.display()))?;
    ensure!(size > 0, "FRP partition {} is empty", resolved.display());
    Ok((resolved, raw_rdev, size))
}

/// Locate the device's `frp` partition and its size in bytes.
///
/// If `ro.frp.pst` is set and non-empty, only that path is inspected.
///
/// # Errors
///
/// Returns an error if no valid `frp` partition exists or candidate links
/// resolve to conflicting block devices.
#[cfg(target_os = "android")]
pub fn discover_partition() -> Result<(PathBuf, u64)> {
    if let Some(prop) = aweyuki_sys::prop("ro.frp.pst") {
        let trimmed = prop.trim();
        if !trimmed.is_empty() {
            let (resolved, _, size) = inspect(Path::new(trimmed))?;
            return Ok((resolved, size));
        }
    }
    let mut found: Option<(PathBuf, u64, u64)> = None;
    for candidate in ["/dev/block/by-name/frp", "/dev/block/bootdevice/by-name/frp"] {
        let Ok((resolved, rdev, size)) = inspect(Path::new(candidate)) else {
            continue;
        };
        if let Some((_, prev_rdev, _)) = found {
            ensure!(prev_rdev == rdev, "conflicting frp block devices");
            continue;
        }
        found = Some((resolved, rdev, size));
    }
    match found {
        Some((path, _, size)) => Ok((path, size)),
        None => bail!("frp partition not found"),
    }
}

/// Wipe the `frp` partition by zero-filling and verifying every byte.
///
/// # Errors
///
/// Returns an error if `frp` cannot be found, cannot be opened exclusively, or
/// zeroing/verification fails.
#[cfg(target_os = "android")]
pub fn wipe() -> Result<u64> {
    let (dev_path, _) = discover_partition()?;
    let (resolved, expected_rdev, expected_bytes) = inspect(&dev_path)?;

    let mut dst = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&resolved)
        .with_context(|| format!("open {} exclusively for wiping", resolved.display()))?;
    let meta = dst.metadata().context("stat opened frp device")?;
    ensure!(
        meta.file_type().is_block_device() && meta.rdev() == expected_rdev,
        "frp device changed before wipe"
    );
    let actual_bytes = dst.seek(SeekFrom::End(0)).context("find frp size")?;
    ensure!(actual_bytes == expected_bytes, "frp size changed before wipe");

    crate::efisp::zero_and_verify(&mut dst, expected_bytes)?;
    Ok(expected_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_block_device_paths() {
        assert!(is_block_path(Path::new("/dev/block/by-name/frp")));
        assert!(is_block_path(Path::new("/dev/block/sda5")));
        assert!(!is_block_path(Path::new("/dev/block")));
        assert!(!is_block_path(Path::new("/dev/block/")));
        assert!(!is_block_path(Path::new("/tmp/frp")));
        assert!(!is_block_path(Path::new("/dev/sda")));
    }
}
