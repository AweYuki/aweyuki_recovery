//! Runtime plugins: discover and run external tools/scripts packaged
//! with a `plugin.toml` manifest under `/aweyuki/plugins` or
//! `/sdcard/Aweyuki/plugins`.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub const DEFAULT_PLUGIN_DIRS: &[&str] = &[
    "/aweyuki/plugins",
    "/sdcard/Aweyuki/plugins",
];

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub entry: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub dir: PathBuf,
    pub manifest: PluginManifest,
}

impl Plugin {
    /// Load a plugin from `dir/plugin.toml` and validate its `entry` path.
    pub fn load(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join("plugin.toml");
        let text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?;
        let manifest: PluginManifest = toml::from_str(&text)
            .with_context(|| format!("parse {}", manifest_path.display()))?;
        if manifest.id.is_empty() || manifest.id.contains('/') || manifest.id.contains("..") {
            bail!("invalid plugin id {:?}", manifest.id);
        }
        let entry_path = Path::new(&manifest.entry);
        if entry_path.is_absolute()
            || entry_path.components().any(|c| !matches!(c, Component::Normal(_)))
        {
            bail!("invalid plugin entry {:?}", manifest.entry);
        }
        Ok(Plugin { dir: dir.to_path_buf(), manifest })
    }

    /// Execute the plugin's entry binary/script, returning its exit code.
    pub fn run(&self, extra_args: &[String]) -> Result<i32> {
        let entry = self.dir.join(&self.manifest.entry);
        let status = Command::new(&entry)
            .args(&self.manifest.args)
            .args(extra_args)
            .env("AWEYUKI_PLUGIN_DIR", &self.dir)
            .env("AWEYUKI_PLUGIN_ID", &self.manifest.id)
            .status()
            .with_context(|| format!("run {}", entry.display()))?;
        Ok(status.code().unwrap_or(-1))
    }
}

/// Discover all valid plugins across `dirs` (sorted by plugin `id`).
pub fn discover(dirs: &[&Path]) -> Vec<Plugin> {
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Ok(plugin) = Plugin::load(&path)
            {
                found.push(plugin);
            }
        }
    }
    found.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_plugin_is_discovered_and_run() {
        let base = std::env::temp_dir().join("aweyuki-plugin-tests");
        let _ = fs::remove_dir_all(&base);
        let pdir = base.join("hello");
        fs::create_dir_all(&pdir).unwrap();
        fs::write(
            pdir.join("plugin.toml"),
            "id = \"hello\"\nname = \"Hello Plugin\"\nversion = \"1.0\"\nentry = \"run.sh\"\nargs = [\"--ok\"]\n",
        )
        .unwrap();
        fs::write(pdir.join("run.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(pdir.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        }

        let found = discover(&[base.as_path()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.name, "Hello Plugin");
        assert_eq!(found[0].run(&[]).unwrap(), 0);
    }

    #[test]
    fn traversal_entry_is_rejected() {
        let base = std::env::temp_dir().join("aweyuki-plugin-bad");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(
            base.join("plugin.toml"),
            "id = \"bad\"\nname = \"Bad\"\nentry = \"../../bin/sh\"\n",
        )
        .unwrap();
        assert!(Plugin::load(&base).is_err());
    }
}
