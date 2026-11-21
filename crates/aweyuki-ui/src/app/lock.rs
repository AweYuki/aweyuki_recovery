//! Recovery PIN lock gate and PIN setup state machine.

use super::*;
use crate::settings::RecoveryLock;
use aweyuki_install::lock::{generate_salt, make_digest, verify_pin, DEFAULT_ITERATIONS, MAX_PIN_LEN, MIN_PIN_LEN};

/// Which step of recovery PIN setup or removal the user is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LockSetupStage {
    #[default]
    Menu,
    VerifyCurrent,
    EnterNew,
    ConfirmNew,
}

/// UI state backing [`Screen::LockSetup`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LockSetupState {
    pub stage: LockSetupStage,
    pub first_pin: String,
    pub removing: bool,
}

impl App {
    /// Engage the full-screen PIN lock before entering `after` when a
    /// recovery PIN is configured.
    pub fn engage_boot_lock(&mut self, after: Screen) {
        if self.state.recovery_lock.is_none() {
            return;
        }
        self.state.locked = true;
        self.state.lock_after = after;
        self.state.lock_input.clear();
        self.stack = vec![(Screen::Lock, 0.0, Focus::Unset)];
        self.settle_page();
        self.refresh();
    }

    pub(super) fn lock_now(&mut self) {
        if self.state.recovery_lock.is_none() {
            return;
        }
        self.state.lock_setup = LockSetupState::default();
        self.state.lock_input.clear();
        self.state.locked = true;
        self.state.lock_after = Screen::Home;
        self.stack = vec![(Screen::Lock, 0.0, Focus::Unset)];
        self.settle_page();
        self.refresh();
    }

    pub(super) fn submit_lock_pin(&mut self, now: u64) -> Vec<Effect> {
        let l = self.state.lang;
        let pin = std::mem::take(&mut self.state.lock_input);
        let ok = self
            .state
            .recovery_lock
            .as_ref()
            .is_some_and(|lock| verify_pin(&pin, &lock.salt_hex, &lock.hash_hex, lock.iterations));
        if ok {
            self.state.locked = false;
            let after = self.state.lock_after;
            self.stack = vec![(after, 0.0, Focus::Unset)];
            self.settle_page();
            self.refresh();
            return self.arrive();
        }
        self.say(l.pick(["PIN 码错误", "PIN 碼錯誤", "Incorrect PIN"]), now);
        self.refresh();
        Vec::new()
    }

    pub(super) fn submit_lock_setup(&mut self, now: u64) {
        let l = self.state.lang;
        let pin = std::mem::take(&mut self.state.lock_input);
        match self.state.lock_setup.stage {
            LockSetupStage::Menu => {}
            LockSetupStage::VerifyCurrent => {
                let ok = self
                    .state
                    .recovery_lock
                    .as_ref()
                    .is_some_and(|lock| verify_pin(&pin, &lock.salt_hex, &lock.hash_hex, lock.iterations));
                if !ok {
                    self.say(l.pick(["当前 PIN 码错误", "目前 PIN 碼錯誤", "Incorrect current PIN"]), now);
                } else if self.state.lock_setup.removing {
                    self.state.recovery_lock = None;
                    self.state.locked = false;
                    self.state.lock_setup = LockSetupState::default();
                    self.say(l.pick(["PIN 锁已移除", "PIN 鎖已移除", "PIN lock removed"]), now);
                } else {
                    self.state.lock_setup.stage = LockSetupStage::EnterNew;
                }
            }
            LockSetupStage::EnterNew => {
                let count = pin.chars().count();
                if !(MIN_PIN_LEN..=MAX_PIN_LEN).contains(&count) {
                    self.say(
                        &tr!(
                            l,
                            "PIN 码需为 {MIN_PIN_LEN} 到 {MAX_PIN_LEN} 位",
                            "PIN 碼需為 {MIN_PIN_LEN} 到 {MAX_PIN_LEN} 位",
                            "PIN must be {MIN_PIN_LEN} to {MAX_PIN_LEN} digits",
                            "Mã PIN phải từ {MIN_PIN_LEN} đến {MAX_PIN_LEN} số",
                        ),
                        now,
                    );
                } else {
                    self.state.lock_setup.first_pin = pin;
                    self.state.lock_setup.stage = LockSetupStage::ConfirmNew;
                }
            }
            LockSetupStage::ConfirmNew => {
                if pin != self.state.lock_setup.first_pin {
                    self.state.lock_setup.first_pin.clear();
                    self.state.lock_setup.stage = LockSetupStage::EnterNew;
                    self.say(l.pick(["两次输入的 PIN 码不一致", "兩次輸入的 PIN 碼不一致", "PINs didn't match"]), now);
                } else {
                    match generate_salt().and_then(|salt| make_digest(&pin, &salt, DEFAULT_ITERATIONS)) {
                        Ok(digest) => {
                            self.state.recovery_lock = Some(RecoveryLock {
                                salt_hex: digest.salt_hex,
                                hash_hex: digest.hash_hex,
                                iterations: digest.iterations,
                            });
                            self.state.lock_setup = LockSetupState::default();
                            self.say(l.pick(["PIN 锁已保存", "PIN 鎖已儲存", "PIN lock saved"]), now);
                        }
                        Err(e) => self.report(&format!("{e:#}"), now),
                    }
                }
            }
        }
        self.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    fn enter_pin(app: &mut App, f: &Fonts, i: &DeviceInfo, digits: &str) -> Vec<Effect> {
        for ch in digits.chars() {
            tap(app, f, i, Action::Type(Cap::Char(ch)));
        }
        tap(app, f, i, Action::Type(Cap::Enter))
    }

    #[test]
    fn sets_locks_blocks_back_and_removes_recovery_pin() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Settings, &f, &i);
        tap(&mut app, &f, &i, Action::Open(Screen::LockSetup));
        assert_eq!(app.screen(), Screen::LockSetup);

        tap(&mut app, &f, &i, Action::LockStartSet);
        assert_eq!(app.state.lock_setup.stage, LockSetupStage::EnterNew);
        enter_pin(&mut app, &f, &i, "12");
        assert_eq!(app.state.lock_setup.stage, LockSetupStage::EnterNew, "short PIN rejected");

        enter_pin(&mut app, &f, &i, "2580");
        assert_eq!(app.state.lock_setup.stage, LockSetupStage::ConfirmNew);
        enter_pin(&mut app, &f, &i, "0000");
        assert_eq!(app.state.lock_setup.stage, LockSetupStage::EnterNew, "mismatch restarts");

        enter_pin(&mut app, &f, &i, "2580");
        enter_pin(&mut app, &f, &i, "2580");
        assert!(app.state.recovery_lock.is_some());
        assert_eq!(app.state.lock_setup.stage, LockSetupStage::Menu);

        tap(&mut app, &f, &i, Action::LockNow);
        assert_eq!(app.screen(), Screen::Lock);
        assert!(app.state.locked);
        assert!(!app.frame(&f, &i).back);
        app.key(Key::Back, 0);
        assert_eq!(app.screen(), Screen::Lock, "Back cannot bypass recovery lock");

        enter_pin(&mut app, &f, &i, "9999");
        assert_eq!(app.screen(), Screen::Lock);
        assert!(app.state.locked);

        enter_pin(&mut app, &f, &i, "2580");
        assert!(!app.state.locked);
        assert_eq!(app.screen(), Screen::Home);

        app.open(Screen::LockSetup);
        tap(&mut app, &f, &i, Action::LockStartRemove);
        enter_pin(&mut app, &f, &i, "2580");
        assert!(app.state.recovery_lock.is_none());
    }
}
