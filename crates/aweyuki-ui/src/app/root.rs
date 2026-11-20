//! The Root Manager pages: kernel KMI, detected root providers, and toggling
//! `/data/adb/modules/*`.

use super::*;
use aweyuki_install::root::{ModuleInfo, Provider};

/// Root environment snapshot reported by the runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootOverview {
    pub kernel: String,
    pub kmi: Option<String>,
    pub providers: Vec<Provider>,
    pub modules: Result<Vec<ModuleInfo>, String>,
}

impl App {
    /// Update the Root Manager snapshot shown on the root and module pages.
    pub fn set_root(&mut self, overview: RootOverview) {
        let missing_selected = self.screen() == Screen::RootModule
            && self.state.root_module.as_deref().is_some_and(|id| {
                !overview
                    .modules
                    .as_ref()
                    .is_ok_and(|mods| mods.iter().any(|m| m.id == id))
            });
        self.state.root = Some(overview);
        if missing_selected {
            self.state.root_module = None;
            if self.stack.len() > 1 {
                self.leave(true);
                self.stack.pop();
                self.settle_page();
            }
        }
        if matches!(self.screen(), Screen::Root | Screen::RootModule) {
            self.refresh();
        }
    }

    /// Take the outcome of toggling a module state; either refresh or show why it failed.
    pub fn root_changed(&mut self, outcome: Result<RootOverview, String>, now: u64) {
        match outcome {
            Ok(overview) => self.set_root(overview),
            Err(why) => self.report(&why, now),
        }
    }

    /// Module currently opened on [`Screen::RootModule`], if still present.
    pub(crate) fn selected_root_module(&self) -> Option<&ModuleInfo> {
        let id = self.state.root_module.as_deref()?;
        let mods = self.state.root.as_ref()?.modules.as_ref().ok()?;
        mods.iter().find(|m| m.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn root_page_reads_and_toggles_modules() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Advanced, W, H, PPD);
        assert_eq!(tap(&mut app, &f, &i, Action::Open(Screen::Root)), vec![Effect::ReadRoot]);
        assert_eq!(app.screen(), Screen::Root);

        let overview = RootOverview {
            kernel: "6.1.75-android14-11".into(),
            kmi: Some("android14-6.1".into()),
            providers: vec![Provider::KernelSu],
            modules: Ok(vec![ModuleInfo {
                id: "zygisksu".into(),
                name: "Zygisk Next".into(),
                version: "v1.2.0".into(),
                author: "Dr-TSNG".into(),
                description: "Standalone Zygisk".into(),
                enabled: true,
                remove_pending: false,
            }]),
        };
        app.set_root(overview.clone());
        assert_eq!(tap(&mut app, &f, &i, Action::RefreshRoot), vec![Effect::ReadRoot]);

        tap(&mut app, &f, &i, Action::OpenRootModule(0));
        assert_eq!(app.screen(), Screen::RootModule);
        assert_eq!(
            tap(&mut app, &f, &i, Action::RootToggleEnabled),
            vec![Effect::RootSetEnabled { id: "zygisksu".into(), enabled: false }]
        );
        assert_eq!(
            tap(&mut app, &f, &i, Action::RootToggleRemove),
            vec![Effect::RootSetRemove { id: "zygisksu".into(), remove: true }]
        );

        app.root_changed(Err("Permission denied".into()), 100);
        assert_eq!(toast(&app), Some("Permission denied"));

        tap_back(&mut app, &f, &i, 0);
        assert_eq!((app.screen(), app.state.root_module.as_ref()), (Screen::Root, None));

        // When no modules exist and data is locked, Decrypt shortcut is shown.
        app.set_root(RootOverview { modules: Ok(Vec::new()), ..overview });
        assert!(app.frame(&f, &i).hits.iter().any(|h| h.action == Action::Open(Screen::Decrypt)));
    }
}
