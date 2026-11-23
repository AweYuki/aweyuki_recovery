//! App Backup Vault page: per-app `.tar.lz4` backup and restore state.

use super::*;

/// Installed user application summarized for [`Screen::AppVault`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultAppRow {
    pub package: String,
    pub apk_bytes: u64,
    pub data_bytes: u64,
}

/// Saved per-app `.tar.lz4` archive summarized for [`Screen::AppVault`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultBackupRow {
    pub package: String,
    pub made: i64,
    pub archive_file: String,
    pub archive_bytes: u64,
    pub has_data: bool,
}

/// Which tab [`Screen::AppVault`] currently displays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VaultTab {
    #[default]
    Apps,
    Saved,
}

/// Active or completed per-app backup or restore operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultJob {
    pub label: String,
    pub restore: bool,
    pub done: u64,
    pub total: u64,
    pub outcome: Option<Result<String, String>>,
}

/// State backing [`Screen::AppVault`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VaultState {
    pub tab: VaultTab,
    pub apps: Option<Result<Vec<VaultAppRow>, String>>,
    pub saved: Option<Result<Vec<VaultBackupRow>, String>>,
    pub job: Option<VaultJob>,
}

impl App {
    /// Update discovered installed apps and saved vault archives.
    pub fn set_vault(&mut self, apps: Result<Vec<VaultAppRow>, String>, saved: Result<Vec<VaultBackupRow>, String>) {
        self.state.vault.apps = Some(apps);
        self.state.vault.saved = Some(saved);
        if self.screen() == Screen::AppVault {
            self.refresh();
        }
    }

    /// Update byte progress for the running vault job.
    pub fn vault_progress(&mut self, done: u64, total: u64) {
        let Some(job) = self.state.vault.job.as_mut() else {
            return;
        };
        job.done = done;
        job.total = total;
        if self.screen() == Screen::AppVault {
            self.refresh();
        }
    }

    /// Record completion or failure of a vault backup/restore job.
    pub fn vault_ended(&mut self, outcome: Result<String, String>, now: u64) {
        if let Some(job) = self.state.vault.job.as_mut() {
            job.outcome = Some(outcome);
        }
        self.wake(now);
        if self.screen() == Screen::AppVault {
            self.refresh();
        }
    }

    /// Handle deletion of a saved vault archive and refresh the list on success.
    pub fn vault_deleted(&mut self, outcome: Result<(), String>, now: u64) -> Vec<Effect> {
        let l = self.state.lang;
        match outcome {
            Ok(()) => {
                self.say(l.pick(crate::texts::fork::s::M029), now);
                vec![Effect::ReadVault]
            }
            Err(why) => {
                self.report(&why, now);
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn app_vault_discovers_backs_up_restores_and_deletes_archives() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Advanced, W, H, PPD);
        assert!(tap(&mut app, &f, &i, Action::Open(Screen::AppVault)).is_empty(), "waits for decrypt");
        app.data_waiting(None);
        assert_eq!(tap(&mut app, &f, &i, Action::RefreshVault), vec![Effect::ReadVault]);

        let populate = |app: &mut App| {
            app.set_vault(
                Ok(vec![VaultAppRow {
                    package: "com.example.demo".into(),
                    apk_bytes: 2048,
                    data_bytes: 4096,
                }]),
                Ok(vec![VaultBackupRow {
                    package: "com.example.demo".into(),
                    made: 1_792_600_000,
                    archive_file: "com.example.demo-1792600000.tar.lz4".into(),
                    archive_bytes: 1500,
                    has_data: true,
                }]),
            );
        };
        populate(&mut app);

        assert_eq!(
            tap(&mut app, &f, &i, Action::VaultBackupApp(0)),
            vec![Effect::VaultBackup("com.example.demo".into())]
        );
        app.vault_progress(100, 200);
        assert!(!app.frame(&f, &i).back);
        app.vault_ended(Ok("com.example.demo-1792600000.tar.lz4".into()), 100);
        assert!(app.frame(&f, &i).back);
        assert_eq!(tap_at(&mut app, &f, &i, Action::RefreshVault, 2000), vec![Effect::ReadVault]);
        populate(&mut app);

        tap_at(&mut app, &f, &i, Action::VaultTab(VaultTab::Saved), 2100);
        assert_eq!(
            tap_at(&mut app, &f, &i, Action::VaultRestoreSaved(0), 2200),
            vec![Effect::VaultRestore("com.example.demo-1792600000.tar.lz4".into())]
        );
        app.vault_ended(Ok("com.example.demo".into()), 2300);
        assert_eq!(tap_at(&mut app, &f, &i, Action::RefreshVault, 4000), vec![Effect::ReadVault]);
        populate(&mut app);

        assert_eq!(
            tap_at(&mut app, &f, &i, Action::VaultDeleteSaved(0), 4100),
            vec![Effect::VaultDelete("com.example.demo-1792600000.tar.lz4".into())]
        );
        assert_eq!(app.vault_deleted(Ok(()), 4200), vec![Effect::ReadVault]);
    }
}
