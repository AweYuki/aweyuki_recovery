//! Isolated AERA WPE WebKit browser viewport, navigation, and input state.

use super::*;
use crate::geom::Rect;
use aweyuki_install::aerap::browser_proto::{HEIGHT, VIEW_HEIGHT, VIEW_WIDTH, WIDTH};
use std::sync::Arc;

/// Touch event kind sent over the browser worker IPC socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserTouchKind {
    Down,
    Move,
    Up,
}

/// High-level browser navigation command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserNavOp {
    Back,
    Forward,
    Reload,
    Stop,
}

/// UI state for [`Screen::Browser`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserState {
    pub active: bool,
    pub starting: bool,
    pub unpack_pct: u8,
    pub url: String,
    pub url_input: String,
    pub editing_url: bool,
    pub show_keyboard: bool,
    pub progress: u8,
    pub can_back: bool,
    pub can_forward: bool,
    pub status: Option<String>,
    pub error: Option<String>,
    pub frame: Option<Arc<Vec<u8>>>,
    pub frame_w: u32,
    pub frame_h: u32,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            active: false,
            starting: false,
            unpack_pct: 0,
            url: "aera://start".into(),
            url_input: "aera://start".into(),
            editing_url: false,
            show_keyboard: false,
            progress: 0,
            can_back: false,
            can_forward: false,
            status: None,
            error: None,
            frame: None,
            frame_w: WIDTH,
            frame_h: HEIGHT,
        }
    }
}

/// Map a screen-space touch `(x, y)` inside `rect` to AERA Browser's virtual
/// `VIEW_WIDTH x VIEW_HEIGHT` (`360x700`) CSS viewport coordinate space.
pub(super) fn scale_browser_touch(rect: Rect, x: f32, y: f32) -> (i32, i32) {
    let vw = VIEW_WIDTH as f32;
    let vh = VIEW_HEIGHT as f32;
    let nx = ((x - rect.x) / rect.w.max(1.0)).clamp(0.0, 0.9999);
    let ny = ((y - rect.y) / rect.h.max(1.0)).clamp(0.0, 0.9999);
    ((nx * vw) as i32, (ny * vh) as i32)
}

impl App {
    /// Update XZ unpacking percentage while starting [`Screen::Browser`].
    pub fn browser_unpack_progress(&mut self, pct: u8) {
        self.state.browser.unpack_pct = pct.min(100);
        if self.screen() == Screen::Browser {
            self.refresh();
        }
    }

    /// Mark the isolated browser worker live and ready to receive frames.
    pub fn browser_started(&mut self, now: u64) {
        self.state.browser.active = true;
        self.state.browser.starting = false;
        self.state.browser.unpack_pct = 100;
        self.state.browser.error = None;
        self.wake(now);
        if self.screen() == Screen::Browser {
            self.refresh();
        }
    }

    /// Present the latest `BGRA8888` frame from shared memory.
    pub fn browser_frame(&mut self, w: u32, h: u32, bgra: Arc<Vec<u8>>) {
        self.state.browser.frame_w = w;
        self.state.browser.frame_h = h;
        self.state.browser.frame = Some(bgra);
        self.state.browser.starting = false;
        self.state.browser.active = true;
        if self.screen() == Screen::Browser {
            self.refresh();
        }
    }

    /// Update page URL, load progress, and history button availability.
    pub fn browser_status(&mut self, url: String, pct: u8, can_back: bool, can_forward: bool) {
        let b = &mut self.state.browser;
        if !url.is_empty() {
            if !b.editing_url {
                b.url_input = url.clone();
            }
            b.url = url;
        }
        b.progress = pct.min(100);
        b.can_back = can_back;
        b.can_forward = can_forward;
        b.status = None;
        if self.screen() == Screen::Browser {
            self.refresh();
        }
    }

    /// Show or hide the soft keyboard for focused page form inputs.
    pub fn browser_keyboard(&mut self, show: bool) {
        if !self.state.browser.editing_url {
            self.state.browser.show_keyboard = show;
            if self.screen() == Screen::Browser {
                self.refresh();
            }
        }
    }

    /// Report a non-fatal browser runtime error or blocked address message.
    pub fn browser_error(&mut self, msg: String, now: u64) {
        self.state.browser.error = Some(msg.clone());
        self.state.browser.starting = false;
        self.say(&msg, now);
        if self.screen() == Screen::Browser {
            self.refresh();
        }
    }

    /// Record that the isolated browser worker exited.
    pub fn browser_ended(&mut self, msg: Option<String>, now: u64) {
        self.state.browser.active = false;
        self.state.browser.starting = false;
        self.state.browser.error = msg.clone();
        if let Some(m) = msg {
            self.wake(now);
            self.say(&m, now);
        }
        if self.screen() == Screen::Browser {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn browser_opens_scales_touches_submits_url_and_stops_on_back() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Plugins, W, H, PPD);
        app.set_aerap_plugins(vec![AerapEntry {
            id: "browser".into(),
            name: "AERA Browser".into(),
            version: "1.5.6".into(),
            description: "WPE WebKit".into(),
            kind: aweyuki_install::aerap::AerapKind::Browser,
            installed: true,
            path: std::path::PathBuf::from("/sdcard/Aweyuki/plugins/browser"),
        }]);

        assert_eq!(
            tap(&mut app, &f, &i, Action::LaunchAerap(0)),
            vec![Effect::StartBrowser(std::path::PathBuf::from("/sdcard/Aweyuki/plugins/browser"))]
        );
        assert_eq!(app.screen(), Screen::Browser);

        app.browser_started(10);
        let px = Arc::new(vec![0xffu8; (WIDTH * HEIGHT * 4) as usize]);
        app.browser_frame(WIDTH, HEIGHT, px);
        app.browser_status("https://example.org".into(), 100, true, false);

        // Scale viewport center -> 180x350
        let rect = app.frame(&f, &i).hits.iter().find(|h| h.action == Action::BrowserViewport).unwrap().rect;
        assert_eq!(scale_browser_touch(rect, rect.x + rect.w * 0.5, rect.y + rect.h * 0.5), (180, 350));

        // Edit URL bar and submit
        tap_at(&mut app, &f, &i, Action::BrowserEditUrl, 1000);
        app.state.browser.url_input = "aweyuki.org".into();
        let fx = tap_at(&mut app, &f, &i, Action::Type(Cap::Enter), 1100);
        assert_eq!(fx, vec![Effect::BrowserOpen("https://aweyuki.org".into())]);
        assert!(!app.state.browser.editing_url);

        // Navigating back stops the isolated browser process
        let fx = app.key(Key::Back, 1200);
        assert!(fx.contains(&Effect::StopBrowser));
        assert_eq!(app.screen(), Screen::Plugins);
    }
}
