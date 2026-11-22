//! Isolated AERA Telegram TDLib client state, auth inputs, and chat view.

use super::*;
use aweyuki_install::aerap::telegram_proto::{TgAuthState, TgKind};

const MAX_MESSAGES: usize = 60;

/// Focused text input field on [`Screen::Telegram`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TgField {
    ApiId,
    ApiHash,
    VaultKey,
    Phone,
    Code,
    Password,
    Email,
    EmailCode,
    FirstName,
    LastName,
    Compose,
}

/// Summary row for a Telegram chat conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TgChat {
    pub id: i64,
    pub title: String,
    pub preview: String,
    pub unread: u32,
}

/// Downloadable attachment metadata attached to a [`TgMessage`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TgAttachment {
    pub file_id: i32,
    pub name: String,
    pub size: u64,
    pub is_photo: bool,
}

/// One message shown inside an open Telegram conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TgMessage {
    pub id: i64,
    pub sender: String,
    pub body: String,
    pub outgoing: bool,
    pub attachments: Vec<TgAttachment>,
}

/// UI state for [`Screen::Telegram`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramState {
    pub active: bool,
    pub starting: bool,
    pub unpack_pct: u8,
    pub auth: TgAuthState,
    pub status: Option<String>,
    pub error: Option<String>,
    pub chats: Vec<TgChat>,
    pub open_chat: Option<(i64, String)>,
    pub messages: Vec<TgMessage>,
    pub focus: TgField,
    pub api_id: String,
    pub api_hash: String,
    pub vault_key: String,
    pub phone: String,
    pub code: String,
    pub password: String,
    pub email: String,
    pub email_code: String,
    pub first_name: String,
    pub last_name: String,
    pub compose: String,
    pub download_note: Option<String>,
}

impl Default for TelegramState {
    fn default() -> Self {
        Self {
            active: false,
            starting: false,
            unpack_pct: 0,
            auth: TgAuthState::Starting,
            status: None,
            error: None,
            chats: Vec::new(),
            open_chat: None,
            messages: Vec::new(),
            focus: TgField::Phone,
            api_id: String::new(),
            api_hash: String::new(),
            vault_key: String::new(),
            phone: String::new(),
            code: String::new(),
            password: String::new(),
            email: String::new(),
            email_code: String::new(),
            first_name: String::new(),
            last_name: String::new(),
            compose: String::new(),
            download_note: None,
        }
    }
}

impl TelegramState {
    /// Whether the current [`TgAuthState`] renders form fields with the on-screen keyboard.
    #[must_use]
    pub fn shows_keyboard(&self) -> bool {
        match self.auth {
            TgAuthState::NeedConfiguration
            | TgAuthState::NeedVault
            | TgAuthState::NeedPhone
            | TgAuthState::NeedCode
            | TgAuthState::NeedPassword
            | TgAuthState::NeedEmail
            | TgAuthState::NeedEmailCode
            | TgAuthState::NeedRegistration => true,
            TgAuthState::Ready => self.open_chat.is_some(),
            _ => false,
        }
    }
}

impl App {
    pub(super) fn tg_active_text(&mut self) -> &mut String {
        let tg = &mut self.state.telegram;
        match tg.focus {
            TgField::ApiId => &mut tg.api_id,
            TgField::ApiHash => &mut tg.api_hash,
            TgField::VaultKey => &mut tg.vault_key,
            TgField::Phone => &mut tg.phone,
            TgField::Code => &mut tg.code,
            TgField::Password => &mut tg.password,
            TgField::Email => &mut tg.email,
            TgField::EmailCode => &mut tg.email_code,
            TgField::FirstName => &mut tg.first_name,
            TgField::LastName => &mut tg.last_name,
            TgField::Compose => &mut tg.compose,
        }
    }

    pub(super) fn tg_submit_effect(&mut self, now: u64) -> Vec<Effect> {
        let l = self.state.lang;
        let tg = &mut self.state.telegram;
        match tg.auth {
            TgAuthState::NeedConfiguration => {
                let Ok(api_id) = tg.api_id.trim().parse::<i64>() else {
                    self.say(l.pick(["请输入有效的 API ID", "請輸入有效的 API ID", "Enter a valid API ID"]), now);
                    return Vec::new();
                };
                if api_id <= 0 || tg.api_hash.trim().is_empty() || tg.vault_key.is_empty() {
                    self.say(
                        l.pick([
                            "请填写 API ID、API Hash 和本地解锁密钥",
                            "請填寫 API ID、API Hash 與本機解鎖金鑰",
                            "Enter API ID, API Hash, and local vault key",
                        ]),
                        now,
                    );
                    return Vec::new();
                }
                let api_hash = tg.api_hash.trim().to_string();
                let vault_key = std::mem::take(&mut tg.vault_key);
                self.refresh();
                vec![Effect::TelegramConfigure { api_id, api_hash, vault_key }]
            }
            TgAuthState::NeedVault => {
                if tg.vault_key.is_empty() {
                    self.say(l.pick(["请输入本地解锁密钥", "請輸入本機解鎖金鑰", "Enter local vault key"]), now);
                    return Vec::new();
                }
                let key = std::mem::take(&mut tg.vault_key);
                self.refresh();
                vec![Effect::TelegramConfigure { api_id: 0, api_hash: String::new(), vault_key: key }]
            }
            TgAuthState::NeedPhone => {
                let phone = tg.phone.trim().to_string();
                if phone.is_empty() {
                    self.say(l.pick(["请输入手机号", "請輸入手機號碼", "Enter phone number"]), now);
                    return Vec::new();
                }
                vec![Effect::TelegramAuth { kind: TgKind::Phone, text: phone }]
            }
            TgAuthState::NeedCode => {
                let code = std::mem::take(&mut tg.code);
                if code.trim().is_empty() {
                    self.say(l.pick(["请输入验证码", "請輸入驗證碼", "Enter login code"]), now);
                    return Vec::new();
                }
                self.refresh();
                vec![Effect::TelegramAuth { kind: TgKind::Code, text: code.trim().to_string() }]
            }
            TgAuthState::NeedPassword => {
                let pw = std::mem::take(&mut tg.password);
                if pw.is_empty() {
                    self.say(l.pick(["请输入两步验证密码", "請輸入兩步驟驗證密碼", "Enter 2FA password"]), now);
                    return Vec::new();
                }
                self.refresh();
                vec![Effect::TelegramAuth { kind: TgKind::Password, text: pw }]
            }
            TgAuthState::NeedEmail => {
                let email = tg.email.trim().to_string();
                if email.is_empty() {
                    self.say(l.pick(["请输入邮箱地址", "請輸入電子郵件地址", "Enter email address"]), now);
                    return Vec::new();
                }
                vec![Effect::TelegramAuth { kind: TgKind::Email, text: email }]
            }
            TgAuthState::NeedEmailCode => {
                let code = std::mem::take(&mut tg.email_code);
                if code.trim().is_empty() {
                    self.say(l.pick(["请输入邮箱验证码", "請輸入信箱驗證碼", "Enter email code"]), now);
                    return Vec::new();
                }
                self.refresh();
                vec![Effect::TelegramAuth { kind: TgKind::EmailCode, text: code.trim().to_string() }]
            }
            TgAuthState::NeedRegistration => {
                let first = tg.first_name.trim().to_string();
                let last = tg.last_name.trim().to_string();
                if first.is_empty() {
                    self.say(l.pick(["请输入名字", "請輸入名字", "Enter first name"]), now);
                    return Vec::new();
                }
                vec![Effect::TelegramAuth { kind: TgKind::Register, text: format!("{first}\n{last}") }]
            }
            TgAuthState::Ready => {
                let Some((chat_id, _)) = tg.open_chat.as_ref() else {
                    return Vec::new();
                };
                let text = std::mem::take(&mut tg.compose);
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return Vec::new();
                }
                let fx = vec![Effect::TelegramSendText { chat_id: *chat_id, text: trimmed.to_string() }];
                self.refresh();
                fx
            }
            TgAuthState::Starting | TgAuthState::ConfirmElsewhere | TgAuthState::Closing => Vec::new(),
        }
    }

    /// Update XZ unpacking percentage while starting [`Screen::Telegram`].
    pub fn telegram_unpack_progress(&mut self, pct: u8) {
        self.state.telegram.unpack_pct = pct.min(100);
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Mark the isolated Telegram TDLib worker live.
    pub fn telegram_started(&mut self, now: u64) {
        self.state.telegram.active = true;
        self.state.telegram.starting = false;
        self.state.telegram.unpack_pct = 100;
        self.state.telegram.error = None;
        self.wake(now);
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Update the Telegram auth state machine and switch default field focus.
    pub fn telegram_state(&mut self, auth: TgAuthState, detail: String, now: u64) {
        let tg = &mut self.state.telegram;
        let changed = tg.auth != auth;
        tg.auth = auth;
        tg.starting = false;
        if !detail.is_empty() {
            tg.status = Some(detail);
        }
        tg.focus = match auth {
            TgAuthState::NeedConfiguration => TgField::ApiId,
            TgAuthState::NeedVault => TgField::VaultKey,
            TgAuthState::NeedPhone => TgField::Phone,
            TgAuthState::NeedCode => TgField::Code,
            TgAuthState::NeedPassword => TgField::Password,
            TgAuthState::NeedEmail => TgField::Email,
            TgAuthState::NeedEmailCode => TgField::EmailCode,
            TgAuthState::NeedRegistration => TgField::FirstName,
            TgAuthState::Ready => TgField::Compose,
            TgAuthState::Starting | TgAuthState::ConfirmElsewhere | TgAuthState::Closing => tg.focus,
        };
        if changed {
            self.wake(now);
        }
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Update the transient status line from the Telegram worker.
    pub fn telegram_status(&mut self, status: String) {
        self.state.telegram.status = (!status.is_empty()).then_some(status);
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Record and toast a Telegram worker error message.
    pub fn telegram_error(&mut self, msg: String, now: u64) {
        self.state.telegram.error = Some(msg.clone());
        self.state.telegram.starting = false;
        self.say(&msg, now);
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Insert or update a chat entry in the Telegram chat list.
    pub fn telegram_chat(&mut self, id: i64, unread: u32, text: String) {
        let mut lines = text.splitn(3, '\n');
        let title = lines.next().unwrap_or("Chat").to_string();
        let preview = lines.next().unwrap_or("").to_string();
        let chat = TgChat { id, title, preview, unread };
        if let Some(existing) = self.state.telegram.chats.iter_mut().find(|c| c.id == id) {
            *existing = chat;
        } else {
            self.state.telegram.chats.push(chat);
        }
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Finish refreshing the chat list.
    pub fn telegram_chats_done(&mut self) {
        self.state.telegram.status = None;
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Append or update a message in the active chat.
    pub fn telegram_message(&mut self, chat_id: i64, msg_id: i64, flags: u32, text: String) {
        if self.state.telegram.open_chat.as_ref().is_none_or(|(id, _)| *id != chat_id) {
            return;
        }
        let mut parts = text.splitn(3, '\n');
        let sender = parts.next().unwrap_or("").to_string();
        let _avatar = parts.next();
        let body = parts.next().unwrap_or("").to_string();
        let outgoing = (flags & 1) != 0;
        let msgs = &mut self.state.telegram.messages;
        if let Some(existing) = msgs.iter_mut().find(|m| m.id == msg_id) {
            existing.sender = sender;
            existing.body = body;
            existing.outgoing = outgoing;
        } else {
            if msgs.len() == MAX_MESSAGES {
                msgs.remove(0);
            }
            msgs.push(TgMessage {
                id: msg_id,
                sender,
                body,
                outgoing,
                attachments: Vec::new(),
            });
        }
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Attach downloadable file metadata onto an existing message in the active chat.
    pub fn telegram_attachment(&mut self, chat_id: i64, msg_id: i64, file_id: i32, text: String) {
        if self.state.telegram.open_chat.as_ref().is_none_or(|(id, _)| *id != chat_id) {
            return;
        }
        let mut parts = text.splitn(4, '\n');
        let name = parts.next().filter(|s| !s.is_empty()).unwrap_or("file").to_string();
        let size = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let is_photo = parts.next().is_some_and(|k| k == "photo");
        if let Some(msg) = self.state.telegram.messages.iter_mut().find(|m| m.id == msg_id)
            && !msg.attachments.iter().any(|a| a.file_id == file_id)
        {
            msg.attachments.push(TgAttachment { file_id, name, size, is_photo });
            if self.screen() == Screen::Telegram {
                self.refresh();
            }
        }
    }

    /// Record a completed attachment download and show a toast.
    pub fn telegram_file_ready(&mut self, path: String, now: u64) {
        self.state.telegram.download_note = Some(path.clone());
        self.say(&path, now);
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }

    /// Record that the isolated Telegram worker exited.
    pub fn telegram_ended(&mut self, msg: Option<String>, now: u64) {
        self.state.telegram.active = false;
        self.state.telegram.starting = false;
        self.state.telegram.error = msg.clone();
        if let Some(m) = msg {
            self.wake(now);
            self.say(&m, now);
        }
        if self.screen() == Screen::Telegram {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn telegram_auth_chats_and_compose_work() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Plugins, W, H, PPD);
        app.set_aerap_plugins(vec![AerapEntry {
            id: "telegram".into(),
            name: "AERA Telegram".into(),
            version: "1.3.5".into(),
            description: "TDLib client".into(),
            kind: aweyuki_install::aerap::AerapKind::Telegram,
            installed: true,
            path: std::path::PathBuf::from("/sdcard/Aweyuki/plugins/telegram"),
        }]);

        assert_eq!(
            tap(&mut app, &f, &i, Action::LaunchAerap(0)),
            vec![Effect::StartTelegram(std::path::PathBuf::from("/sdcard/Aweyuki/plugins/telegram"))]
        );
        assert_eq!(app.screen(), Screen::Telegram);

        app.telegram_started(10);
        app.telegram_state(TgAuthState::NeedPhone, String::new(), 11);
        app.state.telegram.phone = "+84901234567".into();
        assert_eq!(
            tap_at(&mut app, &f, &i, Action::TgSubmit, 12),
            vec![Effect::TelegramAuth { kind: TgKind::Phone, text: "+84901234567".into() }]
        );

        app.telegram_state(TgAuthState::Ready, String::new(), 13);
        app.telegram_chat(100, 2, "Saved Messages\nhello\n".into());
        app.telegram_chats_done();
        assert_eq!(tap_at(&mut app, &f, &i, Action::TgOpenChat(0), 14), vec![Effect::TelegramOpenChat(100)]);
        app.telegram_message(100, 1, 0, "Alice\n\nhi there".into());

        app.state.telegram.compose = "ping".into();
        assert_eq!(
            tap_at(&mut app, &f, &i, Action::Type(Cap::Enter), 15),
            vec![Effect::TelegramSendText { chat_id: 100, text: "ping".into() }]
        );

        // First Back from open chat returns to chat list without stopping daemon
        let fx = tap_at(&mut app, &f, &i, Action::Back, 16);
        assert!(fx.is_empty());
        assert_eq!(app.screen(), Screen::Telegram);
        assert!(app.state.telegram.open_chat.is_none());

        // Second Back leaves Telegram and stops the worker
        let fx = tap_at(&mut app, &f, &i, Action::Back, 17);
        assert_eq!(fx, vec![Effect::StopTelegram]);
        assert_eq!(app.screen(), Screen::Plugins);
    }
}
