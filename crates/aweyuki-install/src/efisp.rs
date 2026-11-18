//! Flashing an ARM64 PE32+ EFI application directly to the non-slotted
//! `efisp` partition, with byte-for-byte read-back verification.

use anyhow::{bail, ensure, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(target_os = "android")]
use std::path::Path;

const PE_MACHINE_ARM64: u16 = 0xaa64;
const PE32_PLUS_MAGIC: u16 = 0x020b;
const SUBSYSTEM_EFI_APPLICATION: u16 = 10;
const CHUNK_SIZE: usize = 64 * 1024;

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Validate that `source` holds an ARM64 PE32+ EFI application fitting
/// within `capacity` bytes, returning its byte length.
pub fn validate_arm64_efi(source: &mut File, capacity: u64) -> Result<u64> {
    let size = source.seek(SeekFrom::End(0)).context("stat EFI image")?;
    ensure!(size >= 64, "EFI image is too small ({size} bytes)");
    ensure!(size <= capacity, "EFI image ({size} bytes) exceeds efisp capacity ({capacity} bytes)");

    let mut dos = [0u8; 64];
    source.seek(SeekFrom::Start(0))?;
    source.read_exact(&mut dos).context("read DOS header")?;
    ensure!(&dos[0..2] == b"MZ", "missing MZ header");

    let pe_offset = u64::from(le32(&dos[60..64]));
    ensure!(pe_offset >= 64 && pe_offset + 24 <= size, "invalid PE header offset {pe_offset}");

    let mut pe = [0u8; 24];
    source.seek(SeekFrom::Start(pe_offset))?;
    source.read_exact(&mut pe).context("read PE header")?;
    ensure!(&pe[0..4] == b"PE\0\0", "missing PE signature");
    ensure!(le16(&pe[4..6]) == PE_MACHINE_ARM64, "EFI image is not ARM64 (0xaa64)");

    let sections = u64::from(le16(&pe[6..8]));
    let optional_size = u64::from(le16(&pe[20..22]));
    ensure!((1..=96).contains(&sections), "invalid PE section count {sections}");
    ensure!(optional_size >= 112 && pe_offset + 24 + optional_size <= size, "invalid PE optional header size {optional_size}");

    let mut optional = [0u8; 112];
    source.read_exact(&mut optional).context("read PE optional header")?;
    ensure!(le16(&optional[0..2]) == PE32_PLUS_MAGIC, "EFI image is not PE32+");
    ensure!(le16(&optional[68..70]) == SUBSYSTEM_EFI_APPLICATION, "PE image is not an EFI application");

    let table_offset = pe_offset + 24 + optional_size;
    ensure!(sections * 40 <= size - table_offset, "PE section table exceeds file size");
    source.seek(SeekFrom::Start(table_offset))?;
    for _ in 0..sections {
        let mut sec = [0u8; 40];
        source.read_exact(&mut sec).context("read PE section header")?;
        let raw_len = u64::from(le32(&sec[16..20]));
        let raw_ptr = u64::from(le32(&sec[20..24]));
        if raw_len > 0 {
            ensure!(raw_ptr <= size && raw_len <= size - raw_ptr, "PE section raw data exceeds file size");
        }
    }
    Ok(size)
}

/// Write `bytes` from `source` to `dest` without truncating or discarding,
/// flush to disk, and compare every written byte back.
pub fn write_and_verify(source: &mut File, dest: &mut File, bytes: u64) -> Result<()> {
    ensure!(bytes > 0, "EFI image is empty");
    source.seek(SeekFrom::Start(0))?;
    dest.seek(SeekFrom::Start(0))?;

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut remaining = bytes;
    while remaining > 0 {
        let count = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("chunk fits usize");
        source.read_exact(&mut buf[..count]).context("read EFI source")?;
        dest.write_all(&buf[..count]).context("write efisp")?;
        remaining -= count as u64;
    }
    dest.sync_all().context("sync efisp")?;

    source.seek(SeekFrom::Start(0))?;
    dest.seek(SeekFrom::Start(0))?;
    let mut actual = vec![0u8; CHUNK_SIZE];
    let mut remaining = bytes;
    while remaining > 0 {
        let count = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("chunk fits usize");
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
#[cfg(target_os = "android")]
pub fn discover_partition() -> Result<(std::path::PathBuf, u64)> {
    for candidate in ["/dev/block/by-name/efisp", "/dev/block/bootdevice/by-name/efisp"] {
        let path = Path::new(candidate);
        if let Ok(mut f) = File::open(path)
            && let Ok(size) = f.seek(SeekFrom::End(0))
            && size > 0
        {
            return Ok((path.to_path_buf(), size));
        }
    }
    bail!("efisp partition not found")
}

/// Flash an ARM64 `.efi` file to the device's `efisp` partition and verify.
#[cfg(target_os = "android")]
pub fn flash(efi_path: &Path) -> Result<u64> {
    let (dev_path, capacity) = discover_partition()?;
    let mut src = File::open(efi_path).with_context(|| format!("open {}", efi_path.display()))?;
    let bytes = validate_arm64_efi(&mut src, capacity)?;
    let mut dst = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&dev_path)
        .with_context(|| format!("open {} for writing", dev_path.display()))?;
    write_and_verify(&mut src, &mut dst, bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_arm64_efi(machine: u16, magic: u16, subsystem: u16, section_end: u32) -> Vec<u8> {
        let mut buf = vec![0u8; 512];
        buf[0..2].copy_from_slice(b"MZ");
        buf[60..64].copy_from_slice(&64u32.to_le_bytes()); // PE offset = 64
        buf[64..68].copy_from_slice(b"PE\0\0");
        buf[68..70].copy_from_slice(&machine.to_le_bytes());
        buf[70..72].copy_from_slice(&1u16.to_le_bytes()); // 1 section
        buf[84..86].copy_from_slice(&112u16.to_le_bytes()); // optional header size
        // Optional header at 88..200
        buf[88..90].copy_from_slice(&magic.to_le_bytes());
        buf[88 + 68..88 + 70].copy_from_slice(&subsystem.to_le_bytes());
        // Section header at 200..240
        buf[200 + 16..200 + 20].copy_from_slice(&section_end.to_le_bytes());
        buf[200 + 20..200 + 24].copy_from_slice(&256u32.to_le_bytes());
        buf
    }

    fn temp_file(name: &str, data: &[u8]) -> (std::path::PathBuf, File) {
        let dir = std::env::temp_dir().join("aweyuki-efisp-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, data).unwrap();
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        (path, f)
    }

    #[test]
    fn valid_arm64_efi_passes_and_writes_with_verification() {
        let efi = synthetic_arm64_efi(PE_MACHINE_ARM64, PE32_PLUS_MAGIC, SUBSYSTEM_EFI_APPLICATION, 128);
        let (_, mut src) = temp_file("valid.efi", &efi);
        let bytes = validate_arm64_efi(&mut src, 4096).unwrap();
        assert_eq!(bytes, 512);

        let (_, mut dst) = temp_file("dest.img", &[0xAAu8; 1024]);
        write_and_verify(&mut src, &mut dst, bytes).unwrap();
        let mut tail = [0u8; 4];
        dst.seek(SeekFrom::Start(512)).unwrap();
        dst.read_exact(&mut tail).unwrap();
        assert_eq!(tail, [0xAA; 4], "bytes after the EFI image stay untouched");
    }

    #[test]
    fn invalid_pe_headers_are_rejected() {
        let bad_arch = synthetic_arm64_efi(0x8664, PE32_PLUS_MAGIC, SUBSYSTEM_EFI_APPLICATION, 128);
        let (_, mut f1) = temp_file("x64.efi", &bad_arch);
        assert!(validate_arm64_efi(&mut f1, 4096).is_err());

        let bad_sub = synthetic_arm64_efi(PE_MACHINE_ARM64, PE32_PLUS_MAGIC, 3, 128);
        let (_, mut f2) = temp_file("win.exe", &bad_sub);
        assert!(validate_arm64_efi(&mut f2, 4096).is_err());

        let oob_sec = synthetic_arm64_efi(PE_MACHINE_ARM64, PE32_PLUS_MAGIC, SUBSYSTEM_EFI_APPLICATION, 1024);
        let (_, mut f3) = temp_file("oob.efi", &oob_sec);
        assert!(validate_arm64_efi(&mut f3, 4096).is_err());
    }
}
