//! Persistent and session-local history of installs, flashes, and wipes.

use anyhow::{Context, Result};
use std::cmp::Reverse;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

pub const RUN_HISTORY: &str = "/aweyuki/run/flash-history.log";
pub const STORAGE_HISTORY: &str = "/sdcard/Aweyuki/logs/flash-history.log";
pub const MAX_ENTRIES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub timestamp: i64,
    pub action: String,
    pub target: String,
    pub outcome: String,
}

fn clean_field(s: &str) -> String {
    s.chars()
        .map(|c| if matches!(c, '\t' | '\r' | '\n') { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

fn parse_line(line: &str) -> Option<Entry> {
    let mut parts = line.splitn(4, '\t');
    let timestamp = parts.next()?.trim().parse::<i64>().ok()?;
    let action = parts.next()?.trim().to_string();
    let target = parts.next()?.trim().to_string();
    let outcome = parts.next()?.trim().to_string();
    if action.is_empty() && target.is_empty() {
        return None;
    }
    Some(Entry { timestamp, action, target, outcome })
}

fn read_all(path: &Path) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_line).collect()
}

/// Append `entry` to the tab-separated history log at `path`, keeping at most
/// the newest [`MAX_ENTRIES`] records.
///
/// # Errors
///
/// Returns an error if creating parent directories or writing the log fails.
pub fn record(path: &Path, entry: &Entry) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut entries = read_all(path);
    entries.push(Entry {
        timestamp: entry.timestamp,
        action: clean_field(&entry.action),
        target: clean_field(&entry.target),
        outcome: clean_field(&entry.outcome),
    });
    if entries.len() > MAX_ENTRIES {
        let drop_count = entries.len() - MAX_ENTRIES;
        entries.drain(..drop_count);
    }
    let mut text = String::new();
    for item in &entries {
        use std::fmt::Write as _;
        let _ = writeln!(text, "{}\t{}\t{}\t{}", item.timestamp, item.action, item.target, item.outcome);
    }
    let mut file = File::create(path).with_context(|| format!("write {}", path.display()))?;
    file.write_all(text.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    file.sync_all().with_context(|| format!("sync {}", path.display()))
}

/// Read up to `limit` entries from `path`, newest first.
#[must_use]
pub fn read_recent(path: &Path, limit: usize) -> Vec<Entry> {
    let mut entries = read_all(path);
    entries.reverse();
    entries.truncate(limit);
    entries
}

/// Merge and deduplicate entries from `paths`, ordered newest first, capped at `limit`.
#[must_use]
pub fn merge_recent(paths: &[&Path], limit: usize) -> Vec<Entry> {
    if limit == 0 {
        return Vec::new();
    }
    let mut combined = Vec::new();
    for path in paths {
        let mut entries = read_all(path);
        entries.reverse();
        combined.extend(entries);
    }
    combined.sort_by_key(|e| Reverse(e.timestamp));

    let mut unique = Vec::with_capacity(combined.len().min(limit));
    for entry in combined {
        if !unique.contains(&entry) {
            unique.push(entry);
            if unique.len() == limit {
                break;
            }
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_caps_reads_newest_first_and_merges_without_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("run/flash-history.log");
        let b = tmp.path().join("storage/flash-history.log");

        for i in 0..=(MAX_ENTRIES as i64 + 5) {
            record(
                &a,
                &Entry {
                    timestamp: 1000 + i,
                    action: "install".into(),
                    target: format!("pkg-{i}.zip"),
                    outcome: "ok".into(),
                },
            )
            .unwrap();
        }
        let recent = read_recent(&a, 500);
        assert_eq!(recent.len(), MAX_ENTRIES);
        assert_eq!(recent[0].timestamp, 1000 + MAX_ENTRIES as i64 + 5);

        record(
            &b,
            &Entry {
                timestamp: 1000 + MAX_ENTRIES as i64 + 5,
                action: "install".into(),
                target: format!("pkg-{}.zip", MAX_ENTRIES + 5),
                outcome: "ok".into(),
            },
        )
        .unwrap();
        record(
            &b,
            &Entry {
                timestamp: 9999,
                action: "wipe\tbad".into(),
                target: "frp\npart".into(),
                outcome: "erased".into(),
            },
        )
        .unwrap();

        let merged = merge_recent(&[&b, &a], 5);
        assert_eq!(merged.len(), 5);
        assert_eq!(merged[0], Entry {
            timestamp: 9999,
            action: "wipe bad".into(),
            target: "frp part".into(),
            outcome: "erased".into(),
        });
        assert_eq!(merged[1].target, format!("pkg-{}.zip", MAX_ENTRIES + 5));
        assert_eq!(merged[2].target, format!("pkg-{}.zip", MAX_ENTRIES + 4));
    }
}
