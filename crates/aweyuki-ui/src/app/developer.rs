//! Developer & kernel inspection and sequential storage benchmark page.

use super::*;
use aweyuki_install::dev::{BenchResult, KernelInfo};

/// Benchmark target storage volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchTarget {
    Storage,
    UsbDrive,
}

/// Active or finished storage benchmark job.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchJob {
    pub target: BenchTarget,
    pub done: u64,
    pub total: u64,
    pub outcome: Option<Result<BenchResult, String>>,
}

/// State backing [`Screen::Developer`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DevState {
    pub info: Option<KernelInfo>,
    pub bench: Option<BenchJob>,
}

impl App {
    /// Update kernel/sysfs snapshot shown on [`Screen::Developer`].
    pub fn set_dev_info(&mut self, info: KernelInfo) {
        self.state.dev.info = Some(info);
        if self.screen() == Screen::Developer {
            self.refresh();
        }
    }

    /// Update in-flight progress for the active storage benchmark.
    pub fn dev_bench_progress(&mut self, done: u64, total: u64) {
        let Some(job) = self.state.dev.bench.as_mut() else {
            return;
        };
        job.done = done;
        job.total = total;
        if self.screen() == Screen::Developer {
            self.refresh();
        }
    }

    /// Record completion or failure of the storage benchmark.
    pub fn dev_bench_ended(&mut self, outcome: Result<BenchResult, String>, now: u64) {
        if let Some(job) = self.state.dev.bench.as_mut() {
            job.outcome = Some(outcome);
        }
        self.wake(now);
        if self.screen() == Screen::Developer {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn developer_page_reads_kernel_info_toggles_and_runs_benchmark() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Advanced, W, H, PPD);
        assert_eq!(tap(&mut app, &f, &i, Action::Open(Screen::Developer)), vec![Effect::ReadDev]);
        assert_eq!(app.screen(), Screen::Developer);

        app.set_dev_info(KernelInfo {
            selinux: "Permissive".into(),
            printk_level: Some(4),
            adb_config: "adb".into(),
            cpu_governor: Some("schedutil".into()),
            cmdline: "console=ttyMSM0".into(),
        });

        assert_eq!(tap(&mut app, &f, &i, Action::SetPrintk(7)), vec![Effect::SetPrintk(7)]);
        assert_eq!(tap(&mut app, &f, &i, Action::ToggleAdb), vec![Effect::SetAdb(false)]);

        assert!(tap(&mut app, &f, &i, Action::RunBench(BenchTarget::Storage)).is_empty(), "needs decrypt");
        app.data_waiting(None);
        assert_eq!(tap(&mut app, &f, &i, Action::RunBench(BenchTarget::Storage)), vec![Effect::RunBench(BenchTarget::Storage)]);
        assert!(!app.frame(&f, &i).back);

        app.dev_bench_progress(50, 100);
        app.dev_bench_ended(
            Ok(BenchResult {
                target_label: "Internal storage".into(),
                bytes: 64 * 1024 * 1024,
                write_mb_s: 320.5,
                read_mb_s: 980.2,
            }),
            1000,
        );
        assert!(app.frame(&f, &i).back);
    }
}
