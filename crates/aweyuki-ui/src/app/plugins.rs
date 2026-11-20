//! The runtime plugins pages: discovered plugins and live execution output.

use super::*;
use aweyuki_install::plugins::PluginManifest;

/// How a plugin run finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginEnd {
    Exited(i32),
    Stopped,
    Failed(String),
}

/// Active or completed plugin execution shown on [`Screen::PluginRun`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginRun {
    pub id: String,
    pub name: String,
    pub output: Vec<String>,
    pub stopping: bool,
    pub outcome: Option<PluginEnd>,
}

impl App {
    /// Set the list of discovered runtime plugins.
    pub fn set_plugins(&mut self, plugins: Vec<PluginManifest>) {
        self.state.plugins = Some(plugins);
        if self.screen() == Screen::Plugins {
            self.refresh();
        }
    }

    /// Open the plugin output page for a run that has just started.
    pub fn plugin_started(&mut self, id: String, name: String) {
        self.state.plugin_run = Some(PluginRun {
            id,
            name,
            output: Vec::new(),
            stopping: false,
            outcome: None,
        });
        self.open(Screen::PluginRun);
    }

    /// Append one line of plugin stdout/stderr output.
    pub fn plugin_output(&mut self, line: String) {
        let Some(run) = self.state.plugin_run.as_mut() else {
            return;
        };
        if run.output.len() == SCRIPT_LINES {
            run.output.remove(0);
        }
        run.output.push(line);
        if self.screen() == Screen::PluginRun {
            self.refresh();
        }
    }

    /// Record how the active plugin run ended and wake the panel so it can be read.
    pub fn plugin_ended(&mut self, end: PluginEnd, now: u64) {
        if let Some(run) = self.state.plugin_run.as_mut() {
            run.stopping = false;
            run.outcome = Some(end);
        }
        self.wake(now);
        if self.screen() == Screen::PluginRun {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn plugins_page_lists_runs_stops_and_shows_output() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Advanced, W, H, PPD);
        assert_eq!(tap(&mut app, &f, &i, Action::Open(Screen::Plugins)), vec![Effect::ReadPlugins]);
        assert_eq!(app.screen(), Screen::Plugins);

        app.set_plugins(vec![PluginManifest {
            id: "diag".into(),
            name: "Diagnostics".into(),
            version: "1.0".into(),
            description: "Check slots".into(),
            entry: "run.sh".into(),
            args: Vec::new(),
        }]);
        assert_eq!(tap(&mut app, &f, &i, Action::RefreshPlugins), vec![Effect::ReadPlugins]);
        assert_eq!(tap(&mut app, &f, &i, Action::RunPlugin(0)), vec![Effect::RunPlugin("diag".into())]);

        app.plugin_started("diag".into(), "Diagnostics".into());
        assert_eq!(app.screen(), Screen::PluginRun);
        assert!(!app.frame(&f, &i).back);
        app.key(Key::Back, 0);
        assert_eq!(app.screen(), Screen::PluginRun);
        assert_eq!(toast(&app), Some("Wait for it to finish, or stop it, before going back"));

        app.plugin_output("slot=_a".into());
        assert_eq!(tap(&mut app, &f, &i, Action::StopPlugin), vec![Effect::StopPlugin]);
        assert!(!app.frame(&f, &i).hits.iter().any(|h| h.action == Action::StopPlugin));

        app.plugin_ended(PluginEnd::Stopped, 0);
        assert!(app.frame(&f, &i).back);
        tap_at(&mut app, &f, &i, Action::Back, 2000);
        assert_eq!((app.screen(), app.state.plugin_run.as_ref()), (Screen::Plugins, None));
    }
}
