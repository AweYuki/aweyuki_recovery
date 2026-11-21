//! Read-only bootloader (`abl`) inspection to warn when `efisp` is active
//! and an `abl` slot loses its EFISP loader between boot and reboot.

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
#[cfg(target_os = "android")]
use std::path::Path;

const MAX_ABL_BYTES: u64 = 32 * 1024 * 1024;
const PRELIM_READ: usize = 64 * 1024;
const LZMA_HEADER_SIG: &[u8] = &[0x5d, 0x00, 0x00, 0x80, 0x00];

/// Whether the bootloader image carries an EFISP chainloading reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoaderState {
    Present,
    Absent,
    Unknown(String),
}

/// Read-only assessment of one `abl` partition slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotAssessment {
    pub slot: String,
    pub sha256: String,
    pub elf_bytes: u64,
    pub state: LoaderState,
}

/// Warning surfaced before reboot/shutdown when an `abl` slot regresses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AblWarning {
    pub slot: String,
    pub detail: String,
    pub fingerprint: String,
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn le16(b: &[u8], off: usize) -> Result<u16> {
    let s = b.get(off..off + 2).context("truncated ELF header (u16)")?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn le32(b: &[u8], off: usize) -> Result<u32> {
    let s = b.get(off..off + 4).context("truncated ELF header (u32)")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn le64(b: &[u8], off: usize) -> Result<u64> {
    let s = b.get(off..off + 8).context("truncated ELF header (u64)")?;
    Ok(u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn add_extent(max: &mut u64, off: u64, size: u64) -> Result<()> {
    let end = off.checked_add(size).context("ELF extent overflow")?;
    *max = (*max).max(end);
    Ok(())
}

/// Compute the byte extent covered by an ELF32 or ELF64 image's header,
/// program headers/segments, and non-`SHT_NOBITS` sections.
///
/// # Errors
///
/// Returns an error if `bytes` is not a valid little-endian ELF32/ELF64 image
/// or header arithmetic overflows.
pub fn elf_extent(bytes: &[u8]) -> Result<u64> {
    ensure!(bytes.len() >= 52 && &bytes[0..4] == b"\x7fELF", "not an ELF image");
    let class = bytes[4];
    ensure!(class == 1 || class == 2, "unsupported ELF class {class}");
    ensure!(bytes[5] == 1, "unsupported ELF endianness {}", bytes[5]);

    if class == 1 {
        let mut max = 52u64;
        let phoff = u64::from(le32(bytes, 28)?);
        let shoff = u64::from(le32(bytes, 32)?);
        let phentsize = u64::from(le16(bytes, 42)?);
        let phnum = u64::from(le16(bytes, 44)?);
        let shentsize = u64::from(le16(bytes, 46)?);
        let shnum = u64::from(le16(bytes, 48)?);

        if phoff > 0 && phnum > 0 {
            let ph_tbl = phentsize.checked_mul(phnum).context("ELF32 phdr size overflow")?;
            add_extent(&mut max, phoff, ph_tbl)?;
            if phentsize >= 20 {
                for i in 0..phnum {
                    let Some(off) = phoff.checked_add(i * phentsize).and_then(|v| usize::try_from(v).ok()) else {
                        break;
                    };
                    if off + 20 > bytes.len() {
                        break;
                    }
                    let p_offset = u64::from(le32(bytes, off + 4)?);
                    let p_filesz = u64::from(le32(bytes, off + 16)?);
                    add_extent(&mut max, p_offset, p_filesz)?;
                }
            }
        }
        if shoff > 0 && shnum > 0 {
            let sh_tbl = shentsize.checked_mul(shnum).context("ELF32 shdr size overflow")?;
            add_extent(&mut max, shoff, sh_tbl)?;
            if shentsize >= 24 {
                for i in 0..shnum {
                    let Some(off) = shoff.checked_add(i * shentsize).and_then(|v| usize::try_from(v).ok()) else {
                        break;
                    };
                    if off + 24 > bytes.len() {
                        break;
                    }
                    let sh_type = le32(bytes, off + 4)?;
                    if sh_type != 8 {
                        let sh_offset = u64::from(le32(bytes, off + 16)?);
                        let sh_size = u64::from(le32(bytes, off + 20)?);
                        add_extent(&mut max, sh_offset, sh_size)?;
                    }
                }
            }
        }
        Ok(max)
    } else {
        ensure!(bytes.len() >= 64, "truncated ELF64 header");
        let mut max = 64u64;
        let phoff = le64(bytes, 32)?;
        let shoff = le64(bytes, 40)?;
        let phentsize = u64::from(le16(bytes, 54)?);
        let phnum = u64::from(le16(bytes, 56)?);
        let shentsize = u64::from(le16(bytes, 58)?);
        let shnum = u64::from(le16(bytes, 60)?);

        if phoff > 0 && phnum > 0 {
            let ph_tbl = phentsize.checked_mul(phnum).context("ELF64 phdr size overflow")?;
            add_extent(&mut max, phoff, ph_tbl)?;
            if phentsize >= 40 {
                for i in 0..phnum {
                    let Some(off) = phoff.checked_add(i * phentsize).and_then(|v| usize::try_from(v).ok()) else {
                        break;
                    };
                    if off + 40 > bytes.len() {
                        break;
                    }
                    let p_offset = le64(bytes, off + 8)?;
                    let p_filesz = le64(bytes, off + 32)?;
                    add_extent(&mut max, p_offset, p_filesz)?;
                }
            }
        }
        if shoff > 0 && shnum > 0 {
            let sh_tbl = shentsize.checked_mul(shnum).context("ELF64 shdr size overflow")?;
            add_extent(&mut max, shoff, sh_tbl)?;
            if shentsize >= 40 {
                for i in 0..shnum {
                    let Some(off) = shoff.checked_add(i * shentsize).and_then(|v| usize::try_from(v).ok()) else {
                        break;
                    };
                    if off + 40 > bytes.len() {
                        break;
                    }
                    let sh_type = le32(bytes, off + 4)?;
                    if sh_type != 8 {
                        let sh_offset = le64(bytes, off + 24)?;
                        let sh_size = le64(bytes, off + 32)?;
                        add_extent(&mut max, sh_offset, sh_size)?;
                    }
                }
            }
        }
        Ok(max)
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn classify_abl(bytes: &[u8]) -> LoaderState {
    const MARKERS: [&[u8]; 5] = [
        b"efisp",
        b"EFISP",
        b"\\EFI\\BOOT\\BOOTAA64.EFI",
        b"e\0f\0i\0s\0p\0",
        b"E\0F\0I\0S\0P\0",
    ];
    if MARKERS.iter().any(|m| contains_subslice(bytes, m)) {
        return LoaderState::Present;
    }
    if contains_subslice(bytes, b"_FVH") || contains_subslice(bytes, LZMA_HEADER_SIG) {
        return LoaderState::Unknown("compressed UEFI volume; cannot inspect without LZMA".into());
    }
    LoaderState::Absent
}

fn read_exact_at_start<R: Read + Seek>(reader: &mut R, len: u64) -> Result<Vec<u8>> {
    ensure!(len <= MAX_ABL_BYTES, "ABL ELF extent {len} exceeds 32 MiB limit");
    reader.seek(SeekFrom::Start(0)).context("rewind ABL image")?;
    let cap = usize::try_from(len).context("ABL size exceeds addressable memory")?;
    let mut buf = vec![0u8; cap];
    reader.read_exact(&mut buf).context("read ABL ELF image")?;
    Ok(buf)
}

/// Inspect one `abl` image through `reader` without modifying it.
///
/// # Errors
///
/// Returns an error if reading fails, the stream does not start with an ELF
/// image, or its extent exceeds 32 MiB.
pub fn assess_reader<R: Read + Seek>(slot: &str, reader: &mut R) -> Result<SlotAssessment> {
    reader.seek(SeekFrom::Start(0)).context("rewind ABL header")?;
    let mut head = Vec::with_capacity(PRELIM_READ);
    reader.take(PRELIM_READ as u64).read_to_end(&mut head).context("read ABL header")?;
    let prelim = elf_extent(&head)?;
    let mut elf = read_exact_at_start(reader, prelim)?;
    let exact = elf_extent(&elf)?;
    if exact > prelim {
        elf = read_exact_at_start(reader, exact)?;
    }

    let mut hasher = Sha256::new();
    hasher.update(&elf);
    let sha256 = hex_lower(&hasher.finalize());
    let state = classify_abl(&elf);

    Ok(SlotAssessment {
        slot: slot.to_string(),
        sha256,
        elf_bytes: exact,
        state,
    })
}

/// Read-only assessment of `/dev/block/by-name/abl_a`, `abl_b`, or `abl`.
#[cfg(target_os = "android")]
#[must_use]
pub fn assess_device() -> Vec<SlotAssessment> {
    let by_name = Path::new(aweyuki_block::BY_NAME);
    let mut found = Vec::new();
    for (slot, name) in [("a", "abl_a"), ("b", "abl_b")] {
        let path = by_name.join(name);
        if let Ok(mut f) = File::open(&path) {
            match assess_reader(slot, &mut f) {
                Ok(a) => found.push(a),
                Err(e) => found.push(SlotAssessment {
                    slot: slot.into(),
                    sha256: String::new(),
                    elf_bytes: 0,
                    state: LoaderState::Unknown(format!("{e:#}")),
                }),
            }
        }
    }
    if found.is_empty() {
        let path = by_name.join("abl");
        if let Ok(mut f) = File::open(&path) {
            match assess_reader("abl", &mut f) {
                Ok(a) => found.push(a),
                Err(e) => found.push(SlotAssessment {
                    slot: "abl".into(),
                    sha256: String::new(),
                    elf_bytes: 0,
                    state: LoaderState::Unknown(format!("{e:#}")),
                }),
            }
        }
    }
    found
}

/// Compare boot-time `abl` assessments against the current ones before leaving
/// recovery, warning if a slot that had the EFISP loader lost it or became
/// unreadable/unverifiable.
#[must_use]
pub fn check_regression(boot: &[SlotAssessment], current: &[SlotAssessment]) -> Option<AblWarning> {
    let mut fp_map = BTreeMap::new();
    let mut by_slot = BTreeMap::new();
    for item in current {
        fp_map.insert(item.slot.as_str(), item.sha256.as_str());
        by_slot.insert(item.slot.as_str(), item);
    }
    let fingerprint = fp_map
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(";");

    for before in boot {
        if before.state != LoaderState::Present {
            continue;
        }
        match by_slot.get(before.slot.as_str()) {
            Some(now) if now.state == LoaderState::Present => {}
            Some(now) => {
                let detail = match &now.state {
                    LoaderState::Absent => "EFISP chainloader marker is no longer present in this abl slot".to_string(),
                    LoaderState::Unknown(why) => why.clone(),
                    LoaderState::Present => unreachable!(),
                };
                return Some(AblWarning {
                    slot: before.slot.clone(),
                    detail,
                    fingerprint,
                });
            }
            None => {
                return Some(AblWarning {
                    slot: before.slot.clone(),
                    detail: "abl partition slot is no longer accessible".to_string(),
                    fingerprint,
                });
            }
        }
    }
    None
}

/// Open and verify an ELF extent directly from `path` (convenience wrapper).
///
/// # Errors
///
/// Returns an error if the file cannot be opened or is not a valid ABL ELF.
pub fn inspect_file(slot: &str, path: &std::path::Path) -> Result<SlotAssessment> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    if file.seek(SeekFrom::End(0)).is_err() {
        bail!("cannot seek {}", path.display());
    }
    assess_reader(slot, &mut file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn synthetic_elf64(payload: &[u8]) -> Vec<u8> {
        let phoff = 64u64;
        let phentsize = 56u16;
        let payload_off = 64 + 56;
        let total = payload_off + payload.len();
        let mut buf = vec![0u8; total];
        buf[0..4].copy_from_slice(b"\x7fELF");
        buf[4] = 2; // ELF64
        buf[5] = 1; // little endian
        buf[32..40].copy_from_slice(&phoff.to_le_bytes());
        buf[54..56].copy_from_slice(&phentsize.to_le_bytes());
        buf[56..58].copy_from_slice(&1u16.to_le_bytes());

        // One PT_LOAD program header at offset 64
        buf[64..68].copy_from_slice(&1u32.to_le_bytes());
        buf[64 + 8..64 + 16].copy_from_slice(&(payload_off as u64).to_le_bytes());
        buf[64 + 32..64 + 40].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        buf[payload_off..].copy_from_slice(payload);
        buf
    }

    #[test]
    fn assesses_elf_extent_and_efisp_markers() {
        let with_ascii = synthetic_elf64(b"\0\0efisp\0");
        let a = assess_reader("a", &mut Cursor::new(&with_ascii)).unwrap();
        assert_eq!(a.state, LoaderState::Present);
        assert_eq!(a.elf_bytes, with_ascii.len() as u64);

        let with_utf16 = synthetic_elf64(b"E\0F\0I\0S\0P\0");
        let u = assess_reader("b", &mut Cursor::new(&with_utf16)).unwrap();
        assert_eq!(u.state, LoaderState::Present);

        let plain = synthetic_elf64(b"ordinary android bootloader strings");
        let p = assess_reader("a", &mut Cursor::new(&plain)).unwrap();
        assert_eq!(p.state, LoaderState::Absent);

        let compressed = synthetic_elf64(b"header _FVH compressed body");
        let c = assess_reader("a", &mut Cursor::new(&compressed)).unwrap();
        assert!(matches!(c.state, LoaderState::Unknown(_)));
    }

    #[test]
    fn detects_abl_regression_between_boot_and_reboot() {
        let present = assess_reader("a", &mut Cursor::new(synthetic_elf64(b"efisp"))).unwrap();
        let absent = assess_reader("a", &mut Cursor::new(synthetic_elf64(b"stock"))).unwrap();
        let unknown = assess_reader("a", &mut Cursor::new(synthetic_elf64(b"_FVH"))).unwrap();

        assert_eq!(check_regression(std::slice::from_ref(&present), std::slice::from_ref(&present)), None);
        let warn_absent = check_regression(std::slice::from_ref(&present), &[absent]).unwrap();
        assert_eq!(warn_absent.slot, "a");
        assert!(!warn_absent.fingerprint.is_empty());

        let warn_unknown = check_regression(&[present], &[unknown]).unwrap();
        assert!(warn_unknown.detail.contains("LZMA"));
    }
}
