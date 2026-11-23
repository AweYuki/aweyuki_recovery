//! What a control does once it is tapped, typed on or swiped.

use super::*;

impl App {
    /// Carry out a tapped control.
    pub(super) fn act(&mut self, action: Action, now: u64) -> Vec<Effect> {
        let l = self.state.lang;
        match action {
            Action::Open(s) => {
                if s == Screen::Search && self.screen() == Screen::Home {
                    // The bar moves up from where it stands unfolded: a
                    // page easing there for the keys gets there at once.
                    if let Some((rest, _)) = self.seek.take() {
                        self.top_mut().1 = rest;
                    }
                    self.start_morph(true, now);
                    self.push(s);
                } else {
                    self.open(s);
                }
                return self.arrive_at(s);
            }
            Action::OpenData => return self.open_data(),
            Action::Skip => {
                self.leave(true);
                if self.stack.len() > 1 {
                    self.stack.pop();
                } else {
                    *self.top_mut() = (Screen::Home, 0.0, Focus::Unset);
                }
                self.settle_page();
                self.refresh();
            }
            Action::Back | Action::CloseSearch => return self.back(now),
            Action::Theme { dark } => {
                if self.state.dark != dark {
                    self.state.dark = dark;
                    self.refresh();
                }
            }
            // The switch moves at once; the runtime then tells how the WLAN
            // came up, or went off.
            Action::Toggle(Switch::Wlan) => {
                let w = self.state.wlan.as_mut().expect("the WLAN switch shows on a phone whose WLAN is brought up");
                let on = matches!(w.power, Power::Off | Power::Failed(_));
                *w = Wlan { power: if on { Power::Starting } else { Power::Off }, ..Wlan::default() };
                self.refresh();
                return vec![Effect::Wlan(on)];
            }
            Action::Toggle(Switch::Share) => {
                if !self.state.share.running
                    && self.state.share.root == ShareRoot::Storage
                    && !self.state.data.storage_open()
                {
                    self.say(l.pick(DECRYPT_FIRST), now);
                    return Vec::new();
                }
                return vec![Effect::Share {
                    on: !self.state.share.running,
                    root: self.state.share.root,
                    port: self.state.share.port,
                }];
            }
            // The zone the system's gave the clock stays as the one chosen,
            // where it is one the pages list.
            Action::Toggle(Switch::SystemTimeZone) => {
                if self.state.follow_system_zone
                    && let Some(at) = self.state.zone_listed()
                {
                    self.state.time_zone = at;
                }
                self.state.follow_system_zone = !self.state.follow_system_zone;
                self.refresh();
            }
            Action::Toggle(switch) => {
                let on = match switch {
                    Switch::Vibrate => &mut self.state.vibrate,
                    Switch::FileTransfer => &mut self.state.file_transfer,
                    Switch::Compress => &mut self.state.compress,
                    Switch::Verify => &mut self.state.verify,
                    Switch::Signature => &mut self.state.check_signature,
                    Switch::KeepRecovery => &mut self.state.keep_recovery,
                    Switch::ShowPassword => &mut self.state.show_password,
                    Switch::Wlan | Switch::Share | Switch::SystemTimeZone => unreachable!("the switch is turned above"),
                };
                *on = !*on;
                let now_on = *on;
                self.refresh();
                if switch == Switch::FileTransfer {
                    return vec![Effect::FileTransfer(now_on)];
                }
            }
            Action::MountToggle(i) => {
                if self.state.mount_failure.take().is_some() {
                    self.refresh();
                }
                let row = &self.state.mounts.as_ref().expect("a partition toggled is one the page lists")[i];
                let base = row.base.clone();
                return vec![if row.mounted { Effect::Unmount(base) } else { Effect::Mount(base) }];
            }
            Action::RebootTarget(i) => {
                self.state.reboot = i;
                self.refresh();
            }
            // Everything takes in every part, so choosing it leaves the
            // parts out, and choosing a part on its own leaves it out.
            Action::WipeItem(item) => {
                let k = WIPES.iter().position(|&w| w == item).expect("the page offers only what it lists");
                let on = !self.state.wipe[k];
                match (on, item) {
                    (true, Wipe::All) => {
                        self.state.wipe = [false; WIPES.len()];
                        self.state.wipe[k] = true;
                    }
                    (true, _) => {
                        self.state.wipe[0] = false;
                        self.state.wipe[k] = true;
                    }
                    (false, _) => self.state.wipe[k] = false,
                }
                self.refresh();
            }
            Action::Confirm(_) => self.say(l.pick(texts::toast::SLIDE_RIGHT), now),
            Action::Slider(_) | Action::Pattern => {}
            Action::Sideload => match self.state.install {
                Install::Idle | Install::Finished { .. } => {
                    self.begin_install(Install::Starting, Source::Computer, None);
                    if self.screen() == Screen::Installing {
                        self.refresh();
                    } else {
                        self.open(Screen::Installing);
                    }
                    return vec![Effect::Sideload];
                }
                _ => self.say(l.pick(INSTALL_UNFINISHED), now),
            },
            Action::FlashPartition(k) => {
                if let Install::Choosing(choice) = &mut self.state.install {
                    choice.partition = k;
                    self.refresh();
                }
            }
            Action::FlashSlots(pick) => {
                if let Install::Choosing(choice) = &mut self.state.install {
                    choice.pick = pick;
                    self.refresh();
                }
            }
            Action::SystemSize(k) => {
                if let Install::SystemChoosing(choice) = &mut self.state.install {
                    choice.size = k;
                    self.refresh();
                }
            }
            Action::PickSystem(who) => {
                if self.state.systems_work.is_none() {
                    self.state.system_pick = Some(who);
                    self.state.asking_delete = false;
                    self.state.confirm = 0.0;
                    self.refresh();
                }
            }
            Action::RebootTo(target) => return vec![Effect::Reboot(target)],
            Action::EnterRecovery => return vec![Effect::EnterRecovery],
            Action::DelayedScreenshot => {
                self.shot_at = Some(now + DELAYED_SHOT_MS);
                let msg = l.pick(crate::texts::fork::s::M001);
                self.toast = Some((msg.to_string(), now + DELAYED_SHOT_TOAST_MS));
                self.dirty = true;
            }
            Action::Dismiss => {
                self.stack = vec![(Screen::Home, 0.0, Focus::Unset)];
                self.settle_page();
                self.refresh();
                return vec![Effect::Dismiss];
            }
            Action::PickEntry(op) => {
                let path = self.entry_path();
                self.state.clip = Some(Clip { op, path });
                return self.back(now);
            }
            // A folder cannot go inside itself, and an entry moved to the
            // folder it is in stays put: the entry stays picked for another
            // folder.
            Action::Paste => {
                let clip = self.state.clip.clone().expect("only a picked entry is pasted");
                let (path, folder) = (std::path::Path::new(&clip.path), std::path::Path::new(&self.state.files.path));
                let name = path.file_name().map_or_else(|| clip.path.clone(), |n| n.to_string_lossy().into_owned());
                if folder.starts_with(path) {
                    self.say(&texts::toast::inside_itself(l, name), now);
                } else if clip.op == FileOp::Move && path.parent() == Some(folder) {
                    self.say(&texts::toast::already_there(l, name), now);
                } else {
                    return vec![Effect::Paste { op: clip.op, path: clip.path, into: self.state.files.path.clone(), clash: None }];
                }
            }
            Action::Unpick => {
                self.state.clip = None;
                self.refresh();
            }
            Action::RenameEntry => {
                self.state.new_name = self.state.file.as_ref().expect("the file page shows an entry").name.clone();
                self.state.layer = Layer::Letters;
                self.open(Screen::Rename);
            }
            Action::EditEntry => return vec![Effect::OpenText(self.entry_path())],
            Action::InstallEntry => {
                let size = self.state.file.as_ref().expect("the file page shows an entry").size;
                return self.install_opened(self.entry_path(), size, now);
            }
            Action::SaveText => {
                let ed = self.state.editor.as_ref().expect("the editor page shows a file");
                return vec![Effect::SaveText { path: ed.path.clone(), text: ed.text() }];
            }
            Action::OpenPermissions => self.open(Screen::Permissions),
            Action::ModeBit(bit) => {
                let mode = self.state.file.as_ref().expect("the permissions page shows an entry").mode;
                return vec![Effect::SetMode { path: self.entry_path(), mode: (mode ^ bit) & 0o7777 }];
            }
            Action::AskDelete => {
                self.state.asking_delete = true;
                self.state.confirm = 0.0;
                self.refresh();
            }
            Action::Resolve(choice) => {
                let (clip, into, _) = self.state.clash.take().expect("a choice answers a clash");
                self.leave(true);
                self.stack.pop();
                self.settle_page();
                self.refresh();
                // Skipping the one entry picked leaves nothing to do.
                if choice == ClashChoice::Skip {
                    self.state.clip = None;
                    return Vec::new();
                }
                return vec![Effect::Paste { op: clip.op, path: clip.path, into, clash: Some(choice) }];
            }
            Action::StopJob => {
                let job = self.state.job.as_mut().expect("only work under way is stopped");
                if job.outcome.is_none() && !job.stopping {
                    job.stopping = true;
                    self.refresh();
                    return vec![Effect::StopJob];
                }
            }
            Action::StopRelabel => {
                let job = self.state.relabel.as_mut().expect("only work under way is stopped");
                if job.outcome.is_none() && !job.stopping {
                    job.stopping = true;
                    self.refresh();
                    return vec![Effect::StopRelabel];
                }
            }
            Action::OpenFilesystem(k) => {
                self.state.fs_chosen = k;
                self.state.fs_action = FsAction::Repair;
                self.open(Screen::Filesystem);
            }
            Action::ChooseFs(choice) => {
                let row = self.fs_row().expect("the page of a partition shows one that was read");
                let action = match choice {
                    FsChoice::Repair => FsAction::Repair,
                    FsChoice::Remake(k) => FsAction::Remake(row.others[k].clone()),
                };
                self.state.fs_action = action;
                self.refresh();
            }
            Action::ScreenOff(k) => {
                self.state.screen_off = k;
                self.refresh();
            }
            Action::TimeZone(k) => {
                self.state.time_zone = k;
                self.state.follow_system_zone = false;
                self.refresh();
            }
            Action::Language(l) => {
                self.state.lang = l;
                self.state.language_chosen = true;
                self.refresh();
            }
            Action::BackupPart(k) => {
                self.state.backup[k] = !self.state.backup[k];
                self.refresh();
            }
            Action::BackupTo(place) => {
                self.state.backup_to = place;
                self.refresh();
            }
            // All a backup holds is ticked to restore at first.
            Action::OpenBackup(i) => {
                let entry = self.state.backups[i].clone();
                let parts = entry.held.as_ref().map_or([false; 6], |h| h.parts);
                self.state.restore = Some((entry, parts));
                self.state.asking_delete = false;
                self.open(Screen::RestoreBackup);
            }
            Action::RestorePart(k) => {
                let (_, parts) = self.state.restore.as_mut().expect("the restore page shows a backup");
                parts[k] = !parts[k];
                self.refresh();
            }
            Action::StopBackup => {
                let job = self.state.backup_job.as_mut().expect("only work under way is stopped");
                if job.outcome.is_none() && !job.stopping {
                    job.stopping = true;
                    self.refresh();
                    return vec![Effect::StopBackup];
                }
            }
            Action::LogPart(k) => {
                self.state.logs[k] = !self.state.logs[k];
                self.refresh();
            }
            Action::LogsTo(k) => {
                self.state.logs_to = k;
                self.refresh();
            }
            Action::ExportLogs => {
                let ([recovery, kernel], storage) = (self.state.logs, self.state.logs_to == 1);
                if !recovery && !kernel {
                    self.say(l.pick(texts::toast::CHOOSE_LOGS), now);
                } else if storage && !self.state.data.storage_open() {
                    self.say(l.pick(DECRYPT_FIRST), now);
                } else {
                    return vec![Effect::ExportLogs { recovery, kernel, storage }];
                }
            }
            Action::PickSlot(slot) => {
                self.state.next_slot = Some(slot);
                self.refresh();
            }
            Action::Type(cap) if self.screen() == Screen::Terminal => return self.term_typed(cap),
            Action::Type(cap) if self.screen() == Screen::Editor => self.edit_typed(cap),
            Action::Type(cap) => return self.type_key(cap, now),
            Action::TermKey(key) if self.screen() == Screen::Editor => {
                let ed = self.state.editor.as_mut().expect("the editor page shows a file");
                match key {
                    TermKey::Tab => ed.insert('\t'),
                    TermKey::Left => ed.left(),
                    TermKey::Up => ed.up(),
                    TermKey::Down => ed.down(),
                    TermKey::Right => ed.right(),
                    TermKey::Esc | TermKey::Ctrl => unreachable!("the editor's row has no {key:?}"),
                }
                self.edits += 1;
                self.refresh_text();
            }
            Action::TermKey(key) => {
                let Some(term) = self.term.as_mut() else { return Vec::new() };
                let bytes = term.key(key);
                let running = term.running;
                self.refresh();
                if running && !bytes.is_empty() {
                    return vec![Effect::TermInput(bytes)];
                }
            }
            Action::ClearSearch => {
                self.state.search.clear();
                self.edits += 1;
                self.refresh();
            }
            Action::OpenEntry(i) => {
                let entry = self.state.files.entry(i).clone();
                match entry.kind {
                    FileKind::Folder { .. } | FileKind::Link { folder: true, .. } => {
                        let scroll = self.top().1;
                        self.state.files.leave(scroll);
                        return vec![Effect::ReadDir(self.state.files.inside(&entry.name))];
                    }
                    FileKind::File if matches!(files::file_type(&entry.name), FileType::Text | FileType::Code) => {
                        return vec![Effect::OpenText(self.state.files.inside(&entry.name))];
                    }
                    _ if installs(&entry) => {
                        let path = self.state.files.inside(&entry.name);
                        return self.install_opened(path, entry.size, now);
                    }
                    FileKind::File | FileKind::Link { folder: false, .. } | FileKind::Special => {
                        self.state.file = Some(entry);
                        self.open(Screen::FileInfo);
                    }
                }
            }
            // A shell already running goes to the folder; otherwise one starts
            // there.
            Action::TerminalHere => {
                let folder = self.state.files.path.clone();
                let running = self.term.as_ref().is_some_and(|term| term.running);
                self.open(Screen::Terminal);
                self.state.layer = Layer::Letters;
                if running {
                    return vec![Effect::ShellTo(folder)];
                }
                self.shell_folder = folder.clone();
                return vec![Effect::StartShell(folder)];
            }
            Action::ChooseEntry(i) => {
                let screen = self.screen();
                let Screen::Choose(place) = screen else { unreachable!("only a place's folder is chosen in") };
                let folder = self.state.folder(screen);
                let entry = folder.entry(i).clone();
                let path = folder.inside(&entry.name);
                match entry.kind {
                    FileKind::Folder { .. } | FileKind::Link { folder: true, .. } => {
                        let scroll = self.top().1;
                        self.state.folder_mut(screen).leave(scroll);
                        return vec![Effect::ReadPlace(place, path)];
                    }
                    FileKind::File | FileKind::Link { folder: false, .. } => return self.install_file(place, path, entry.size, now),
                    FileKind::Special => unreachable!("a place lists no special files"),
                }
            }
            Action::OpenAncestor(depth) => {
                let screen = self.screen();
                if let Some(path) = self.state.folder(screen).ancestor(depth) {
                    return vec![match screen {
                        Screen::Choose(place) => Effect::ReadPlace(place, path),
                        _ => Effect::ReadDir(path),
                    }];
                }
            }
            Action::JoinNetwork(i) => {
                let w = self.state.wlan.as_ref().expect("networks show on a phone whose WLAN is brought up");
                // A scan may have changed the list since the page was drawn.
                let Some(network) = w.networks.get(i).cloned() else { return Vec::new() };
                if w.joined.as_ref().is_some_and(|j| j.ssid == network.ssid) {
                    return self.act(Action::OpenJoined, now);
                }
                match network.security {
                    Security::Wep => self.say(l.pick(texts::toast::WEP_UNSUPPORTED), now),
                    Security::Enterprise => self.say(l.pick(texts::toast::ENTERPRISE_UNSUPPORTED), now),
                    s if s.password() => self.ask_password(network),
                    _ => {
                        return vec![Effect::JoinWlan {
                            ssid: network.ssid,
                            name: network.name,
                            security: network.security,
                            password: Secret::default(),
                        }]
                    }
                }
            }
            Action::NetworkDetails(i) => {
                let w = self.state.wlan.as_ref().expect("networks show on a phone whose WLAN is brought up");
                let Some(network) = w.networks.get(i).cloned() else { return Vec::new() };
                self.state.details = Some(network);
                self.open(Screen::WlanNetwork);
            }
            Action::OpenJoined => {
                let w = self.state.wlan.as_ref().expect("a network is chosen on a phone whose WLAN is brought up");
                let Some(j) = w.joined.clone() else { return Vec::new() };
                // As found, where it still is; as it was chosen otherwise.
                let found = w.networks.iter().find(|n| n.ssid == j.ssid).cloned();
                let network = found.unwrap_or(Network {
                    ssid: j.ssid,
                    name: j.name,
                    signal: 0,
                    frequency: 0,
                    bands: Default::default(),
                    security: j.security,
                });
                if j.stage == Joining::WrongPassword {
                    self.ask_password(network);
                } else {
                    self.state.details = Some(network);
                    self.open(Screen::WlanNetwork);
                }
            }
            Action::LeaveNetwork => {
                let mut fx = self.back(now);
                fx.push(Effect::LeaveWlan);
                return fx;
            }
            Action::Rescan => return vec![Effect::ScanWlan],
            Action::AddNetwork => {
                self.state.adding = Some(Adding { name: String::new(), security: Security::Open, field: Field::Name, choosing: false });
                self.state.password.clear();
                self.state.show_password = false;
                self.state.layer = Layer::Letters;
                self.open(Screen::WlanAdd);
            }
            Action::TypeInto(field) => {
                let a = self.state.adding.as_mut().expect("the fields are on the page that adds a network");
                if a.field != field {
                    a.field = field;
                    self.refresh();
                }
            }
            Action::ChooseSecurity => {
                self.state.adding.as_mut().expect("the protection is on the page that adds a network").choosing = true;
                // The page draws the row held down while its list is open.
                self.row_press = None;
                self.row_art = None;
                self.refresh();
            }
            Action::PickSecurity(security) => {
                let a = self.state.adding.as_mut().expect("the protections open on the page that adds a network");
                a.security = security;
                a.choosing = false;
                // With no password to type, the keyboard types the name.
                if !security.password() {
                    a.field = Field::Name;
                }
                self.refresh();
            }
            Action::CloseChoices => {
                self.state.adding.as_mut().expect("the protections open on the page that adds a network").choosing = false;
                self.refresh();
            }
            // Asking takes a network, which the WLAN page joins.
            Action::CheckUpdate => {
                if !self.state.online() {
                    self.open(Screen::Wlan);
                    return self.arrive_at(Screen::Wlan);
                }
                if self.state.check != Check::Asking {
                    self.state.check = Check::Asking;
                    self.refresh();
                    return vec![Effect::CheckUpdate];
                }
            }
            // The package goes to internal storage, which the decrypt page
            // opens, over a network the WLAN page joins.
            Action::Download => {
                let release = self.state.newest().expect("the download is offered for the newest system").clone();
                if !self.state.data.storage_open() {
                    self.open(Screen::Decrypt);
                    return self.arrive_at(Screen::Decrypt);
                }
                if !self.state.online() {
                    self.open(Screen::Wlan);
                    return self.arrive_at(Screen::Wlan);
                }
                if !self.state.downloading() {
                    self.state.download = Some(DownloadJob { progress: None, rate: 0, outcome: None, stopping: false });
                    self.refresh();
                    return vec![Effect::Download(release)];
                }
            }
            Action::StopDownload => {
                let job = self.state.download.as_mut().expect("only a download under way is stopped");
                if job.outcome.is_none() && !job.stopping {
                    job.stopping = true;
                    self.refresh();
                    return vec![Effect::StopDownload];
                }
            }
            Action::InstallDownloaded => {
                let name = self.state.newest().expect("the package installed is the newest system's").name.clone();
                let Have::Whole(size) = self.state.have else { unreachable!("only a whole package is offered to install") };
                return self.install_file(Place::Storage, format!("{DOWNLOADS}/{name}"), size, now);
            }
            // Asking takes a network, which the WLAN page joins.
            Action::CheckAweyuki => {
                if !self.state.online() {
                    self.open(Screen::Wlan);
                    return self.arrive_at(Screen::Wlan);
                }
                if self.state.aweyuki_check != AweyukiCheck::Asking {
                    self.state.aweyuki_check = AweyukiCheck::Asking;
                    self.refresh();
                    return vec![Effect::CheckAweyuki];
                }
            }
            // The build goes into the recovery partitions an install may
            // write too, so one waits for the other.
            Action::UpdateAweyuki => {
                let build = self.state.aweyuki_newest().and_then(|n| n.build.clone()).expect("the update is offered for the newest release's build");
                if !self.state.online() {
                    self.open(Screen::Wlan);
                    return self.arrive_at(Screen::Wlan);
                }
                if !matches!(self.state.install, Install::Idle | Install::Finished { .. }) {
                    self.say(l.pick(INSTALL_UNFINISHED), now);
                    return Vec::new();
                }
                if !self.state.updating_aweyuki() {
                    self.state.aweyuki_job = Some(AweyukiJob { stage: None, rate: 0, outcome: None, stopping: false });
                    self.refresh();
                    return vec![Effect::UpdateAweyuki(build)];
                }
            }
            // Only the download stops: writing, once begun, goes on to its
            // end.
            Action::StopAweyukiUpdate => {
                let job = self.state.aweyuki_job.as_mut().expect("only an update under way is stopped");
                if job.outcome.is_none() && !job.stopping && !matches!(job.stage, Some(AweyukiStage::Write { .. })) {
                    job.stopping = true;
                    self.refresh();
                    return vec![Effect::StopAweyukiUpdate];
                }
            }
            Action::RefreshRoot => return vec![Effect::ReadRoot],
            Action::OpenRootModule(i) => {
                if let Some(m) = self.state.root.as_ref().and_then(|r| r.modules.as_ref().ok()).and_then(|mods| mods.get(i)) {
                    self.state.root_module = Some(m.id.clone());
                    self.open(Screen::RootModule);
                }
            }
            Action::RootToggleEnabled => {
                if let Some(m) = self.selected_root_module() {
                    return vec![Effect::RootSetEnabled { id: m.id.clone(), enabled: !m.enabled }];
                }
            }
            Action::RootToggleRemove => {
                if let Some(m) = self.selected_root_module() {
                    return vec![Effect::RootSetRemove { id: m.id.clone(), remove: !m.remove_pending }];
                }
            }
            Action::ShareRoot(root) => {
                if self.state.share.root == root {
                    return Vec::new();
                }
                if self.state.share.running && root == ShareRoot::Storage && !self.state.data.storage_open() {
                    self.say(l.pick(DECRYPT_FIRST), now);
                    return Vec::new();
                }
                self.state.share.root = root;
                self.refresh();
                if self.state.share.running {
                    return vec![Effect::Share { on: true, root, port: self.state.share.port }];
                }
            }
            Action::RefreshPlugins => return vec![Effect::ReadPlugins],
            Action::RunPlugin(i) => {
                if self.state.plugin_run.as_ref().is_some_and(|r| r.outcome.is_none()) {
                    self.say(l.pick(crate::texts::fork::s::M002), now);
                    return Vec::new();
                }
                if let Some(p) = self.state.plugins.as_ref().and_then(|list| list.get(i)) {
                    return vec![Effect::RunPlugin(p.id.clone())];
                }
            }
            Action::StopPlugin => {
                if let Some(run) = self.state.plugin_run.as_mut()
                    && run.outcome.is_none()
                    && !run.stopping
                {
                    run.stopping = true;
                    self.refresh();
                    return vec![Effect::StopPlugin];
                }
            }
            Action::LaunchAerap(i) => {
                let Some(entry) = self.state.aerap_plugins.as_ref().and_then(|v| v.get(i)).cloned() else {
                    return Vec::new();
                };
                match entry.kind {
                    aweyuki_install::aerap::AerapKind::Browser => {
                        self.state.browser = BrowserState {
                            starting: true,
                            url: "aera://start".into(),
                            url_input: "aera://start".into(),
                            ..BrowserState::default()
                        };
                        self.open(Screen::Browser);
                        return vec![Effect::StartBrowser(entry.path)];
                    }
                    aweyuki_install::aerap::AerapKind::Telegram => {
                        self.state.telegram = TelegramState { starting: true, ..TelegramState::default() };
                        self.open(Screen::Telegram);
                        return vec![Effect::StartTelegram(entry.path)];
                    }
                    aweyuki_install::aerap::AerapKind::Other => {
                        self.say(
                            l.pick(crate::texts::fork::s::M003),
                            now,
                        );
                    }
                }
            }
            Action::DownloadAerap(kind) => {
                if !self.state.online() {
                    self.open(Screen::Wlan);
                    return self.arrive_at(Screen::Wlan);
                }
                if !self.state.data.storage_open() {
                    self.open(Screen::Decrypt);
                    return self.arrive_at(Screen::Decrypt);
                }
                if self.state.aerap_download.is_some() {
                    self.say(l.pick(crate::texts::fork::s::M004), now);
                    return Vec::new();
                }
                let name = match kind {
                    aweyuki_install::aerap::AerapKind::Browser => "AERA Browser",
                    aweyuki_install::aerap::AerapKind::Telegram => "AERA Telegram",
                    aweyuki_install::aerap::AerapKind::Other => "AERA Plugin",
                };
                self.state.aerap_download = Some((name.to_string(), 0));
                self.refresh();
                return vec![Effect::DownloadAerap(kind)];
            }
            Action::InstallAerapEntry => {
                if !self.state.data.storage_open() {
                    self.say(l.pick(DECRYPT_FIRST), now);
                    return Vec::new();
                }
                let path = std::path::PathBuf::from(self.entry_path());
                let name = self.state.file.as_ref().map_or_else(|| "AERA".into(), |f| f.name.clone());
                self.state.aerap_download = Some((name, 0));
                let mut fx = self.back(now);
                fx.push(Effect::InstallAerap(path));
                return fx;
            }
            Action::BrowserBack => return vec![Effect::BrowserNav(BrowserNavOp::Back)],
            Action::BrowserForward => return vec![Effect::BrowserNav(BrowserNavOp::Forward)],
            Action::BrowserReload => return vec![Effect::BrowserNav(BrowserNavOp::Reload)],
            Action::BrowserEditUrl => {
                self.state.browser.editing_url = true;
                self.state.browser.show_keyboard = false;
                self.state.layer = Layer::Letters;
                self.refresh();
            }
            Action::BrowserHideKeyboard => {
                self.state.browser.editing_url = false;
                self.state.browser.show_keyboard = false;
                self.refresh();
            }
            Action::BrowserViewport => {}
            Action::BrowserClose => {
                self.state.browser.editing_url = false;
                self.state.browser.show_keyboard = false;
                return self.back(now);
            }
            Action::TgFocus(field) => {
                self.state.telegram.focus = field;
                self.refresh();
            }
            Action::TgSubmit => return self.tg_submit_effect(now),
            Action::TgRefreshChats => return vec![Effect::TelegramLoadChats],
            Action::TgOpenChat(i) => {
                if let Some(chat) = self.state.telegram.chats.get(i).cloned() {
                    self.state.telegram.open_chat = Some((chat.id, chat.title));
                    self.state.telegram.messages.clear();
                    self.state.telegram.focus = TgField::Compose;
                    self.refresh();
                    return vec![Effect::TelegramOpenChat(chat.id)];
                }
            }
            Action::TgBackToChats => {
                self.state.telegram.open_chat = None;
                self.state.telegram.messages.clear();
                self.state.telegram.compose.clear();
                self.refresh();
            }
            Action::TgDownload(file_id) => return vec![Effect::TelegramDownload(file_id)],
            Action::TgClose => {
                self.state.telegram.open_chat = None;
                return self.back(now);
            }
            Action::DismissAblWarning => {
                self.state.abl_warning = None;
                self.refresh();
            }
            Action::InspectPayload => {
                let path = std::path::PathBuf::from(self.entry_path());
                self.state.payload = Some(PayloadState {
                    path: path.clone(),
                    report: None,
                    extracting: None,
                    last_extracted: None,
                });
                self.open(Screen::Payload);
                return vec![Effect::InspectPayload(path)];
            }
            Action::ExtractPayloadPart(i) => {
                let Some(st) = self.state.payload.as_ref() else {
                    return Vec::new();
                };
                if st.extracting.is_some() {
                    self.say(l.pick(crate::texts::fork::s::M005), now);
                    return Vec::new();
                }
                let Some(Ok(report)) = &st.report else {
                    return Vec::new();
                };
                let Some(part) = report.partitions.get(i) else {
                    return Vec::new();
                };
                if !part.extractable {
                    self.say(l.pick(crate::texts::fork::s::M006), now);
                    return Vec::new();
                }
                let path = st.path.clone();
                let partition = part.name.clone();
                let size = part.size;
                if let Some(mut_st) = self.state.payload.as_mut() {
                    mut_st.extracting = Some((partition.clone(), 0, size));
                }
                self.refresh();
                return vec![Effect::ExtractPayloadPart { path, partition }];
            }
            Action::BatchPlace(k) => {
                if let Some(&src) = BatchSource::ALL.get(k)
                    && self.state.batch.job.is_none()
                {
                    self.state.batch.source = src;
                    self.state.batch.items = None;
                    self.refresh();
                    if self.state.batch_source_open() {
                        return vec![Effect::ReadBatch(src)];
                    }
                }
            }
            Action::RefreshBatch => {
                if self.state.batch.job.as_ref().is_some_and(|j| j.outcome.is_some()) {
                    self.state.batch.job = None;
                }
                if self.state.batch.job.is_none() {
                    if self.state.batch_source_open() {
                        self.state.batch.items = None;
                        self.refresh();
                        return vec![Effect::ReadBatch(self.state.batch.source)];
                    }
                    self.refresh();
                }
            }
            Action::BatchToggle(i) => {
                if self.state.batch.job.is_none()
                    && let Some(Ok(items)) = self.state.batch.items.as_mut()
                    && let Some((item, on)) = items.get_mut(i)
                    && matches!(item.geometry, aweyuki_install::batch::GeometryCheck::Ok { .. })
                {
                    *on = !*on;
                    self.refresh();
                }
            }
            Action::BatchSlot(pick) => {
                if self.state.batch.job.is_none() {
                    self.state.batch.pick = pick;
                    self.refresh();
                }
            }
            Action::LockNow => self.lock_now(),
            Action::LockStartSet => {
                self.state.lock_input.clear();
                self.state.lock_setup = LockSetupState {
                    stage: LockSetupStage::EnterNew,
                    first_pin: String::new(),
                    removing: false,
                };
                self.refresh();
            }
            Action::LockStartChange => {
                self.state.lock_input.clear();
                self.state.lock_setup = LockSetupState {
                    stage: LockSetupStage::VerifyCurrent,
                    first_pin: String::new(),
                    removing: false,
                };
                self.refresh();
            }
            Action::LockStartRemove => {
                self.state.lock_input.clear();
                self.state.lock_setup = LockSetupState {
                    stage: LockSetupStage::VerifyCurrent,
                    first_pin: String::new(),
                    removing: true,
                };
                self.refresh();
            }
            Action::RefreshDev => {
                if self.state.dev.bench.as_ref().is_some_and(|j| j.outcome.is_some()) {
                    self.state.dev.bench = None;
                }
                if self.state.dev.bench.is_none() {
                    self.refresh();
                    return vec![Effect::ReadDev];
                }
            }
            Action::SetPrintk(level) => {
                if self.state.dev.bench.is_none() {
                    return vec![Effect::SetPrintk(level)];
                }
            }
            Action::ToggleAdb => {
                if self.state.dev.bench.is_none() {
                    let adb_on = self
                        .state
                        .dev
                        .info
                        .as_ref()
                        .is_some_and(|d| d.adb_config.split(',').any(|tok| tok == "adb"));
                    return vec![Effect::SetAdb(!adb_on)];
                }
            }
            Action::RunBench(target) => {
                if self.state.dev.bench.as_ref().is_some_and(|j| j.outcome.is_none()) {
                    return Vec::new();
                }
                let ready = match target {
                    BenchTarget::Storage => {
                        if !self.state.data.storage_open() {
                            self.say(l.pick(DECRYPT_FIRST), now);
                            false
                        } else {
                            true
                        }
                    }
                    BenchTarget::UsbDrive => {
                        if self.state.usb_drive != Some(UsbDrive::Shown) {
                            self.say(l.pick(USB_DRIVE_WANTED), now);
                            false
                        } else {
                            true
                        }
                    }
                };
                if ready {
                    self.state.dev.bench = Some(BenchJob {
                        target,
                        done: 0,
                        total: 100,
                        outcome: None,
                    });
                    self.refresh();
                    return vec![Effect::RunBench(target)];
                }
            }
            Action::VaultTab(tab) => {
                if self.state.vault.job.is_none() && self.state.vault.tab != tab {
                    self.state.vault.tab = tab;
                    self.refresh();
                }
            }
            Action::RefreshVault => {
                if self.state.vault.job.as_ref().is_some_and(|j| j.outcome.is_some()) {
                    self.state.vault.job = None;
                }
                if self.state.vault.job.is_none() {
                    if self.state.data.storage_open() {
                        self.state.vault.apps = None;
                        self.state.vault.saved = None;
                        self.refresh();
                        return vec![Effect::ReadVault];
                    }
                    self.refresh();
                }
            }
            Action::VaultBackupApp(i) => {
                if self.state.vault.job.is_some() {
                    return Vec::new();
                }
                if !self.state.data.storage_open() {
                    self.say(l.pick(DECRYPT_FIRST), now);
                    return Vec::new();
                }
                if let Some(Ok(apps)) = &self.state.vault.apps
                    && let Some(app) = apps.get(i)
                {
                    let package = app.package.clone();
                    let total = app.apk_bytes.saturating_add(app.data_bytes).max(1);
                    self.state.vault.job = Some(VaultJob {
                        label: package.clone(),
                        restore: false,
                        done: 0,
                        total,
                        outcome: None,
                    });
                    self.refresh();
                    return vec![Effect::VaultBackup(package)];
                }
            }
            Action::VaultRestoreSaved(i) => {
                if self.state.vault.job.is_some() {
                    return Vec::new();
                }
                if !self.state.data.storage_open() {
                    self.say(l.pick(DECRYPT_FIRST), now);
                    return Vec::new();
                }
                if let Some(Ok(saved)) = &self.state.vault.saved
                    && let Some(backup) = saved.get(i)
                {
                    let archive = backup.archive_file.clone();
                    self.state.vault.job = Some(VaultJob {
                        label: backup.package.clone(),
                        restore: true,
                        done: 0,
                        total: backup.archive_bytes.max(1),
                        outcome: None,
                    });
                    self.refresh();
                    return vec![Effect::VaultRestore(archive)];
                }
            }
            Action::VaultDeleteSaved(i) => {
                if self.state.vault.job.is_some() {
                    return Vec::new();
                }
                if !self.state.data.storage_open() {
                    self.say(l.pick(DECRYPT_FIRST), now);
                    return Vec::new();
                }
                if let Some(Ok(saved)) = &self.state.vault.saved
                    && let Some(backup) = saved.get(i)
                {
                    return vec![Effect::VaultDelete(backup.archive_file.clone())];
                }
            }
            Action::Unavailable(text) => self.say(text, now),
        }
        Vec::new()
    }

    /// The text the keyboard types into on this page.
    fn typing(&mut self) -> &mut String {
        match self.screen() {
            Screen::Search => &mut self.state.search,
            Screen::Decrypt => &mut self.state.code,
            Screen::Lock | Screen::LockSetup => &mut self.state.lock_input,
            Screen::Rename => &mut self.state.new_name,
            Screen::WlanPassword => &mut self.state.password,
            Screen::Browser => &mut self.state.browser.url_input,
            Screen::Telegram => self.tg_active_text(),
            Screen::WlanAdd => {
                let a = self.state.adding.as_mut().expect("the page adds a network");
                match a.field {
                    Field::Name => &mut a.name,
                    Field::Password => &mut self.state.password,
                }
            }
            s => panic!("{s:?} has no text field"),
        }
    }

    /// A key of the on-screen keyboard. A capital lasts one letter; return
    /// opens the first result of a search, and runs a terminal's command.
    fn type_key(&mut self, cap: Cap, now: u64) -> Vec<Effect> {
        let (l, layer) = (self.state.lang, self.state.layer);
        if self.screen() == Screen::Browser && !self.state.browser.editing_url {
            match cap {
                Cap::Char(ch) => {
                    let shifted = self.state.layer == Layer::Shifted;
                    let out = if shifted { ch.to_ascii_uppercase() } else { ch };
                    if shifted {
                        self.state.layer = Layer::Letters;
                        self.refresh();
                    }
                    return vec![Effect::BrowserKey { code: 0, text: out.to_string() }];
                }
                Cap::Space => return vec![Effect::BrowserKey { code: 0, text: " ".into() }],
                Cap::Backspace => return vec![Effect::BrowserKey { code: 8, text: String::new() }],
                Cap::Enter => return vec![Effect::BrowserKey { code: 13, text: String::new() }],
                Cap::Shift => {
                    self.state.layer = if self.state.layer == Layer::Shifted { Layer::Letters } else { Layer::Shifted };
                    self.refresh();
                    return Vec::new();
                }
                Cap::Show(next_layer) => {
                    self.state.layer = next_layer;
                    self.refresh();
                    return Vec::new();
                }
            }
        }
        match cap {
            Cap::Char(ch) => {
                let shifted = self.state.layer == Layer::Shifted;
                self.typing().push(if shifted { ch.to_ascii_uppercase() } else { ch });
                self.edits += 1;
                if shifted {
                    self.state.layer = Layer::Letters;
                }
            }
            Cap::Space => {
                self.typing().push(' ');
                self.edits += 1;
            }
            Cap::Backspace => {
                self.typing().pop();
                self.edits += 1;
            }
            Cap::Shift => {
                self.state.layer = if self.state.layer == Layer::Shifted { Layer::Letters } else { Layer::Shifted };
            }
            Cap::Show(layer) => self.state.layer = layer,
            Cap::Enter => match self.screen() {
                Screen::Search => {
                    if let Some(first) = search::find_on(&self.state.search, l, |screen| self.state.offers(screen)).first() {
                        return self.act(Action::Open(first.screen), now);
                    }
                }
                Screen::Decrypt if self.state.code.is_empty() => self.say(l.pick(texts::toast::ENTER_LOCK), now),
                Screen::Decrypt => {
                    let code = std::mem::take(&mut self.state.code);
                    return self.unlock(code);
                }
                Screen::Lock => return self.submit_lock_pin(now),
                Screen::LockSetup => {
                    self.submit_lock_setup(now);
                    return Vec::new();
                }
                // A name the folder cannot take is turned down here, in
                // the user's words; an unchanged one leaves the page.
                Screen::Rename => {
                    let name = self.state.new_name.clone();
                    let current = &self.state.file.as_ref().expect("the file page shows an entry").name;
                    let taken = self.state.files.entries.as_ref().is_ok_and(|entries| entries.iter().any(|e| e.name == name));
                    match name.as_str() {
                        "" => self.say(l.pick(texts::toast::ENTER_NAME), now),
                        "." | ".." => self.say(l.pick(texts::toast::NAME_DOTS), now),
                        _ if name.contains('/') => self.say(l.pick(texts::toast::NAME_SLASH), now),
                        _ if name == *current => return self.back(now),
                        _ if taken => self.say_taken(now),
                        _ => return vec![Effect::Rename { path: self.entry_path(), name }],
                    }
                }
                // A password the network's protection cannot take is turned
                // down here; one it can goes, and the WLAN page shows how
                // joining goes.
                Screen::WlanPassword => {
                    let network = self.state.joining.clone().expect("the password page is for a network");
                    match refused(l, network.security, &self.state.password) {
                        Some(why) => self.say(why, now),
                        None => {
                            let password = Secret(std::mem::take(&mut self.state.password));
                            let mut fx = self.back(now);
                            fx.push(Effect::JoinWlan { ssid: network.ssid, name: network.name, security: network.security, password });
                            return fx;
                        }
                    }
                }
                // So is a name no network can have; the keyboard then types
                // into the field turned down.
                Screen::WlanAdd => {
                    let a = self.state.adding.as_mut().expect("the page adds a network");
                    let (field, why) = match a.name.len() {
                        0 => (Field::Name, Some(l.pick(texts::toast::ENTER_NETWORK_NAME).to_string())),
                        n if n > SSID_MAX => (Field::Name, Some(texts::toast::network_name_long(l, SSID_MAX))),
                        _ if a.security.password() => (Field::Password, refused(l, a.security, &self.state.password).map(str::to_string)),
                        _ => (Field::Name, None),
                    };
                    match why {
                        Some(why) => {
                            if a.field != field {
                                a.field = field;
                                self.refresh();
                            }
                            self.say(&why, now)
                        }
                        None => {
                            let (name, security) = (a.name.clone(), a.security);
                            let password = match security.password() {
                                true => Secret(std::mem::take(&mut self.state.password)),
                                false => Secret::default(),
                            };
                            let mut fx = self.back(now);
                            fx.push(Effect::JoinWlan { ssid: name.clone().into_bytes(), name, security, password });
                            return fx;
                        }
                    }
                }
                Screen::Browser => {
                    match aweyuki_install::aerap::browser_proto::normalize_url(&self.state.browser.url_input) {
                        Some(url) => {
                            self.state.browser.url = url.clone();
                            self.state.browser.url_input = url.clone();
                            self.state.browser.editing_url = false;
                            self.state.browser.show_keyboard = false;
                            self.refresh();
                            return vec![Effect::BrowserOpen(url)];
                        }
                        None => {
                            self.say(
                                l.pick(crate::texts::fork::s::M007),
                                now,
                            );
                            return Vec::new();
                        }
                    }
                }
                Screen::Telegram => return self.tg_submit_effect(now),
                other => unreachable!("{other:?} shows no keyboard"),
            },
        }
        // What is typed shows above the keyboard, which stays as it was
        // while its keys do.
        if !matches!(self.screen(), Screen::Browser | Screen::Telegram)
            && matches!(cap, Cap::Char(_) | Cap::Space | Cap::Backspace)
            && self.state.layer == layer
        {
            let t = self.theme();
            let panel_h = if matches!(self.screen(), Screen::Lock | Screen::LockSetup) {
                widgets::keypad_height(&t)
            } else {
                widgets::keyboard_height(&t)
            };
            self.refresh_rows(0, (t.m.h as f32 - panel_h).floor() as u32);
        } else {
            self.refresh();
        }
        Vec::new()
    }

    /// What a swipe-to-confirm control slid to its end does.
    pub(super) fn confirmed(&mut self, which: Confirm, now: u64) -> Vec<Effect> {
        let (l, mut fx) = (self.state.lang, Vec::new());
        match which {
            Confirm::Reboot => fx.push(Effect::Reboot(TARGETS[self.state.reboot])),
            Confirm::RebootAnyway => {
                if let Some((warn, target)) = self.state.abl_warning.take() {
                    fx.push(Effect::RebootConfirmed {
                        target,
                        fingerprint: warn.fingerprint,
                    });
                    self.refresh();
                }
            }
            Confirm::Wipe => {
                let chosen: Vec<Wipe> = WIPES.iter().zip(self.state.wipe).filter(|(_, on)| *on).map(|(&w, _)| w).collect();
                match chosen.is_empty() {
                    true => self.say(l.pick(texts::toast::CHOOSE_WIPE), now),
                    false => fx.push(Effect::Wipe(chosen)),
                }
            }
            Confirm::Install | Confirm::InstallWipe => {
                if let Install::Confirming { version, .. } = &self.state.install {
                    self.state.install = Install::Updating { version: version.clone(), stage: None, overwriting: false };
                    fx.push(Effect::Install);
                    self.refresh();
                }
            }
            Confirm::RunScript => {
                if self.state.install == Install::ConfirmingScript {
                    self.state.install = Install::Scripting { stage: None, running: false };
                    fx.push(Effect::Install);
                    self.refresh();
                }
            }
            Confirm::Backup => {
                let s = &self.state;
                let mut parts = s.backup;
                parts[5] &= s.efisp_partition;
                let refused = if !parts.contains(&true) {
                    Some(texts::toast::CHOOSE_BACKUP)
                } else if (parts[DATA_PART] || parts[MEDIA_PART]) && s.data != Data::Open {
                    Some(texts::toast::DECRYPT_TO_BACK_UP)
                } else if s.place_open(s.backup_to) {
                    None
                } else {
                    Some(match (s.backup_to, &s.usb_drive) {
                        (Place::Storage, _) if s.data == Data::Erased => DATA_ERASED,
                        (Place::Storage, _) => DECRYPT_FIRST,
                        (Place::UsbDrive, Some(UsbDrive::Unreadable(_))) => USB_DRIVE_UNREADABLE,
                        (Place::UsbDrive, _) => USB_DRIVE_WANTED,
                    })
                };
                let backup = Effect::Backup { parts, place: s.backup_to, compress: s.compress, verify: s.verify };
                match refused {
                    Some(why) => self.say(l.pick(why), now),
                    None => fx.push(backup),
                }
            }
            Confirm::Restore => {
                let (entry, parts) = self.state.restore.as_ref().expect("the restore page shows a backup");
                let restore = Effect::Restore { place: entry.place, name: entry.name.clone(), parts: *parts };
                // The files of the data partition and of internal storage go
                // back into a data partition that is open: one erased is
                // made again by the system, as it next starts.
                let files = parts[DATA_PART] || parts[MEDIA_PART];
                if !parts.contains(&true) {
                    self.say(l.pick(texts::toast::CHOOSE_RESTORE), now);
                } else if files && self.state.data == Data::Erased {
                    self.say(l.pick(texts::toast::RESTORE_ONCE_MADE), now);
                } else if files && self.state.data != Data::Open {
                    self.say(l.pick(texts::toast::DECRYPT_TO_RESTORE), now);
                } else {
                    fx.push(restore);
                }
            }
            Confirm::DeleteBackup => {
                let (entry, _) = self.state.restore.as_ref().expect("the restore page shows a backup");
                fx.push(Effect::DeleteBackup { place: entry.place, name: entry.name.clone() });
            }
            Confirm::Slot => match (&self.state.boot_slots, self.state.next_slot) {
                (Some(Ok(slots)), Some(pick)) if pick != slots.active => fx.push(Effect::SetSlot(pick)),
                (Some(Ok(slots)), _) => self.say(&texts::toast::slot_already_next(l, slots.active.letter()), now),
                _ => self.say(l.pick(texts::slot::UNREADABLE), now),
            },
            Confirm::FactoryReset => fx.push(Effect::FactoryReset),
            Confirm::Merge => {
                if self.state.update == Standing::Waiting {
                    self.state.update = Standing::Merging(0.0);
                    self.refresh();
                    fx.push(Effect::Merge);
                }
            }
            Confirm::DeleteEntry => fx.push(Effect::Delete(self.entry_path())),
            // The partition has to be open; whether its users have unlocked
            // their own files makes no difference.
            Confirm::Relabel => match self.state.data {
                Data::Open | Data::Locked { .. } => fx.push(Effect::Relabel),
                Data::Opening => self.say(l.pick(DATA_OPENING), now),
                Data::Erased => self.say(l.pick(DATA_ERASED), now),
                Data::Closed | Data::Failed(_) => self.say(l.pick(texts::toast::OPEN_DATA_FIRST), now),
            },
            // The data partition is reached through its own key whether or
            // not it is open, but not while it is being opened or erased.
            Confirm::Filesystem => {
                let row = self.fs_row().expect("the page of a partition shows one that was read");
                let data = row.name == DATA_PARTITION;
                match (&self.state.fs_action, &self.state.data) {
                    (_, Data::Opening) if data => self.say(l.pick(DATA_OPENING), now),
                    (FsAction::Repair, Data::Erased) if data => self.say(l.pick(DATA_ERASED), now),
                    (action, _) => fx.push(Effect::Filesystem { name: row.name.clone(), action: action.clone() }),
                }
            }
            Confirm::Flash => {
                if let Install::Choosing(choice) = &self.state.install {
                    let FlashTarget { name, place } = choice.partitions[choice.partition].clone();
                    fx.push(Effect::Flash { partition: name.clone(), place, pick: choice.pick });
                    self.state.install = Install::Flashing { partition: name, stage: None };
                    self.refresh();
                }
            }
            Confirm::SwitchSystem => {
                if let (Some(Ok(systems)), Some(pick)) = (&self.state.systems, self.state.system_pick)
                    && pick != starting(systems)
                {
                    fx.push(Effect::SwitchSystem(pick));
                    self.state.systems_work = Some(SystemsWork::Switching(pick));
                    self.state.system_pick = None;
                    self.refresh();
                }
            }
            Confirm::RemoveSystem => {
                fx.push(Effect::RemoveSystem);
                self.state.systems_work = Some(SystemsWork::Removing);
                self.state.asking_delete = false;
                self.state.system_pick = None;
                self.refresh();
            }
            Confirm::InstallSystem => {
                if let Install::SystemChoosing(choice) = &self.state.install {
                    fx.push(Effect::InstallSystem { size: choice.sizes[choice.size] });
                    self.state.install = Install::Systeming { name: choice.name.clone(), stage: None };
                    self.refresh();
                }
            }
            Confirm::BatchFlash => {
                let selected = self.state.batch.selected_payload();
                if selected.is_empty() {
                    self.say(l.pick(crate::texts::fork::s::M008), now);
                } else {
                    let pick = self.state.batch.pick;
                    self.state.batch.job = Some(BatchJob {
                        total: selected.len(),
                        index: 0,
                        partition: selected[0].1.clone(),
                        done: 0,
                        of: 0,
                        outcome: None,
                    });
                    self.refresh();
                    fx.push(Effect::RunBatch { items: selected, pick });
                }
            }
        }
        fx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn search_finds_by_pinyin_typed_on_its_own_keyboard() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Search, &f, &i);
        app.state.lang = Lang::Hans;
        type_text(&mut app, &f, &i, "bf");
        assert_eq!(app.state.search, "bf");
        let opens: Vec<Action> = app.frame(&f, &i).hits.iter().map(|h| h.action).filter(|a| matches!(a, Action::Open(_))).collect();
        assert_eq!(opens, [Action::Open(Screen::Backup)]);
        tap(&mut app, &f, &i, Action::Type(Cap::Backspace));
        assert_eq!(app.state.search, "b");
        tap(&mut app, &f, &i, Action::ClearSearch);
        assert_eq!(app.state.search, "");
        type_text(&mut app, &f, &i, "xpsj");
        tap(&mut app, &f, &i, Action::Type(Cap::Enter));
        assert_eq!(app.screen(), Screen::ScreenOff, "return opens the first result");
        app.key(Key::Back, 0);
        assert_eq!(app.screen(), Screen::Search);
    }

    #[test]
    fn shift_makes_one_capital_and_the_layers_switch() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Search, &f, &i);
        tap(&mut app, &f, &i, Action::Type(Cap::Shift));
        type_text(&mut app, &f, &i, "ab");
        assert_eq!(app.state.search, "Ab");
        tap(&mut app, &f, &i, Action::Type(Cap::Show(Layer::Digits)));
        type_text(&mut app, &f, &i, "1");
        tap(&mut app, &f, &i, Action::Type(Cap::Show(Layer::Symbols)));
        type_text(&mut app, &f, &i, "|");
        tap(&mut app, &f, &i, Action::Type(Cap::Space));
        assert_eq!(app.state.search, "Ab1| ");
        tap(&mut app, &f, &i, Action::Type(Cap::Show(Layer::Letters)));
        assert_eq!(app.state.layer, Layer::Letters);
    }

    #[test]
    fn a_key_shows_held_down_while_touched() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Search, &f, &i);
        let (x, y) = spot(&mut app, &f, &i, Action::Type(Cap::Char('q')));
        app.touch(Touch::Down(x, y), &f, 0);
        assert_eq!(app.state.key_held(), Some(Cap::Char('q')));
        app.touch(Touch::Up, &f, 0);
        assert_eq!(app.state.held, None);
        assert_eq!(app.state.search, "q");
    }
}
