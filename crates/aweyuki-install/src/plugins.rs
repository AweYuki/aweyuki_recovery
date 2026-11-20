//! Runtime plugins: discover and run external tools/scripts packaged
//! with a `plugin.toml` manifest under `/aweyuki/plugins` or
//! `/sdcard/Aweyuki/plugins`.

use anyhow::{bail, ensure, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

pub const DEFAULT_PLUGIN_DIRS: &[&str] = &[
    "/aweyuki/plugins",
    "/sdcard/Aweyuki/plugins",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub entry: String,
    pub args: Vec<String>,
}

fn parse_quoted(src: &str) -> Result<(String, &str)> {
    let s = src.trim_start();
    let Some(rest) = s.strip_prefix('"') else {
        bail!("expected quoted string near {s:?}");
    };
    let mut out = String::new();
    let mut chars = rest.char_indices();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '"' => return Ok((out, &rest[idx + 1..])),
            '\\' => match chars.next() {
                Some((_, escaped)) => out.push(escaped),
                None => bail!("unterminated escape sequence"),
            },
            c => out.push(c),
        }
    }
    bail!("unterminated quoted string")
}

fn unquote(src: &str) -> Result<String> {
    let (val, tail) = parse_quoted(src)?;
    ensure!(tail.trim().is_empty(), "unexpected trailing characters {tail:?}");
    Ok(val)
}

fn parse_args(src: &str) -> Result<Vec<String>> {
    let mut rest = src
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .context("invalid args array")?
        .trim();
    let mut args = Vec::new();
    while !rest.is_empty() {
        let (arg, tail) = parse_quoted(rest)?;
        args.push(arg);
        rest = tail.trim_start();
        if let Some(after_comma) = rest.strip_prefix(',') {
            rest = after_comma.trim_start();
        } else {
            ensure!(rest.is_empty(), "expected comma in args array near {rest:?}");
        }
    }
    Ok(args)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && !id.contains("..")
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn parse_manifest(text: &str) -> Result<PluginManifest> {
    let mut id = String::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut description = String::new();
    let mut entry = String::new();
    let mut args = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "id" => id = unquote(v)?,
            "name" => name = unquote(v)?,
            "version" => version = unquote(v)?,
            "description" => description = unquote(v)?,
            "entry" => entry = unquote(v)?,
            "args" => args = parse_args(v)?,
            _ => {}
        }
    }
    if id.is_empty() || name.is_empty() || entry.is_empty() {
        bail!("plugin.toml must specify id, name, and entry");
    }
    Ok(PluginManifest { id, name, version, description, entry, args })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub dir: PathBuf,
    pub manifest: PluginManifest,
}

impl Plugin {
    /// Load a plugin from `dir/plugin.toml` and validate its `id` and `entry`.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest is missing/malformed, `id` or `entry`
    /// fails validation, or the entry file does not exist.
    pub fn load(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join("plugin.toml");
        let text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?;
        let manifest = parse_manifest(&text)
            .with_context(|| format!("parse {}", manifest_path.display()))?;
        ensure!(valid_id(&manifest.id), "invalid plugin id {:?}", manifest.id);
        let entry_rel = Path::new(&manifest.entry);
        ensure!(
            !entry_rel.as_os_str().is_empty()
                && !entry_rel.is_absolute()
                && entry_rel.components().all(|c| matches!(c, Component::Normal(_))),
            "invalid plugin entry {:?}",
            manifest.entry
        );
        let entry_full = dir.join(entry_rel);
        ensure!(entry_full.is_file(), "plugin entry {} does not exist", entry_full.display());
        Ok(Plugin { dir: dir.to_path_buf(), manifest })
    }

    /// Execute the plugin's entry binary/script, returning its exit status.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin process fails to spawn or wait.
    pub fn run(&self, extra_args: &[&str]) -> Result<ExitStatus> {
        let entry = self.dir.join(&self.manifest.entry);
        Command::new(&entry)
            .args(&self.manifest.args)
            .args(extra_args)
            .env("AWEYUKI_PLUGIN_DIR", &self.dir)
            .env("AWEYUKI_PLUGIN_ID", &self.manifest.id)
            .status()
            .with_context(|| format!("run {}", entry.display()))
    }

    /// Execute the plugin while streaming stdout and stderr lines to `on_line`
    /// and polling `cancel` so a user can terminate a running plugin cleanly.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails to spawn or wait.
    pub fn run_streaming(
        &self,
        extra_args: &[&str],
        cancel: &AtomicBool,
        mut on_line: impl FnMut(String),
    ) -> Result<ExitStatus> {
        let entry = self.dir.join(&self.manifest.entry);
        let mut child = Command::new(&entry)
            .args(&self.manifest.args)
            .args(extra_args)
            .env("AWEYUKI_PLUGIN_DIR", &self.dir)
            .env("AWEYUKI_PLUGIN_ID", &self.manifest.id)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("run {}", entry.display()))?;

        let (tx, rx) = mpsc::channel();
        let out_thread = child.stdout.take().map(|p| forward_lines(p, tx.clone()));
        let err_thread = child.stderr.take().map(|p| forward_lines(p, tx.clone()));
        drop(tx);

        let mut killed = false;
        loop {
            if !killed && cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                killed = true;
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => on_line(line),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if let Some(handle) = out_thread {
            let _ = handle.join();
        }
        if let Some(handle) = err_thread {
            let _ = handle.join();
        }
        child.wait().with_context(|| format!("wait for {}", entry.display()))
    }
}

fn forward_lines(pipe: impl Read + Send + 'static, tx: mpsc::Sender<String>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    })
}

/// Discover all valid plugins across `dirs`, deduplicating by `id` (earlier
/// directories take precedence) and returning them sorted by `id`.
#[must_use]
pub fn discover(dirs: &[&Path]) -> Vec<Plugin> {
    let mut by_id = BTreeMap::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Ok(plugin) = Plugin::load(&path)
            {
                by_id.entry(plugin.manifest.id.clone()).or_insert(plugin);
            }
        }
    }
    by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_plugin_is_discovered_deduplicated_and_run() {
        let tmp = tempfile::tempdir().unwrap();
        let high = tmp.path().join("high");
        let low = tmp.path().join("low");
        let p_high = high.join("hello");
        let p_low = low.join("hello");
        fs::create_dir_all(&p_high).unwrap();
        fs::create_dir_all(&p_low).unwrap();

        fs::write(
            p_high.join("plugin.toml"),
            "id = \"hello\"\nname = \"Hello Plugin\"\nversion = \"1.0\"\nentry = \"run.sh\"\nargs = [\"--fmt=a,b\", \"--ok\"]\n",
        )
        .unwrap();
        fs::write(p_low.join("plugin.toml"), "id = \"hello\"\nname = \"Shadowed\"\nentry = \"run.sh\"\n").unwrap();
        for p in [&p_high, &p_low] {
            let sh = p.join("run.sh");
            fs::write(&sh, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&sh, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }

        let found = discover(&[high.as_path(), low.as_path()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.name, "Hello Plugin");
        assert_eq!(found[0].manifest.args, ["--fmt=a,b", "--ok"]);
        assert!(found[0].run(&["--extra"]).unwrap().success());
    }

    #[test]
    fn invalid_plugins_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let bad_trav = tmp.path().join("trav");
        fs::create_dir_all(&bad_trav).unwrap();
        fs::write(bad_trav.join("plugin.toml"), "id = \"bad\"\nname = \"Bad\"\nentry = \"../../bin/sh\"\n").unwrap();
        assert!(Plugin::load(&bad_trav).is_err());

        let bad_missing = tmp.path().join("missing");
        fs::create_dir_all(&bad_missing).unwrap();
        fs::write(bad_missing.join("plugin.toml"), "id = \"ok\"\nname = \"Missing\"\nentry = \"nope.sh\"\n").unwrap();
        assert!(Plugin::load(&bad_missing).is_err());

        let bad_id = tmp.path().join("id");
        fs::create_dir_all(&bad_id).unwrap();
        fs::write(bad_id.join("run.sh"), "#!/bin/sh\n").unwrap();
        fs::write(bad_id.join("plugin.toml"), "id = \"bad/id\"\nname = \"Bad Id\"\nentry = \"run.sh\"\n").unwrap();
        assert!(Plugin::load(&bad_id).is_err());
    }

    #[test]
    fn streaming_execution_captures_output_and_stops_on_cancel() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("stream");
        fs::create_dir_all(&dir).unwrap();
        let sh = dir.join("stream.sh");
        fs::write(&sh, "#!/bin/sh\necho stdout-line\necho stderr-line >&2\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&sh, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(dir.join("plugin.toml"), "id = \"stream\"\nname = \"Stream\"\nentry = \"stream.sh\"\n").unwrap();
        let plugin = Plugin::load(&dir).unwrap();
        let cancel = AtomicBool::new(false);
        let mut lines = Vec::new();
        let status = plugin.run_streaming(&[], &cancel, |l| lines.push(l)).unwrap();
        assert!(status.success());
        assert!(lines.contains(&"stdout-line".to_string()), "{lines:?}");
        assert!(lines.contains(&"stderr-line".to_string()), "{lines:?}");

        let sleep_sh = dir.join("sleep.sh");
        fs::write(&sleep_sh, "#!/bin/sh\nsleep 10\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&sleep_sh, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(dir.join("plugin.toml"), "id = \"sleep\"\nname = \"Sleep\"\nentry = \"sleep.sh\"\n").unwrap();
        let sleeper = Plugin::load(&dir).unwrap();
        let cancel = AtomicBool::new(true);
        let status = sleeper.run_streaming(&[], &cancel, |_| {}).unwrap();
        assert!(!status.success());
    }
}
