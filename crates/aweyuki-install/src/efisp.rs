//! Flashing an ARM64 PE32+ EFI application directly to the non-slotted
//! `efisp` partition, with byte-for-byte read-back verification.

use anyhow::{bail, ensure, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(target_os = "android")]
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
#[cfg(target_os = "android")]
use std::path::{Path, PathBuf};

const PE_MACHINE_ARM64: u16 = 0xaa64;
const PE32_PLUS_MAGIC: u16 = 0x020b;
const SUBSYSTEM_EFI_APPLICATION: u16 = 10;
const CHUNK_SIZE: usize = 64 * 1024;

fn le16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

fn le32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

/// Validate that `source` holds an ARM64 PE32+ EFI application fitting
/// within `capacity` bytes, returning its byte length.
///
/// # Errors
///
/// Returns an error if `source` cannot be read, exceeds `capacity`, or fails
/// DOS/PE32+/ARM64/EFI header or section-bounds checks.
pub fn validate_arm64_efi<R: Read + Seek>(source: &mut R, capacity: u64) -> Result<u64> {
    let size = source.seek(SeekFrom::End(0)).context("stat EFI image")?;
    ensure!(size >= 64, "EFI image is too small ({size} bytes)");
    ensure!(size <= capacity, "EFI image ({size} bytes) exceeds efisp capacity ({capacity} bytes)");

    let mut dos = [0u8; 64];
    source.seek(SeekFrom::Start(0))?;
    source.read_exact(&mut dos).context("read DOS header")?;
    ensure!(&dos[0..2] == b"MZ", "missing MZ header");

    let pe_offset = u64::from(le32(&dos, 60));
    ensure!(pe_offset >= 64 && pe_offset + 24 <= size, "invalid PE header offset {pe_offset}");

    let mut pe = [0u8; 24];
    source.seek(SeekFrom::Start(pe_offset))?;
    source.read_exact(&mut pe).context("read PE header")?;
    ensure!(&pe[0..4] == b"PE\0\0", "missing PE signature");
    ensure!(le16(&pe, 4) == PE_MACHINE_ARM64, "EFI image is not ARM64 (0xaa64)");

    let sections = u64::from(le16(&pe, 6));
    let optional_size = u64::from(le16(&pe, 20));
    ensure!((1..=96).contains(&sections), "invalid PE section count {sections}");
    ensure!(
        optional_size >= 112 && pe_offset + 24 + optional_size <= size,
        "invalid PE optional header size {optional_size}"
    );

    let mut optional = [0u8; 112];
    source.read_exact(&mut optional).context("read PE optional header")?;
    ensure!(le16(&optional, 0) == PE32_PLUS_MAGIC, "EFI image is not PE32+");
    ensure!(le16(&optional, 68) == SUBSYSTEM_EFI_APPLICATION, "PE image is not an EFI application");

    let table_offset = pe_offset + 24 + optional_size;
    ensure!(sections * 40 <= size - table_offset, "PE section table exceeds file size");
    source.seek(SeekFrom::Start(table_offset))?;
    for _ in 0..sections {
        let mut sec = [0u8; 40];
        source.read_exact(&mut sec).context("read PE section header")?;
        let raw_len = u64::from(le32(&sec, 16));
        let raw_ptr = u64::from(le32(&sec, 20));
        if raw_len > 0 {
            ensure!(raw_ptr <= size && raw_len <= size - raw_ptr, "PE section raw data exceeds file size");
        }
    }
    Ok(size)
}

/// Write `bytes` from `source` to `dest` without truncating or discarding,
/// flush to disk, and compare every written byte back.
///
/// # Errors
///
/// Returns an error if reading, writing, syncing, or read-back comparison
/// fails.
pub fn write_and_verify<R: Read + Seek>(source: &mut R, dest: &mut File, bytes: u64) -> Result<()> {
    ensure!(bytes > 0, "EFI image is empty");
    source.seek(SeekFrom::Start(0))?;
    dest.seek(SeekFrom::Start(0))?;

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut remaining = bytes;
    while remaining > 0 {
        let count = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("bounded by CHUNK_SIZE");
        source.read_exact(&mut buf[..count]).context("read EFI source")?;
        dest.write_all(&buf[..count]).context("write efisp")?;
        remaining -= count as u64;
    }
    dest.sync_all().context("sync efisp")?;
    aweyuki_block::drop_cache(dest).context("drop cached efisp pages")?;

    source.seek(SeekFrom::Start(0))?;
    dest.seek(SeekFrom::Start(0))?;
    let mut actual = vec![0u8; CHUNK_SIZE];
    let mut remaining = bytes;
    while remaining > 0 {
        let count = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("bounded by CHUNK_SIZE");
        source.read_exact(&mut buf[..count]).context("read EFI source for verification")?;
        dest.read_exact(&mut actual[..count]).context("read back efisp for verification")?;
        if buf[..count] != actual[..count] {
            bail!("efisp read-back verification mismatch");
        }
        remaining -= count as u64;
    }
    Ok(())
}

/// Locate the non-slotted `efisp` block device and its capacity in bytes.
///
/// # Errors
///
/// Returns an error if no `efisp` partition exists or candidate symlinks refer
/// to different block devices.
#[cfg(target_os = "android")]
pub fn discover_partition() -> Result<(PathBuf, u64)> {
    let mut found: Option<(PathBuf, u64, u64)> = None;
    for candidate in ["/dev/block/by-name/efisp", "/dev/block/bootdevice/by-name/efisp"] {
        let path = Path::new(candidate);
        let Ok(mut file) = File::open(path) else {
            continue;
        };
        let Ok(meta) = file.metadata() else {
            continue;
        };
        let Ok(size) = file.seek(SeekFrom::End(0)) else {
            continue;
        };
        if size == 0 {
            continue;
        }
        let rdev = meta.rdev();
        if let Some((_, prev_rdev, _)) = found {
            ensure!(prev_rdev == rdev, "conflicting efisp block devices");
            continue;
        }
        found = Some((path.to_path_buf(), rdev, size));
    }
    match found {
        Some((path, _, size)) => Ok((path, size)),
        None => bail!("efisp partition not found"),
    }
}

/// Flash an ARM64 `.efi` file to the device's `efisp` partition and verify.
///
/// # Errors
///
/// Returns an error if `efi_path` is not a regular valid ARM64 EFI image,
/// `efisp` cannot be opened, or writing/verification fails.
#[cfg(target_os = "android")]
pub fn flash(efi_path: &Path) -> Result<u64> {
    let (dev_path, capacity) = discover_partition()?;
    let mut src = File::open(efi_path).with_context(|| format!("open {}", efi_path.display()))?;
    ensure!(src.metadata()?.is_file(), "{} is not a regular file", efi_path.display());
    let bytes = validate_arm64_efi(&mut src, capacity)?;
    let mut dst = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&dev_path)
        .with_context(|| format!("open {} for writing", dev_path.display()))?;
    write_and_verify(&mut src, &mut dst, bytes)?;
    Ok(bytes)
}

/// Zero-fill `bytes` from the start of `dest`, sync to disk, and verify every
/// byte reads back as zero.
///
/// # Errors
///
/// Returns an error if writing, syncing, or read-back verification fails.
pub fn zero_and_verify(dest: &mut File, bytes: u64) -> Result<()> {
    ensure!(bytes > 0, "partition size is zero");
    dest.seek(SeekFrom::Start(0))?;

    let zeros = vec![0u8; CHUNK_SIZE];
    let mut remaining = bytes;
    while remaining > 0 {
        let count = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("bounded by CHUNK_SIZE");
        dest.write_all(&zeros[..count]).context("write zeros")?;
        remaining -= count as u64;
    }
    dest.sync_all().context("sync zeroed partition")?;
    aweyuki_block::drop_cache(dest).context("drop cached partition pages")?;

    dest.seek(SeekFrom::Start(0))?;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut remaining = bytes;
    while remaining > 0 {
        let count = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("bounded by CHUNK_SIZE");
        dest.read_exact(&mut buf[..count]).context("read back zeroed partition")?;
        if buf[..count].iter().any(|&b| b != 0) {
            bail!("partition zero verification failed");
        }
        remaining -= count as u64;
    }
    Ok(())
}

/// Wipe the non-slotted `efisp` partition by zeroing and verifying all bytes.
///
/// # Errors
///
/// Returns an error if `efisp` cannot be found, is in use (`O_EXCL`), or
/// zeroing/verification fails.
#[cfg(target_os = "android")]
pub fn wipe() -> Result<u64> {
    let (dev_path, expected_bytes) = discover_partition()?;
    let resolved = std::fs::canonicalize(&dev_path).with_context(|| format!("resolve {}", dev_path.display()))?;
    ensure!(
        crate::frp::is_block_path(&resolved),
        "resolved efisp path {} is outside /dev/block",
        resolved.display()
    );
    let expected_meta = std::fs::metadata(&resolved).with_context(|| format!("stat {}", resolved.display()))?;
    ensure!(expected_meta.file_type().is_block_device(), "{} is not a block device", resolved.display());
    let expected_rdev = expected_meta.rdev();

    let mut dst = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&resolved)
        .with_context(|| format!("open {} exclusively for wiping", resolved.display()))?;
    let meta = dst.metadata().context("stat opened efisp device")?;
    ensure!(meta.file_type().is_block_device() && meta.rdev() == expected_rdev, "efisp device changed before wipe");
    let actual_bytes = dst.seek(SeekFrom::End(0)).context("find efisp size")?;
    ensure!(actual_bytes == expected_bytes, "efisp size changed before wipe");

    zero_and_verify(&mut dst, expected_bytes)?;
    Ok(expected_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn synthetic_arm64_efi(machine: u16, magic: u16, subsystem: u16, section_end: u32) -> Vec<u8> {
        let mut buf = vec![0u8; 512];
        buf[0..2].copy_from_slice(b"MZ");
        buf[60..64].copy_from_slice(&64u32.to_le_bytes());
        buf[64..68].copy_from_slice(b"PE\0\0");
        buf[68..70].copy_from_slice(&machine.to_le_bytes());
        buf[70..72].copy_from_slice(&1u16.to_le_bytes());
        buf[84..86].copy_from_slice(&112u16.to_le_bytes());
        buf[88..90].copy_from_slice(&magic.to_le_bytes());
        buf[88 + 68..88 + 70].copy_from_slice(&subsystem.to_le_bytes());
        buf[200 + 16..200 + 20].copy_from_slice(&section_end.to_le_bytes());
        buf[200 + 20..200 + 24].copy_from_slice(&256u32.to_le_bytes());
        buf
    }

    #[test]
    fn valid_arm64_efi_passes_and_writes_with_verification() {
        let dir = tempfile::tempdir().unwrap();
        let efi = synthetic_arm64_efi(PE_MACHINE_ARM64, PE32_PLUS_MAGIC, SUBSYSTEM_EFI_APPLICATION, 128);
        let mut src = Cursor::new(&efi);
        let bytes = validate_arm64_efi(&mut src, 4096).unwrap();
        assert_eq!(bytes, 512);

        let dst_path = dir.path().join("dest.img");
        std::fs::write(&dst_path, vec![0xAAu8; 1024]).unwrap();
        let mut dst = std::fs::OpenOptions::new().read(true).write(true).open(&dst_path).unwrap();
        write_and_verify(&mut src, &mut dst, bytes).unwrap();

        let mut tail = [0u8; 4];
        dst.seek(SeekFrom::Start(512)).unwrap();
        dst.read_exact(&mut tail).unwrap();
        assert_eq!(tail, [0xAA; 4], "bytes after the EFI image stay untouched");
    }

    #[test]
    fn invalid_pe_headers_are_rejected() {
        let bad_arch = synthetic_arm64_efi(0x8664, PE32_PLUS_MAGIC, SUBSYSTEM_EFI_APPLICATION, 128);
        assert!(validate_arm64_efi(&mut Cursor::new(bad_arch), 4096).is_err());

        let bad_sub = synthetic_arm64_efi(PE_MACHINE_ARM64, PE32_PLUS_MAGIC, 3, 128);
        assert!(validate_arm64_efi(&mut Cursor::new(bad_sub), 4096).is_err());

        let oob_sec = synthetic_arm64_efi(PE_MACHINE_ARM64, PE32_PLUS_MAGIC, SUBSYSTEM_EFI_APPLICATION, 1024);
        assert!(validate_arm64_efi(&mut Cursor::new(oob_sec), 4096).is_err());

        let too_large = synthetic_arm64_efi(PE_MACHINE_ARM64, PE32_PLUS_MAGIC, SUBSYSTEM_EFI_APPLICATION, 128);
        assert!(validate_arm64_efi(&mut Cursor::new(too_large), 256).is_err());
    }

    #[test]
    fn zero_and_verify_clears_and_verifies_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("part.img");
        std::fs::write(&path, vec![0x5Au8; 128 * 1024 + 321]).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        let mut file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        zero_and_verify(&mut file, size).unwrap();
        let after = std::fs::read(&path).unwrap();
        assert_eq!(after.len() as u64, size);
        assert!(after.iter().all(|&b| b == 0));
    }
}
