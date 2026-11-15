//! Aweyuki's own update page: asking GitHub for its newest release and
//! updating to it.

use super::*;

/// How far asking GitHub for Aweyuki's newest release has come.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AweyukiCheck {
    #[default]
    Idle,
    Asking,
    Failed(Unanswered),
    Answered(Newest),
}

/// Updating Aweyuki, as its page shows it: downloading the newest release's
/// build for the phone, then writing it into the recovery partitions, how
/// far that has come, and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AweyukiJob {
    /// None until the download has begun.
    pub stage: Option<AweyukiStage>,
    /// Bytes a second, while it downloads.
    pub rate: u64,
    pub outcome: Option<AweyukiEnd>,
    /// Stopping the download was asked for and is under way.
    pub stopping: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AweyukiStage {
    Download(Progress),
    /// The build is written into the recovery partitions and read back:
    /// bytes so far, of all.
    Write { done: u64, total: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AweyukiEnd {
    /// The build is in the recovery partitions of these slots.
    Updated(Vec<Slot>),
    /// The download was stopped; what it downloaded is kept to go on from.
    Stopped,
    /// The download did not end with the build whole.
    Download(Failure),
    /// The build could not go in, or where it goes could not be told: why.
    /// Nothing was written.
    NotWritten(String),
    /// Writing it into the recovery partitions of these slots failed, after
    /// some of it was written: why.
    WriteFailed(Vec<Slot>, String),
}

impl App {
    /// What GitHub answered of Aweyuki's newest release.
    pub fn aweyuki_checked(&mut self, answer: Result<Newest, Unanswered>) {
        self.state.aweyuki_check = match answer {
            Ok(newest) => AweyukiCheck::Answered(newest),
            Err(why) => AweyukiCheck::Failed(why),
        };
        if matches!(self.screen(), Screen::Settings | Screen::AweyukiUpdate) {
            self.refresh();
        }
    }

    /// How far updating Aweyuki has come, and how fast the download goes,
    /// in bytes a second.
    pub fn aweyuki_progress(&mut self, stage: AweyukiStage, rate: u64) {
        let job = self.state.aweyuki_job.as_mut().expect("only an update under way moves on");
        job.stage = Some(stage);
        job.rate = rate;
        if self.screen() == Screen::AweyukiUpdate {
            self.refresh();
        }
    }

    /// Updating Aweyuki has ended: its page says how, lit for the user to
    /// read.
    pub fn aweyuki_ended(&mut self, end: AweyukiEnd, now: u64) {
        let job = self.state.aweyuki_job.as_mut().expect("only an update under way ends");
        job.stopping = false;
        job.outcome = Some(end);
        self.wake(now);
        self.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    fn aweyuki_newest(newer: bool) -> Newest {
        Newest {
            version: "0.1.4".into(),
            newer,
            build: Some(Build {
                name: "aweyuki-v0.1.4-ishtar-OS3.0.308.0.WMACNXM.zip".into(),
                url: "https://api.github.com/repos/AweYuki/aweyuki_recovery/releases/assets/719751598".into(),
                link: "https://github.com/AweYuki/aweyuki_recovery/releases/download/v0.1.4/aweyuki-v0.1.4-ishtar-OS3.0.308.0.WMACNXM.zip".into(),
                size: 42_739_608,
                sha256: "62b66f7d13a0eb6c9af3359b5ec6f9c223fd9a86b1f8abab3e9d2646cb2ce534".into(),
            }),
        }
    }

    #[test]
    fn aweyuki_updates_from_its_newest_release_and_its_page_stays_until_the_build_is_in() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Settings, W, H, PPD);
        app.refresh();
        let offered = |app: &mut App| app.frame(&f, &i).hits.iter().any(|h| h.action == Action::Open(Screen::AweyukiUpdate));
        assert!(!offered(&mut app), "a phone whose WLAN the recovery does not bring up");
        app.state.wlan = Some(Wlan::default());
        app.refresh();
        assert!(!offered(&mut app), "a phone that keeps no recovery partition in each slot");
        app.state.recovery_partitions = true;
        app.refresh();
        assert!(tap(&mut app, &f, &i, Action::Open(Screen::AweyukiUpdate)).is_empty(), "nothing is asked without a network");
        assert!(tap(&mut app, &f, &i, Action::CheckAweyuki).is_empty());
        assert_eq!(app.screen(), Screen::Wlan, "asking takes a network, which the WLAN page joins");
        app.key(Key::Back, 0);
        app.set_wlan(connected("Aweyuki"));
        assert_eq!(tap(&mut app, &f, &i, Action::CheckAweyuki), vec![Effect::CheckAweyuki]);
        assert_eq!(app.state.aweyuki_check, AweyukiCheck::Asking);
        app.aweyuki_checked(Ok(aweyuki_newest(true)));
        let build = aweyuki_newest(true).build.unwrap();
        app.state.install = Install::Waiting;
        assert!(tap(&mut app, &f, &i, Action::UpdateAweyuki).is_empty(), "an install under way goes first");
        app.state.install = Install::Idle;
        assert_eq!(tap(&mut app, &f, &i, Action::UpdateAweyuki), vec![Effect::UpdateAweyuki(build.clone())]);
        app.aweyuki_progress(AweyukiStage::Download(Progress::Downloading { done: 10_000_000, total: 42_739_608 }), 2_000_000);
        app.key(Key::Back, 0);
        assert_eq!(app.screen(), Screen::AweyukiUpdate, "the page stays while the build downloads");
        assert_eq!(tap(&mut app, &f, &i, Action::StopAweyukiUpdate), vec![Effect::StopAweyukiUpdate]);
        assert!(!app.frame(&f, &i).hits.iter().any(|h| h.action == Action::StopAweyukiUpdate), "stopping, it is not asked again");
        // An end lights the panel, and a touch just after only keeps it lit.
        app.aweyuki_ended(AweyukiEnd::Stopped, 0);
        assert_eq!(tap_at(&mut app, &f, &i, Action::UpdateAweyuki, 2000), vec![Effect::UpdateAweyuki(build.clone())], "a download stopped carries on");
        app.aweyuki_progress(AweyukiStage::Write { done: 1 << 20, total: 4 << 20 }, 0);
        assert!(!app.frame(&f, &i).hits.iter().any(|h| h.action == Action::StopAweyukiUpdate), "writing goes on to its end");
        app.key(Key::Back, 2100);
        assert_eq!(app.screen(), Screen::AweyukiUpdate, "nor is the page left while the build is written");
        app.aweyuki_ended(AweyukiEnd::Updated(vec![Slot::A, Slot::B]), 3000);
        assert_eq!(tap_at(&mut app, &f, &i, Action::RebootTo(Target::Recovery), 5000), vec![Effect::Reboot(Target::Recovery)]);
        app.key(Key::Back, 5100);
        assert_eq!(app.screen(), Screen::Settings);
    }

    #[test]
    fn the_aweyuki_page_asks_github_as_it_opens_once_a_network_is_joined() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Settings, W, H, PPD);
        app.state.wlan = Some(connected("Aweyuki"));
        app.state.recovery_partitions = true;
        app.refresh();
        assert_eq!(tap(&mut app, &f, &i, Action::Open(Screen::AweyukiUpdate)), vec![Effect::CheckAweyuki]);
        app.aweyuki_checked(Err(Unanswered::Failed("GitHub answered 403 Forbidden: API rate limit exceeded".into())));
        app.key(Key::Back, 0);
        assert_eq!(tap(&mut app, &f, &i, Action::Open(Screen::AweyukiUpdate)), vec![Effect::CheckAweyuki], "a failed check is tried again");
        app.aweyuki_checked(Ok(aweyuki_newest(false)));
        assert!(!app.frame(&f, &i).hits.iter().any(|h| h.action == Action::UpdateAweyuki), "nothing newer to update to");
        app.key(Key::Back, 0);
        assert!(tap(&mut app, &f, &i, Action::Open(Screen::AweyukiUpdate)).is_empty(), "an answer heard is not asked again");
    }
}
