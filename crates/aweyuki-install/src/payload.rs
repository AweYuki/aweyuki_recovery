//! Pure-Rust AOSP `payload.bin` inspector, Anti-Rollback (ARB) checker, and
//! full-OTA partition extractor (`REPLACE`, `ZERO`, `DISCARD`, `REPLACE_ZSTD`).

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, ZipArchive};

const PAYLOAD_MAGIC: &[u8] = b"CrAU";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const COPY_BUF: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Replace,
    ReplaceBz,
    Zero,
    Discard,
    ReplaceXz,
    ReplaceZstd,
    Other(u32),
}

impl OpKind {
    fn from_u32(v: u32) -> Self {
        match v {
            0 => OpKind::Replace,
            1 => OpKind::ReplaceBz,
            6 => OpKind::Zero,
            7 => OpKind::Discard,
            8 => OpKind::ReplaceXz,
            14 => OpKind::ReplaceZstd,
            other => OpKind::Other(other),
        }
    }

    fn codec_name(self) -> &'static str {
        match self {
            OpKind::ReplaceBz => "bz2",
            OpKind::ReplaceXz => "xz",
            OpKind::Other(_) => "delta",
            OpKind::Replace | OpKind::Zero | OpKind::Discard | OpKind::ReplaceZstd => "supported",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub start_block: u64,
    pub num_blocks: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOp {
    pub kind: OpKind,
    pub data_offset: u64,
    pub data_length: u64,
    pub dst_extents: Vec<Extent>,
    pub data_sha256: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartitionInfo {
    pub name: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub version: Option<String>,
    pub ops: Vec<InstallOp>,
    pub extractable: bool,
    pub unsupported_codecs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbStatus {
    pub package_timestamp: Option<i64>,
    pub device_timestamp: Option<i64>,
    pub package_patch: Option<String>,
    pub device_patch: Option<String>,
    pub downgrade: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadReport {
    pub path: PathBuf,
    pub block_size: u32,
    pub minor_version: u32,
    pub data_offset: u64,
    pub security_patch: Option<String>,
    pub max_timestamp: Option<i64>,
    pub pre_device: Option<String>,
    pub ota_type: Option<String>,
    pub arb: ArbStatus,
    pub partitions: Vec<PartitionInfo>,
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let mut out = 0u64;
    let mut shift = 0u32;
    loop {
        let &b = buf.get(*pos).context("truncated protobuf varint")?;
        *pos += 1;
        out |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(out);
        }
        shift += 7;
        ensure!(shift < 64, "protobuf varint overflow");
    }
}

fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8]> {
    let len = usize::try_from(read_varint(buf, pos)?).context("protobuf length exceeds usize")?;
    let end = pos.checked_add(len).context("protobuf slice overflow")?;
    let slice = buf.get(*pos..end).context("truncated protobuf bytes")?;
    *pos = end;
    Ok(slice)
}

fn skip_field(buf: &[u8], pos: &mut usize, wire: u64) -> Result<()> {
    match wire {
        0 => {
            let _ = read_varint(buf, pos)?;
        }
        1 => {
            ensure!(*pos + 8 <= buf.len(), "truncated 64-bit protobuf field");
            *pos += 8;
        }
        2 => {
            let _ = read_bytes(buf, pos)?;
        }
        5 => {
            ensure!(*pos + 4 <= buf.len(), "truncated 32-bit protobuf field");
            *pos += 4;
        }
        other => bail!("unsupported protobuf wire type {other}"),
    }
    Ok(())
}

fn decode_extent(buf: &[u8]) -> Result<Extent> {
    let (mut pos, mut start_block, mut num_blocks) = (0, 0, 0);
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let (field, wire) = (tag >> 3, tag & 7);
        match (field, wire) {
            (1, 0) => start_block = read_varint(buf, &mut pos)?,
            (2, 0) => num_blocks = read_varint(buf, &mut pos)?,
            _ => skip_field(buf, &mut pos, wire)?,
        }
    }
    Ok(Extent { start_block, num_blocks })
}

fn decode_install_op(buf: &[u8]) -> Result<InstallOp> {
    let (mut pos, mut kind) = (0, OpKind::Replace);
    let (mut data_offset, mut data_length) = (0, 0);
    let mut dst_extents = Vec::new();
    let mut data_sha256 = None;
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let (field, wire) = (tag >> 3, tag & 7);
        match (field, wire) {
            (1, 0) => kind = OpKind::from_u32(read_varint(buf, &mut pos)? as u32),
            (2, 0) => data_offset = read_varint(buf, &mut pos)?,
            (3, 0) => data_length = read_varint(buf, &mut pos)?,
            (6, 2) => dst_extents.push(decode_extent(read_bytes(buf, &mut pos)?)?),
            (7, 2) => data_sha256 = Some(read_bytes(buf, &mut pos)?.to_vec()),
            _ => skip_field(buf, &mut pos, wire)?,
        }
    }
    Ok(InstallOp {
        kind,
        data_offset,
        data_length,
        dst_extents,
        data_sha256,
    })
}

fn decode_partition_info(buf: &[u8]) -> Result<(u64, Option<String>)> {
    let (mut pos, mut size, mut sha256) = (0, 0, None);
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let (field, wire) = (tag >> 3, tag & 7);
        match (field, wire) {
            (1, 0) => size = read_varint(buf, &mut pos)?,
            (2, 2) => sha256 = Some(crate::abl::hex_lower(read_bytes(buf, &mut pos)?)),
            _ => skip_field(buf, &mut pos, wire)?,
        }
    }
    Ok((size, sha256))
}

fn decode_partition_update(buf: &[u8], minor_version: u32) -> Result<PartitionInfo> {
    let mut pos = 0;
    let mut name = String::new();
    let mut size = 0;
    let mut sha256 = None;
    let mut version = None;
    let mut ops = Vec::new();
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let (field, wire) = (tag >> 3, tag & 7);
        match (field, wire) {
            (1, 2) => name = String::from_utf8(read_bytes(buf, &mut pos)?.to_vec()).context("non-UTF8 partition name")?,
            (7, 2) => (size, sha256) = decode_partition_info(read_bytes(buf, &mut pos)?)?,
            (8, 2) => ops.push(decode_install_op(read_bytes(buf, &mut pos)?)?),
            (9, 2) => version = Some(String::from_utf8(read_bytes(buf, &mut pos)?.to_vec()).context("non-UTF8 partition version")?),
            _ => skip_field(buf, &mut pos, wire)?,
        }
    }
    let mut unsupported = BTreeSet::new();
    if minor_version != 0 {
        unsupported.insert("delta".to_string());
    }
    for op in &ops {
        if !matches!(op.kind, OpKind::Replace | OpKind::Zero | OpKind::Discard | OpKind::ReplaceZstd) {
            unsupported.insert(op.kind.codec_name().to_string());
        }
    }
    let extractable = unsupported.is_empty();
    Ok(PartitionInfo {
        name,
        size,
        sha256,
        version,
        ops,
        extractable,
        unsupported_codecs: unsupported.into_iter().collect(),
    })
}

struct RawManifest {
    block_size: u32,
    minor_version: u32,
    partitions: Vec<PartitionInfo>,
    max_timestamp: Option<i64>,
    security_patch: Option<String>,
}

fn decode_manifest(buf: &[u8]) -> Result<RawManifest> {
    let mut pos = 0;
    let mut block_size = 4096u32;
    let mut minor_version = 0u32;
    let mut raw_parts: Vec<&[u8]> = Vec::new();
    let mut max_timestamp = None;
    let mut security_patch = None;

    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let (field, wire) = (tag >> 3, tag & 7);
        match (field, wire) {
            (3, 0) => block_size = read_varint(buf, &mut pos)? as u32,
            (12, 0) => minor_version = read_varint(buf, &mut pos)? as u32,
            (13, 2) => raw_parts.push(read_bytes(buf, &mut pos)?),
            (14, 0) => max_timestamp = Some(read_varint(buf, &mut pos)? as i64),
            (18, 2) => {
                let s = String::from_utf8(read_bytes(buf, &mut pos)?.to_vec()).context("non-UTF8 security patch")?;
                if !s.is_empty() {
                    security_patch = Some(s);
                }
            }
            _ => skip_field(buf, &mut pos, wire)?,
        }
    }
    let mut partitions = Vec::with_capacity(raw_parts.len());
    for raw in raw_parts {
        partitions.push(decode_partition_update(raw, minor_version)?);
    }
    Ok(RawManifest {
        block_size,
        minor_version,
        partitions,
        max_timestamp,
        security_patch,
    })
}

struct OpenedPayload {
    file: File,
    base_offset: u64,
    pre_device: Option<String>,
    ota_type: Option<String>,
    meta_timestamp: Option<i64>,
    meta_patch: Option<String>,
}

fn open_payload_source(path: &Path) -> Result<OpenedPayload> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).with_context(|| format!("read header of {}", path.display()))?;
    file.seek(SeekFrom::Start(0))?;

    if &magic == b"PK\x03\x04" {
        let mut zip = ZipArchive::new(BufReader::new(file)).context("open OTA zip")?;
        let meta_text = zip
            .by_name("META-INF/com/android/metadata")
            .ok()
            .and_then(|mut entry| {
                let mut s = String::new();
                entry.read_to_string(&mut s).ok().map(|_| s)
            });
        let meta = meta_text.as_deref().map(crate::package::Metadata::parse).unwrap_or_default();
        let pre_device = meta.get("pre-device").map(str::to_string);
        let ota_type = meta.get("ota-type").map(str::to_string);
        let meta_timestamp = meta.get("post-timestamp").and_then(|v| v.parse::<i64>().ok());
        let meta_patch = meta.get("post-security-patch-level").map(str::to_string);

        let payload = zip.by_name("payload.bin").context("OTA zip has no payload.bin")?;
        ensure!(
            payload.compression() == CompressionMethod::Stored,
            "payload.bin is compressed ({:?}), cannot inspect or extract directly",
            payload.compression()
        );
        let base_offset = payload.data_start().context("payload.bin has no data offset")?;
        drop(payload);
        let inner = zip.into_inner().into_inner();
        return Ok(OpenedPayload {
            file: inner,
            base_offset,
            pre_device,
            ota_type,
            meta_timestamp,
            meta_patch,
        });
    }

    Ok(OpenedPayload {
        file,
        base_offset: 0,
        pre_device: None,
        ota_type: None,
        meta_timestamp: None,
        meta_patch: None,
    })
}

fn parse_header_and_manifest(file: &mut File, base_offset: u64) -> Result<(u64, RawManifest)> {
    file.seek(SeekFrom::Start(base_offset)).context("seek to payload header")?;
    let mut header = [0u8; 24];
    file.read_exact(&mut header).context("read CrAU header")?;
    ensure!(&header[0..4] == PAYLOAD_MAGIC, "not an AOSP payload.bin (missing CrAU magic)");

    let major = u64::from_be_bytes(header[4..12].try_into().unwrap());
    ensure!(major == 2, "unsupported payload major version {major} (expected 2)");
    let manifest_size = u64::from_be_bytes(header[12..20].try_into().unwrap());
    let sig_size = u64::from_be_bytes([0, 0, 0, 0, header[20], header[21], header[22], header[23]]);
    ensure!(manifest_size > 0 && manifest_size <= MAX_MANIFEST_BYTES, "invalid payload manifest size {manifest_size}");

    let len = usize::try_from(manifest_size).context("manifest exceeds addressable memory")?;
    let mut manifest_buf = vec![0u8; len];
    file.read_exact(&mut manifest_buf).context("read payload manifest")?;
    let manifest = decode_manifest(&manifest_buf)?;
    let data_offset = 24u64
        .checked_add(manifest_size)
        .and_then(|v| v.checked_add(sig_size))
        .context("payload data offset overflow")?;
    Ok((data_offset, manifest))
}

fn valid_patch_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Inspect an AOSP `payload.bin` or OTA `.zip` package without modifying the
/// device, returning partition details and Anti-Rollback (ARB) comparison.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, is not a `CrAU` v2 payload or
/// stored OTA zip, or contains malformed protobuf metadata.
pub fn inspect_path(path: &Path, device_timestamp: Option<i64>, device_patch: Option<&str>) -> Result<PayloadReport> {
    let mut src = open_payload_source(path)?;
    let (data_offset, manifest) = parse_header_and_manifest(&mut src.file, src.base_offset)?;

    let pkg_ts = manifest.max_timestamp.or(src.meta_timestamp);
    let pkg_patch = manifest.security_patch.clone().or(src.meta_patch);
    let dev_patch = device_patch.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

    let ts_downgrade = matches!((pkg_ts, device_timestamp), (Some(p), Some(d)) if p < d);
    let patch_downgrade = matches!(
        (pkg_patch.as_deref(), dev_patch.as_deref()),
        (Some(p), Some(d)) if valid_patch_date(p) && valid_patch_date(d) && p < d
    );

    Ok(PayloadReport {
        path: path.to_path_buf(),
        block_size: manifest.block_size.max(512),
        minor_version: manifest.minor_version,
        data_offset,
        security_patch: pkg_patch.clone(),
        max_timestamp: pkg_ts,
        pre_device: src.pre_device,
        ota_type: src.ota_type,
        arb: ArbStatus {
            package_timestamp: pkg_ts,
            device_timestamp,
            package_patch: pkg_patch,
            device_patch: dev_patch,
            downgrade: ts_downgrade || patch_downgrade,
        },
        partitions: manifest.partitions,
    })
}

struct HashingReader<'a, R> {
    inner: R,
    hasher: &'a mut Sha256,
}

impl<R: Read> Read for HashingReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.hasher.update(&buf[..n]);
        }
        Ok(n)
    }
}

struct ExtentWriter<'a> {
    file: &'a mut File,
    block_size: u64,
    extents: &'a [Extent],
    extent_idx: usize,
    extent_pos: u64,
    total_written: u64,
    max_bytes: u64,
}

impl<'a> ExtentWriter<'a> {
    fn new(file: &'a mut File, block_size: u64, extents: &'a [Extent], max_bytes: u64) -> Self {
        ExtentWriter {
            file,
            block_size,
            extents,
            extent_idx: 0,
            extent_pos: 0,
            total_written: 0,
            max_bytes,
        }
    }
}

impl Write for ExtentWriter<'_> {
    fn write(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        let original = buf.len();
        while !buf.is_empty() && self.total_written < self.max_bytes {
            while self.extent_idx < self.extents.len() {
                let ext_cap = self.extents[self.extent_idx].num_blocks.saturating_mul(self.block_size);
                if self.extent_pos < ext_cap {
                    break;
                }
                self.extent_idx += 1;
                self.extent_pos = 0;
            }
            if self.extent_idx >= self.extents.len() {
                break;
            }
            let ext = self.extents[self.extent_idx];
            let ext_cap = ext.num_blocks.saturating_mul(self.block_size);
            let ext_rem = ext_cap - self.extent_pos;
            let global_rem = self.max_bytes - self.total_written;
            let take = (buf.len() as u64).min(ext_rem).min(global_rem) as usize;
            if take == 0 {
                break;
            }
            let disk_off = ext.start_block.saturating_mul(self.block_size).saturating_add(self.extent_pos);
            self.file.seek(SeekFrom::Start(disk_off))?;
            self.file.write_all(&buf[..take])?;
            self.extent_pos += take as u64;
            self.total_written += take as u64;
            buf = &buf[take..];
        }
        Ok(original)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn partial_path(out_path: &Path) -> PathBuf {
    let mut name = out_path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    out_path.with_file_name(name)
}

/// Extract a single partition image from `payload_path` into `out_path`,
/// verifying per-operation and full-partition SHA-256 digests.
///
/// # Errors
///
/// Returns an error if the partition is absent, uses delta or unsupported
/// compression codecs, fails SHA-256 verification, or cannot write `out_path`.
pub fn extract_partition(
    payload_path: &Path,
    partition_name: &str,
    out_path: &Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<u64> {
    let mut src = open_payload_source(payload_path)?;
    let (data_offset, manifest) = parse_header_and_manifest(&mut src.file, src.base_offset)?;
    let block_size = u64::from(manifest.block_size.max(512));
    let part = manifest
        .partitions
        .into_iter()
        .find(|p| p.name == partition_name)
        .with_context(|| format!("partition {partition_name:?} not found in payload"))?;
    ensure!(
        part.extractable,
        "cannot extract {}: unsupported payload mode ({})",
        part.name,
        part.unsupported_codecs.join(", ")
    );

    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let tmp = partial_path(out_path);
    let _ = fs::remove_file(&tmp);

    let extracted = (|| -> Result<u64> {
        let mut out = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        out.set_len(part.size).context("size output partition image")?;

        let zeros = vec![0u8; COPY_BUF];
        let mut bytes_done = 0u64;
        on_progress(0, part.size);

        for op in &part.ops {
            let max_op_bytes: u64 = op
                .dst_extents
                .iter()
                .map(|e| e.num_blocks.saturating_mul(block_size))
                .fold(0u64, u64::saturating_add);
            let mut writer = ExtentWriter::new(&mut out, block_size, &op.dst_extents, max_op_bytes);

            match op.kind {
                OpKind::Zero | OpKind::Discard => {
                    let mut rem = max_op_bytes;
                    while rem > 0 {
                        let n = rem.min(zeros.len() as u64) as usize;
                        writer.write_all(&zeros[..n])?;
                        rem -= n as u64;
                    }
                }
                OpKind::Replace => {
                    let off = src
                        .base_offset
                        .checked_add(data_offset)
                        .and_then(|v| v.checked_add(op.data_offset))
                        .context("op offset overflow")?;
                    src.file.seek(SeekFrom::Start(off)).context("seek payload op")?;
                    let mut hasher = Sha256::new();
                    let mut hr = HashingReader {
                        inner: (&mut src.file).take(op.data_length),
                        hasher: &mut hasher,
                    };
                    io::copy(&mut hr, &mut writer).context("copy REPLACE op")?;
                    io::copy(&mut hr, &mut io::sink()).context("finish REPLACE hash")?;
                    if let Some(expected) = &op.data_sha256 {
                        ensure!(hasher.finalize().as_slice() == expected.as_slice(), "op SHA-256 mismatch in {}", part.name);
                    }
                }
                OpKind::ReplaceZstd => {
                    let off = src
                        .base_offset
                        .checked_add(data_offset)
                        .and_then(|v| v.checked_add(op.data_offset))
                        .context("op offset overflow")?;
                    if let Some(expected) = &op.data_sha256 {
                        src.file.seek(SeekFrom::Start(off)).context("seek zstd payload op")?;
                        let mut hasher = Sha256::new();
                        let mut hr = HashingReader {
                            inner: (&mut src.file).take(op.data_length),
                            hasher: &mut hasher,
                        };
                        io::copy(&mut hr, &mut io::sink()).context("hash zstd payload op")?;
                        ensure!(
                            hasher.finalize().as_slice() == expected.as_slice(),
                            "zstd op SHA-256 mismatch in {}",
                            part.name
                        );
                    }
                    src.file.seek(SeekFrom::Start(off)).context("seek zstd payload op")?;
                    let mut decoder = zstd::stream::read::Decoder::with_buffer(
                        io::BufReader::with_capacity(COPY_BUF, (&mut src.file).take(op.data_length)),
                    )
                    .context("init zstd payload decoder")?;
                    io::copy(&mut decoder, &mut writer).context("decompress REPLACE_ZSTD op")?;
                }
                other => bail!("unsupported op {other:?}"),
            }
            bytes_done = (bytes_done + max_op_bytes).min(part.size);
            on_progress(bytes_done, part.size);
        }

        out.set_len(part.size).context("truncate extracted image")?;
        out.sync_all().context("sync extracted image")?;

        if let Some(expected_hex) = &part.sha256 {
            out.seek(SeekFrom::Start(0))?;
            let mut full = Sha256::new();
            let mut buf = vec![0u8; COPY_BUF];
            let mut rem = part.size;
            while rem > 0 {
                let n = rem.min(buf.len() as u64) as usize;
                out.read_exact(&mut buf[..n]).context("verify extracted image")?;
                full.update(&buf[..n]);
                rem -= n as u64;
            }
            let actual = crate::abl::hex_lower(&full.finalize());
            ensure!(&actual == expected_hex, "extracted {} SHA-256 mismatch ({actual} != {expected_hex})", part.name);
        }
        Ok(part.size)
    })();

    match extracted {
        Ok(size) => {
            fs::rename(&tmp, out_path).with_context(|| format!("move {} to {}", tmp.display(), out_path.display()))?;
            Ok(size)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn push_varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push(((v as u8) & 0x7f) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    fn push_field_varint(out: &mut Vec<u8>, field: u64, v: u64) {
        push_varint(out, field << 3);
        push_varint(out, v);
    }

    fn push_field_bytes(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
        push_varint(out, (field << 3) | 2);
        push_varint(out, bytes.len() as u64);
        out.extend_from_slice(bytes);
    }

    fn synthetic_payload(boot_data: &[u8], vbmeta_plain: &[u8], max_ts: i64, patch: &str) -> Vec<u8> {
        let zstd_vbmeta = zstd::encode_all(vbmeta_plain, 3).unwrap();
        let boot_sha: [u8; 32] = Sha256::digest(boot_data).into();
        let zstd_sha: [u8; 32] = Sha256::digest(&zstd_vbmeta).into();

        // Expected extracted boot = boot_data + 4096 zeros
        let mut full_boot = boot_data.to_vec();
        full_boot.resize(8192, 0);
        let full_boot_sha: [u8; 32] = Sha256::digest(&full_boot).into();
        let full_vbmeta_sha: [u8; 32] = Sha256::digest(vbmeta_plain).into();

        // Partition 1: boot (op0 REPLACE block 0, op1 ZERO block 1)
        let mut ext0 = Vec::new();
        push_field_varint(&mut ext0, 1, 0);
        push_field_varint(&mut ext0, 2, 1);
        let mut op0 = Vec::new();
        push_field_varint(&mut op0, 1, 0); // REPLACE
        push_field_varint(&mut op0, 2, 0);
        push_field_varint(&mut op0, 3, boot_data.len() as u64);
        push_field_bytes(&mut op0, 6, &ext0);
        push_field_bytes(&mut op0, 7, &boot_sha);

        let mut ext1 = Vec::new();
        push_field_varint(&mut ext1, 1, 1);
        push_field_varint(&mut ext1, 2, 1);
        let mut op1 = Vec::new();
        push_field_varint(&mut op1, 1, 6); // ZERO
        push_field_bytes(&mut op1, 6, &ext1);

        let mut boot_info = Vec::new();
        push_field_varint(&mut boot_info, 1, 8192);
        push_field_bytes(&mut boot_info, 2, &full_boot_sha);
        let mut boot_upd = Vec::new();
        push_field_bytes(&mut boot_upd, 1, b"boot");
        push_field_bytes(&mut boot_upd, 7, &boot_info);
        push_field_bytes(&mut boot_upd, 8, &op0);
        push_field_bytes(&mut boot_upd, 8, &op1);

        // Partition 2: vbmeta (op REPLACE_ZSTD block 0)
        let mut ext_v = Vec::new();
        push_field_varint(&mut ext_v, 1, 0);
        push_field_varint(&mut ext_v, 2, 1);
        let mut op_v = Vec::new();
        push_field_varint(&mut op_v, 1, 14); // REPLACE_ZSTD
        push_field_varint(&mut op_v, 2, boot_data.len() as u64);
        push_field_varint(&mut op_v, 3, zstd_vbmeta.len() as u64);
        push_field_bytes(&mut op_v, 6, &ext_v);
        push_field_bytes(&mut op_v, 7, &zstd_sha);

        let mut vb_info = Vec::new();
        push_field_varint(&mut vb_info, 1, vbmeta_plain.len() as u64);
        push_field_bytes(&mut vb_info, 2, &full_vbmeta_sha);
        let mut vb_upd = Vec::new();
        push_field_bytes(&mut vb_upd, 1, b"vbmeta");
        push_field_bytes(&mut vb_upd, 7, &vb_info);
        push_field_bytes(&mut vb_upd, 8, &op_v);

        let mut manifest = Vec::new();
        push_field_varint(&mut manifest, 3, 4096);
        push_field_varint(&mut manifest, 12, 0);
        push_field_bytes(&mut manifest, 13, &boot_upd);
        push_field_bytes(&mut manifest, 13, &vb_upd);
        push_field_varint(&mut manifest, 14, max_ts as u64);
        push_field_bytes(&mut manifest, 18, patch.as_bytes());

        let mut payload = Vec::new();
        payload.extend_from_slice(PAYLOAD_MAGIC);
        payload.extend_from_slice(&2u64.to_be_bytes());
        payload.extend_from_slice(&(manifest.len() as u64).to_be_bytes());
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&manifest);
        payload.extend_from_slice(boot_data);
        payload.extend_from_slice(&zstd_vbmeta);
        payload
    }

    #[test]
    fn inspects_arb_and_extracts_partitions_from_zip_and_bin() {
        let dir = tempfile::tempdir().unwrap();
        let boot_block = vec![0x42u8; 4096];
        let vbmeta_raw = vec![0x19u8; 2048];
        let raw_payload = synthetic_payload(&boot_block, &vbmeta_raw, 1_750_000_000, "2026-08-05");

        let bin_path = dir.path().join("payload.bin");
        fs::write(&bin_path, &raw_payload).unwrap();

        let ok_rep = inspect_path(&bin_path, Some(1_700_000_000), Some("2026-07-05")).unwrap();
        assert!(!ok_rep.arb.downgrade);
        assert_eq!(ok_rep.partitions.len(), 2);
        assert!(ok_rep.partitions.iter().all(|p| p.extractable));

        let down_rep = inspect_path(&bin_path, Some(1_800_000_000), Some("2026-09-05")).unwrap();
        assert!(down_rep.arb.downgrade);

        let zip_path = dir.path().join("ota.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zip = ZipWriter::new(file);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file("META-INF/com/android/metadata", stored).unwrap();
            zip.write_all(b"pre-device=ishtar\nota-type=AB\n").unwrap();
            zip.start_file("payload.bin", stored).unwrap();
            zip.write_all(&raw_payload).unwrap();
            zip.finish().unwrap();
        }

        let zip_rep = inspect_path(&zip_path, None, None).unwrap();
        assert_eq!(zip_rep.pre_device.as_deref(), Some("ishtar"));
        assert_eq!(zip_rep.ota_type.as_deref(), Some("AB"));

        let boot_out = dir.path().join("boot.img");
        let n_boot = extract_partition(&zip_path, "boot", &boot_out, |_, _| {}).unwrap();
        assert_eq!(n_boot, 8192);
        let got_boot = fs::read(&boot_out).unwrap();
        assert_eq!(&got_boot[..4096], boot_block.as_slice());
        assert!(got_boot[4096..].iter().all(|&b| b == 0));

        let vb_out = dir.path().join("vbmeta.img");
        let n_vb = extract_partition(&zip_path, "vbmeta", &vb_out, |_, _| {}).unwrap();
        assert_eq!(n_vb, 2048);
        assert_eq!(fs::read(&vb_out).unwrap(), vbmeta_raw);
    }
}
