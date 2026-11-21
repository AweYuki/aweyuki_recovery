//! Batch image flasher page: scan folders for `.img` / `.efi`, validate
//! partition targets and sizes, choose slots, and flash sequentially.

use super::*;
use aweyuki_install::batch::{BatchItem, GeometryCheck};

/// Where batch images can be scanned from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BatchSource {
    #[default]
    Downloads,
    Storage,
    UsbDrive,
}

impl BatchSource {
    pub const ALL: [BatchSource; 3] = [BatchSource::Downloads, BatchSource::Storage, BatchSource::UsbDrive];
}

/// Progress or completion snapshot of a batch flash job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchJob {
    pub total: usize,
    pub index: usize,
    pub partition: String,
    pub done: u64,
    pub of: u64,
    pub outcome: Option<Result<Vec<String>, String>>,
}

/// State backing [`Screen::BatchFlash`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchState {
    pub source: BatchSource,
    pub items: Option<Result<Vec<(BatchItem, bool)>, String>>,
    pub pick: Pick,
    pub job: Option<BatchJob>,
}

impl Default for BatchState {
    fn default() -> Self {
        BatchState {
            source: BatchSource::Downloads,
            items: None,
            pick: Pick::Current,
            job: None,
        }
    }
}

impl BatchState {
    pub(crate) fn any_slotted_selected(&self) -> bool {
        self.items
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .is_some_and(|list| list.iter().any(|(it, on)| *on && it.slotted))
    }

    pub(crate) fn selected_payload(&self) -> Vec<(std::path::PathBuf, String, bool)> {
        self.items
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|list| {
                list.iter()
                    .filter(|(it, on)| *on && matches!(it.geometry, GeometryCheck::Ok { .. }) && it.partition.is_some())
                    .map(|(it, _)| (it.path.clone(), it.partition.clone().unwrap_or_default(), it.slotted))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl State {
    pub(crate) fn batch_source_open(&self) -> bool {
        match self.batch.source {
            BatchSource::Downloads | BatchSource::Storage => self.data.storage_open(),
            BatchSource::UsbDrive => self.usb_drive == Some(UsbDrive::Shown),
        }
    }
}

impl App {
    /// Set scanned image items for [`Screen::BatchFlash`], preselecting every
    /// item that passes geometry validation.
    pub fn set_batch(&mut self, outcome: Result<Vec<BatchItem>, String>) {
        self.state.batch.items = Some(outcome.map(|items| {
            items
                .into_iter()
                .map(|item| {
                    let ok = matches!(item.geometry, GeometryCheck::Ok { .. });
                    (item, ok)
                })
                .collect()
        }));
        if self.screen() == Screen::BatchFlash {
            self.refresh();
        }
    }

    /// Update in-flight progress for the active batch flash job.
    pub fn batch_progress(&mut self, index: usize, partition: String, done: u64, total: u64) {
        let Some(job) = self.state.batch.job.as_mut() else {
            return;
        };
        job.index = index;
        job.partition = partition;
        job.done = done;
        job.of = total;
        if self.screen() == Screen::BatchFlash {
            self.refresh();
        }
    }

    /// Record completion (list of flashed partitions or error) of a batch flash job.
    pub fn batch_ended(&mut self, outcome: Result<Vec<String>, String>, now: u64) {
        if let Some(job) = self.state.batch.job.as_mut() {
            job.outcome = Some(outcome);
        }
        self.wake(now);
        if self.screen() == Screen::BatchFlash {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;
    use aweyuki_install::batch::{BatchItem, GeometryCheck};
    use std::path::PathBuf;

    #[test]
    fn batch_flash_validates_geometry_toggles_and_runs() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Advanced, W, H, PPD);
        assert!(tap(&mut app, &f, &i, Action::Open(Screen::BatchFlash)).is_empty(), "downloads wait for data decrypt");
        assert_eq!(app.screen(), Screen::BatchFlash);

        app.data_waiting(None);
        assert_eq!(tap(&mut app, &f, &i, Action::BatchPlace(1)), vec![Effect::ReadBatch(BatchSource::Storage)]);
        app.set_batch(Ok(vec![
            BatchItem {
                path: PathBuf::from("/sdcard/boot.img"),
                file_name: "boot.img".into(),
                size: 1024,
                kind: None,
                partition: Some("boot".into()),
                slotted: true,
                geometry: GeometryCheck::Ok { partition_bytes: 2048 },
            },
            BatchItem {
                path: PathBuf::from("/sdcard/bad.img"),
                file_name: "bad.img".into(),
                size: 4096,
                kind: None,
                partition: Some("abl".into()),
                slotted: true,
                geometry: GeometryCheck::TooLarge { image_bytes: 4096, partition_bytes: 512 },
            },
        ]));

        // Oversized item is not selectable
        tap(&mut app, &f, &i, Action::BatchToggle(1));
        assert!(!app.state.batch.items.as_ref().unwrap().as_ref().unwrap()[1].1);

        assert_eq!(
            swipe(&mut app, &f, &i, Confirm::BatchFlash),
            vec![Effect::RunBatch {
                items: vec![(PathBuf::from("/sdcard/boot.img"), "boot".into(), true)],
                pick: Pick::Current,
            }]
        );
        app.batch_progress(0, "boot".into(), 512, 1024);
        assert!(!app.frame(&f, &i).back);
        app.batch_ended(Ok(vec!["boot".into()]), 1000);
        assert!(app.frame(&f, &i).back);
    }
}
