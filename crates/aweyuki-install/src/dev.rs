//! Developer & kernel inspection helpers and sequential storage benchmark.

use anyhow::{ensure, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const BENCH_CHUNK: usize = 1024 * 1024;
const CMDLINE_LIMIT: usize = 512;

/// Snapshot of kernel/sysfs runtime flags shown on Developer tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelInfo {
    pub selinux: String,
    pub printk_level: Option<u8>,
    pub adb_config: String,
    pub cpu_governor: Option<String>,
    pub cmdline: String,
}

/// Sequential storage write/read benchmark throughput measurement.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchResult {
    pub target_label: String,
    pub bytes: u64,
    pub write_mb_s: f64,
    pub read_mb_s: f64,
}

/// Parse the console loglevel (first integer column) from `/proc/sys/kernel/printk`.
#[must_use]
pub fn parse_printk_level(raw: &str) -> Option<u8> {
    raw.split_whitespace().next()?.parse::<u8>().ok()
}

fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Read current SELinux status, console `printk` level, USB/ADB state,
/// CPU0 scaling governor, and kernel command line.
#[must_use]
pub fn read_kernel_info() -> KernelInfo {
    let selinux = match fs::read_to_string("/sys/fs/selinux/enforce") {
        Ok(s) => match s.trim() {
            "1" => "Enforcing".to_string(),
            "0" => "Permissive".to_string(),
            other => format!("Enforce={other}"),
        },
        Err(_) => "Disabled".to_string(),
    };
    let printk_level = read_trimmed("/proc/sys/kernel/printk").and_then(|s| parse_printk_level(&s));
    let adb_config = aweyuki_sys::prop("sys.usb.state")
        .or_else(|| aweyuki_sys::prop("sys.usb.config"))
        .unwrap_or_else(|| "unknown".to_string());
    let cpu_governor = read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
    let raw_cmd = read_trimmed("/proc/cmdline").unwrap_or_default();
    let cmdline = if raw_cmd.len() > CMDLINE_LIMIT {
        let mut end = CMDLINE_LIMIT;
        while !raw_cmd.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &raw_cmd[..end])
    } else {
        raw_cmd
    };
    KernelInfo {
        selinux,
        printk_level,
        adb_config,
        cpu_governor,
        cmdline,
    }
}

/// Set kernel console `printk` verbosity level (`0..=8`).
///
/// # Errors
///
/// Returns an error if `level > 8` or `/proc/sys/kernel/printk` cannot be written.
pub fn set_printk_level(level: u8) -> Result<()> {
    ensure!(level <= 8, "printk level {level} out of range (0..=8)");
    fs::write("/proc/sys/kernel/printk", format!("{level}\n")).context("write /proc/sys/kernel/printk")
}

/// Toggle Android USB gadget (`adb` / `mtp,adb` vs `none`) without clobbering MTP.
///
/// # Errors
///
/// Returns an error if `sys.usb.config` transition fails or times out.
#[cfg(target_os = "android")]
pub fn set_adb_enabled(on: bool) -> Result<()> {
    let target = if on { aweyuki_sys::usb::resting() } else { "none" };
    aweyuki_sys::usb::set(target, std::time::Duration::from_secs(5))
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Run a sequential write + `sync_all` + `drop_cache` + sequential read benchmark
/// inside `dir` over `size_bytes`, removing the temporary file even on error.
///
/// # Errors
///
/// Returns an error if `size_bytes < 1 MiB` or creating, writing, syncing,
/// or reading the benchmark file fails.
pub fn run_storage_benchmark(
    dir: &Path,
    target_label: &str,
    size_bytes: u64,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<BenchResult> {
    ensure!(size_bytes >= BENCH_CHUNK as u64, "benchmark size must be at least 1 MiB");
    ensure!(dir.is_dir(), "{} is not an accessible directory", dir.display());

    let probe_path = dir.join(format!(".aweyuki-bench-{}.tmp", std::process::id()));
    let _cleanup = RemoveOnDrop(probe_path.clone());

    let mut pattern = vec![0u8; BENCH_CHUNK];
    for (i, b) in pattern.iter_mut().enumerate() {
        *b = ((i as u8).wrapping_mul(31)) ^ 0xA5;
    }

    let total_steps = size_bytes.saturating_mul(2);
    let mut done = 0u64;
    on_progress(0, total_steps);

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&probe_path)
        .with_context(|| format!("create {}", probe_path.display()))?;

    let w_start = Instant::now();
    let mut rem = size_bytes;
    while rem > 0 {
        let n = usize::try_from(rem.min(BENCH_CHUNK as u64)).expect("bounded by BENCH_CHUNK");
        file.write_all(&pattern[..n]).context("write benchmark probe")?;
        rem -= n as u64;
        done += n as u64;
        on_progress(done, total_steps);
    }
    file.sync_all().context("sync benchmark probe")?;
    let write_secs = w_start.elapsed().as_secs_f64().max(1e-6);

    let _ = aweyuki_block::drop_cache(&file);
    file.seek(SeekFrom::Start(0)).context("rewind benchmark probe")?;

    let r_start = Instant::now();
    let mut rem = size_bytes;
    while rem > 0 {
        let n = usize::try_from(rem.min(BENCH_CHUNK as u64)).expect("bounded by BENCH_CHUNK");
        file.read_exact(&mut pattern[..n]).context("read benchmark probe")?;
        rem -= n as u64;
        done += n as u64;
        on_progress(done, total_steps);
    }
    let read_secs = r_start.elapsed().as_secs_f64().max(1e-6);
    drop(file);

    let mb = size_bytes as f64 / (1024.0 * 1024.0);
    Ok(BenchResult {
        target_label: target_label.to_string(),
        bytes: size_bytes,
        write_mb_s: mb / write_secs,
        read_mb_s: mb / read_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_printk_console_level() {
        assert_eq!(parse_printk_level("4\t4\t1\t7\n"), Some(4));
        assert_eq!(parse_printk_level("7 4 1 7"), Some(7));
        assert_eq!(parse_printk_level(""), None);
    }

    #[test]
    fn runs_storage_benchmark_and_cleans_up_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut last = (0, 0);
        let res = run_storage_benchmark(dir.path(), "Test", 2 * 1024 * 1024, |d, t| {
            last = (d, t);
        })
        .unwrap();
        assert_eq!(res.bytes, 2 * 1024 * 1024);
        assert!(res.write_mb_s > 0.0 && res.read_mb_s > 0.0);
        assert_eq!(last, (4 * 1024 * 1024, 4 * 1024 * 1024));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
