//! OTA `payload.bin` / `.zip` inspector, Anti-Rollback check, and partition extractor page.

use super::*;
use aweyuki_install::payload::PayloadReport;
use std::path::PathBuf;

/// Inspection and extraction state shown on [`Screen::Payload`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadState {
    pub path: PathBuf,
    pub report: Option<Result<PayloadReport, String>>,
    pub extracting: Option<(String, u64, u64)>,
    pub last_extracted: Option<Result<String, String>>,
}

impl App {
    /// Update the parsed payload metadata, ARB status, and partition table.
    pub fn set_payload(&mut self, outcome: Result<PayloadReport, String>) {
        if let Some(p) = self.state.payload.as_mut() {
            p.report = Some(outcome);
            if self.screen() == Screen::Payload {
                self.refresh();
            }
        }
    }

    /// Update extraction progress in bytes for the partition currently being extracted.
    pub fn payload_progress(&mut self, done: u64, total: u64) {
        if let Some(p) = self.state.payload.as_mut()
            && let Some((_, d, t)) = &mut p.extracting
        {
            *d = done;
            *t = total;
            if self.screen() == Screen::Payload {
                self.refresh();
            }
        }
    }

    /// Record completion (output path or error) of a single-partition extraction.
    pub fn payload_extracted(&mut self, outcome: Result<String, String>, now: u64) {
        if let Some(p) = self.state.payload.as_mut() {
            p.extracting = None;
            p.last_extracted = Some(outcome.clone());
        }
        self.wake(now);
        let msg = match outcome {
            Ok(out) => tr!(
                self.state.lang,
                "已提取到 {out}",
                "已提取到 {out}",
                "Extracted to {out}",
                "Đã trích xuất tới {out}",
            ),
            Err(why) => tr!(
                self.state.lang,
                "提取失败：{why}",
                "提取失敗：{why}",
                "Extract failed: {why}",
                "Trích xuất thất bại: {why}",
            ),
        };
        self.say(&msg, now);
        if self.screen() == Screen::Payload {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;
    use aweyuki_install::payload::{ArbStatus, PartitionInfo, PayloadReport};

    #[test]
    fn inspects_payload_from_file_info_and_triggers_partition_extraction() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Files, W, H, PPD);
        app.set_files(
            "/sdcard/Download".into(),
            Ok(vec![FileRow {
                name: "ota.zip".into(),
                kind: FileKind::File,
                size: 1024,
                modified: 0,
                mode: 0o644,
                uid: 0,
                gid: 0,
            }]),
            0,
        );
        app.state.file = Some(app.state.files.entry(0).clone());
        app.open(Screen::FileInfo);

        assert_eq!(
            tap(&mut app, &f, &i, Action::InspectPayload),
            vec![Effect::InspectPayload(PathBuf::from("/sdcard/Download/ota.zip"))]
        );
        assert_eq!(app.screen(), Screen::Payload);

        app.set_payload(Ok(PayloadReport {
            path: PathBuf::from("/sdcard/Download/ota.zip"),
            block_size: 4096,
            minor_version: 0,
            data_offset: 64,
            security_patch: Some("2026-08-05".into()),
            max_timestamp: Some(1_750_000_000),
            pre_device: Some("ishtar".into()),
            ota_type: Some("AB".into()),
            arb: ArbStatus {
                package_timestamp: Some(1_750_000_000),
                device_timestamp: Some(1_800_000_000),
                package_patch: Some("2026-08-05".into()),
                device_patch: Some("2026-09-05".into()),
                downgrade: true,
            },
            partitions: vec![
                PartitionInfo {
                    name: "boot".into(),
                    size: 67_108_864,
                    sha256: None,
                    version: None,
                    ops: Vec::new(),
                    extractable: true,
                    unsupported_codecs: Vec::new(),
                },
                PartitionInfo {
                    name: "system".into(),
                    size: 1_000_000,
                    sha256: None,
                    version: None,
                    ops: Vec::new(),
                    extractable: false,
                    unsupported_codecs: vec!["xz".into()],
                },
            ],
        }));

        assert_eq!(
            tap(&mut app, &f, &i, Action::ExtractPayloadPart(0)),
            vec![Effect::ExtractPayloadPart {
                path: PathBuf::from("/sdcard/Download/ota.zip"),
                partition: "boot".into(),
            }]
        );
        app.payload_progress(32_000_000, 67_108_864);
        assert!(!app.frame(&f, &i).back);
        app.payload_extracted(Ok("/sdcard/Download/boot.img".into()), 1000);
        assert!(app.frame(&f, &i).back);
    }
}
