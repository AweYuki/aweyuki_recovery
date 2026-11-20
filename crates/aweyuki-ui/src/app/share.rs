//! The network file-sharing page: HTTP/1.1 browser upload/download server state.

use super::*;

/// Which folder the HTTP file-sharing server exposes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShareRoot {
    #[default]
    Storage,
    Root,
}

impl ShareRoot {
    pub const ALL: [ShareRoot; 2] = [ShareRoot::Storage, ShareRoot::Root];
}

/// Network file-sharing server state shown on the share and advanced pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareState {
    pub running: bool,
    pub root: ShareRoot,
    pub port: u16,
    pub address: Option<String>,
    pub error: Option<String>,
}

impl Default for ShareState {
    fn default() -> Self {
        ShareState {
            running: false,
            root: ShareRoot::Storage,
            port: aweyuki_net::share::DEFAULT_PORT,
            address: None,
            error: None,
        }
    }
}

impl App {
    /// Update the HTTP file-sharing server status reported by the runtime.
    pub fn set_share(&mut self, running: bool, address: Option<String>, error: Option<String>) {
        self.state.share.running = running;
        self.state.share.address = address;
        self.state.share.error = error;
        if matches!(self.screen(), Screen::Share | Screen::Advanced) {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn share_switch_checks_decryption_for_storage_and_starts_server() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Advanced, W, H, PPD);
        tap(&mut app, &f, &i, Action::Open(Screen::Share));
        assert_eq!(app.screen(), Screen::Share);

        assert!(tap(&mut app, &f, &i, Action::Toggle(Switch::Share)).is_empty());
        assert_eq!(toast(&app), Some("Decrypt first"));

        app.state.data = Data::Open;
        app.refresh();
        assert_eq!(
            tap(&mut app, &f, &i, Action::Toggle(Switch::Share)),
            vec![Effect::Share { on: true, root: ShareRoot::Storage, port: aweyuki_net::share::DEFAULT_PORT }]
        );
        app.set_share(true, Some("192.168.1.42".into()), None);
        assert_eq!(
            tap(&mut app, &f, &i, Action::ShareRoot(ShareRoot::Root)),
            vec![Effect::Share { on: true, root: ShareRoot::Root, port: aweyuki_net::share::DEFAULT_PORT }]
        );
        assert_eq!(
            tap(&mut app, &f, &i, Action::Toggle(Switch::Share)),
            vec![Effect::Share { on: false, root: ShareRoot::Root, port: aweyuki_net::share::DEFAULT_PORT }]
        );
    }
}
