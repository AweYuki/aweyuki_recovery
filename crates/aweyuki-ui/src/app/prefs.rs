//! The settings page and the pages under it: its sliders and switches,
//! the language, the time zone, and how long the screen stays on.

use super::*;

/// The settings' sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Brightness,
    /// How strongly the motor taps.
    Vibration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Vibrate,
    /// Offer internal storage to a computer over USB.
    FileTransfer,
    /// Compress a backup.
    Compress,
    /// Read a backup back once it is written.
    Verify,
    /// Run a zip's install script only if the zip was signed with one of
    /// the phone's update keys.
    Signature,
    /// Write Aweyuki back into the recovery partitions an install writes
    /// over.
    KeepRecovery,
    Wlan,
    /// HTTP/1.1 file-sharing server.
    Share,
    /// Show the password typed rather than a dot for each character.
    ShowPassword,
}

/// How long the screen stays on with nothing touched, in milliseconds, in
/// the order the page lists them; `None` keeps it on.
pub const SCREEN_OFF: [Option<u64>; 7] = [Some(15_000), Some(30_000), Some(60_000), Some(120_000), Some(300_000), Some(600_000), None];

/// The time zones the clock and the times on the pages can keep, in the
/// order their page lists them: the name, the cities it covers, and the
/// zone's name in the tz database.
pub const TIME_ZONES: [(Text, Text, &str); 28] = [
    (["中国标准时间", "中國標準時間", "China Standard Time"], ["UTC+8 · 北京、上海", "UTC+8 · 北京、上海", "UTC+8 · Beijing, Shanghai"], "Asia/Shanghai"),
    (["香港时间", "香港時間", "Hong Kong Time"], ["UTC+8 · 香港", "UTC+8 · 香港", "UTC+8 · Hong Kong"], "Asia/Hong_Kong"),
    (["越南时间", "越南時間", "Vietnam Time"], ["UTC+7 · 河内、胡志明市", "UTC+7 · 河內、胡志明市", "UTC+7 · Hanoi, Ho Chi Minh City"], "Asia/Ho_Chi_Minh"),
    (["台北时间", "台北時間", "Taipei Time"], ["UTC+8 · 台北", "UTC+8 · 台北", "UTC+8 · Taipei"], "Asia/Taipei"),
    (["新加坡时间", "新加坡時間", "Singapore Time"], ["UTC+8 · 新加坡、吉隆坡", "UTC+8 · 新加坡、吉隆坡", "UTC+8 · Singapore, Kuala Lumpur"], "Asia/Singapore"),
    (["菲律宾时间", "菲律賓時間", "Philippine Time"], ["UTC+8 · 马尼拉", "UTC+8 · 馬尼拉", "UTC+8 · Manila"], "Asia/Manila"),
    (["中南半岛时间", "中南半島時間", "Indochina Time"], ["UTC+7 · 曼谷、金边", "UTC+7 · 曼谷、金邊", "UTC+7 · Bangkok, Phnom Penh"], "Asia/Bangkok"),
    (["印度尼西亚西部时间", "印尼西部時間", "Western Indonesia Time"], ["UTC+7 · 雅加达", "UTC+7 · 雅加達", "UTC+7 · Jakarta"], "Asia/Jakarta"),
    (["韩国标准时间", "韓國標準時間", "Korea Standard Time"], ["UTC+9 · 首尔", "UTC+9 · 首爾", "UTC+9 · Seoul"], "Asia/Seoul"),
    (["日本标准时间", "日本標準時間", "Japan Standard Time"], ["UTC+9 · 东京", "UTC+9 · 東京", "UTC+9 · Tokyo"], "Asia/Tokyo"),
    (["澳大利亚东部时间", "澳洲東部時間", "Australian Eastern Time"], ["UTC+10/+11 · 悉尼、墨尔本", "UTC+10/+11 · 雪梨、墨爾本", "UTC+10/+11 · Sydney, Melbourne"], "Australia/Sydney"),
    (["新西兰时间", "紐西蘭時間", "New Zealand Time"], ["UTC+12/+13 · 奥克兰", "UTC+12/+13 · 奧克蘭", "UTC+12/+13 · Auckland"], "Pacific/Auckland"),
    (["印度标准时间", "印度標準時間", "India Standard Time"], ["UTC+5:30 · 新德里、孟买", "UTC+5:30 · 新德里、孟買", "UTC+5:30 · New Delhi, Mumbai"], "Asia/Kolkata"),
    (["巴基斯坦标准时间", "巴基斯坦標準時間", "Pakistan Standard Time"], ["UTC+5 · 卡拉奇、伊斯兰堡", "UTC+5 · 卡拉奇、伊斯蘭瑪巴德", "UTC+5 · Karachi, Islamabad"], "Asia/Karachi"),
    (["海湾标准时间", "波斯灣標準時間", "Gulf Standard Time"], ["UTC+4 · 迪拜、阿布扎比", "UTC+4 · 杜拜、阿布達比", "UTC+4 · Dubai, Abu Dhabi"], "Asia/Dubai"),
    (["莫斯科时间", "莫斯科時間", "Moscow Time"], ["UTC+3 · 莫斯科", "UTC+3 · 莫斯科", "UTC+3 · Moscow"], "Europe/Moscow"),
    (["阿拉伯标准时间", "阿拉伯標準時間", "Arabia Standard Time"], ["UTC+3 · 利雅得", "UTC+3 · 利雅德", "UTC+3 · Riyadh"], "Asia/Riyadh"),
    (["东欧时间", "東歐時間", "Eastern European Time"], ["UTC+2/+3 · 雅典、基辅", "UTC+2/+3 · 雅典、基輔", "UTC+2/+3 · Athens, Kyiv"], "Europe/Athens"),
    (["中欧时间", "中歐時間", "Central European Time"], ["UTC+1/+2 · 柏林、巴黎", "UTC+1/+2 · 柏林、巴黎", "UTC+1/+2 · Berlin, Paris"], "Europe/Berlin"),
    (["英国时间", "英國時間", "British Time"], ["UTC+0/+1 · 伦敦", "UTC+0/+1 · 倫敦", "UTC+0/+1 · London"], "Europe/London"),
    (["协调世界时", "世界標準時間", "Coordinated Universal Time"], ["UTC+0 · 国际标准时间", "UTC+0 · 世界標準", "UTC+0 · Universal"], "UTC"),
    (["巴西利亚时间", "巴西利亞時間", "Brasilia Time"], ["UTC-3 · 圣保罗", "UTC-3 · 聖保羅", "UTC-3 · Sao Paulo"], "America/Sao_Paulo"),
    (["美国东部时间", "美國東部時間", "Eastern Time"], ["UTC-5/-4 · 纽约、多伦多", "UTC-5/-4 · 紐約、多倫多", "UTC-5/-4 · New York, Toronto"], "America/New_York"),
    (["美国中部时间", "美國中部時間", "Central Time"], ["UTC-6/-5 · 芝加哥、墨西哥城", "UTC-6/-5 · 芝加哥、墨西哥城", "UTC-6/-5 · Chicago, Mexico City"], "America/Chicago"),
    (["美国山地时间", "美國山區時間", "Mountain Time"], ["UTC-7/-6 · 丹佛", "UTC-7/-6 · 丹佛", "UTC-7/-6 · Denver"], "America/Denver"),
    (["美国太平洋时间", "美國太平洋時間", "Pacific Time"], ["UTC-8/-7 · 洛杉矶、温哥华", "UTC-8/-7 · 洛杉磯、溫哥華", "UTC-8/-7 · Los Angeles, Vancouver"], "America/Los_Angeles"),
    (["阿拉斯加时间", "阿拉斯加時間", "Alaska Time"], ["UTC-9/-8 · 安克雷奇", "UTC-9/-8 · 安克拉治", "UTC-9/-8 · Anchorage"], "America/Anchorage"),
    (["夏威夷时间", "夏威夷時間", "Hawaii Time"], ["UTC-10 · 檀香山", "UTC-10 · 檀香山", "UTC-10 · Honolulu"], "Pacific/Honolulu"),
];

impl App {
    pub(super) fn set_slider(&mut self, t: &Theme, which: Slider, track: Rect, x: f32) {
        self.set_level(t, which, track, (x - track.x) / track.w);
    }

    /// Where a slider stands, as a share of its span.
    pub(super) fn level(&self, which: Slider) -> f32 {
        match which {
            Slider::Brightness => self.state.brightness,
            Slider::Vibration => self.state.vibration,
        }
    }

    /// Set a slider to `level` of its span, the brightness no lower than
    /// its floor, and draw the slider at `track` on the page again.
    pub(super) fn set_level(&mut self, t: &Theme, which: Slider, track: Rect, level: f32) {
        let value = match which {
            Slider::Brightness => &mut self.state.brightness,
            Slider::Vibration => &mut self.state.vibration,
        };
        *value = level.clamp(if which == Slider::Brightness { MIN_BRIGHTNESS } else { 0.0 }, 1.0);
        let value = *value;
        if let Some(fr) = self.frame.as_mut() {
            fr.canvas.fill_rect(track.x, track.y, track.w, track.h, t.pal.card);
            widgets::slider(&mut fr.canvas, t, track.x, track.y, track.w, value);
        }
        self.layer = None;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::*;

    #[test]
    fn the_file_transfer_switch_has_the_runtime_follow_it() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Settings, W, H, PPD);
        assert!(app.state.file_transfer, "on until turned off");
        let (_, y) = spot(&mut app, &f, &i, Action::Toggle(Switch::FileTransfer));
        if y > H as f32 * 0.9 {
            scroll_to(&mut app, &f, y - H as f32 * 0.8, 0);
        }
        assert_eq!(tap_at(&mut app, &f, &i, Action::Toggle(Switch::FileTransfer), 2000), [Effect::FileTransfer(false)]);
        assert!(!app.state.file_transfer);
        assert_eq!(tap_at(&mut app, &f, &i, Action::Toggle(Switch::FileTransfer), 3000), [Effect::FileTransfer(true)]);
        app.file_transfer_failed(false, "无法开启文件传输", 4000);
        assert!(!app.state.file_transfer, "the switch shows how it stands");
    }

    #[test]
    fn theme_previews_switch_the_theme() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Settings, W, H, PPD);
        tap(&mut app, &f, &i, Action::Theme { dark: true });
        assert!(app.state.dark);
        assert!(app.theme().dark);
        tap(&mut app, &f, &i, Action::Theme { dark: false });
        assert!(!app.state.dark);
    }

    #[test]
    fn dragging_the_track_sets_brightness() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Settings, W, H, PPD);
        let track = app.frame(&f, &i).hits.iter().find(|h| h.action == Action::Slider(Slider::Brightness)).unwrap().rect;
        let y = track.centre().1 - app.top().1;
        assert!(app.touch(Touch::Down(track.x + track.w * 0.25, y), &f, 0).is_empty());
        assert_eq!(app.panel(0), Some(0.25));
        assert!(app.touch(Touch::Move(track.x + track.w * 0.75, y + 40.0), &f, 0).is_empty());
        assert_eq!(app.panel(0), Some(0.75));
        assert!(app.touch(Touch::Up, &f, 0).is_empty(), "the panel shows the brightness");
        assert_eq!(app.state.brightness, 0.75);
    }

    #[test]
    fn the_strength_slider_taps_the_motor_as_strongly_as_it_is_let_go() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Settings, W, H, PPD);
        let track = app.frame(&f, &i).hits.iter().find(|h| h.action == Action::Slider(Slider::Vibration)).unwrap().rect;
        let y = track.centre().1 - app.top().1;
        assert!(app.touch(Touch::Down(track.x + track.w * 0.25, y), &f, 0).is_empty());
        assert!(app.touch(Touch::Move(track.x + track.w * 0.8, y + 40.0), &f, 0).is_empty(), "nothing taps while the finger moves");
        assert_eq!(app.touch(Touch::Up, &f, 0), vec![Effect::Haptic], "let go, the motor taps");
        assert!((app.state.vibration - 0.8).abs() < 1e-5);
        assert!(app.touch(Touch::Down(track.x - 50.0, y), &f, 0).is_empty());
        assert!(app.touch(Touch::Up, &f, 0).is_empty(), "touched beside the track, the slider stays");
        app.state.vibrate = false;
        app.touch(Touch::Down(track.x, y), &f, 0);
        assert!(app.touch(Touch::Up, &f, 0).is_empty(), "with touch vibration off, the motor stays still");
        assert_eq!(app.state.vibration, 0.0, "the strength still moves, down to the slider's left end");
    }

    #[test]
    fn the_language_chosen_is_spoken_on_every_page() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Settings, &f, &i);
        tap(&mut app, &f, &i, Action::Open(Screen::Language));
        assert_eq!(app.frame(&f, &i).title, "Language");
        tap(&mut app, &f, &i, Action::Language(Lang::Vi));
        assert_eq!((app.state.lang, app.screen()), (Lang::Vi, Screen::Language), "the page stays");
        assert_eq!(app.frame(&f, &i).title, "Ngôn ngữ");
        tap(&mut app, &f, &i, Action::Language(Lang::Hant));
        assert_eq!(app.frame(&f, &i).title, "語言");
        tap_back(&mut app, &f, &i, 0);
        app.tick(1000);
        assert_eq!((app.screen(), app.frame(&f, &i).title), (Screen::Settings, "設定"), "and so does the page gone back to");
        app.set_mounts(Ok(vec![MountRow { base: "odm".into(), point: "/odm".into(), mounted: true, holders: vec!["qseecomd".into()] }]));
        tap(&mut app, &f, &i, Action::Open(Screen::Language));
        tap(&mut app, &f, &i, Action::Language(Lang::En));
        tap_back(&mut app, &f, &i, 0);
        app.tick(2000);
        tap_back(&mut app, &f, &i, 2000);
        app.tick(3000);
        tap(&mut app, &f, &i, Action::Open(Screen::Mount));
        tap(&mut app, &f, &i, Action::Unavailable("A partition in use can't be unmounted"));
        assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some("A partition in use can't be unmounted"), "as do its messages");
    }

    #[test]
    fn a_language_chosen_is_kept_and_spoken_over_the_one_the_system_names() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Settings, &f, &i);
        app.take_settings(None, Some(Lang::Hant));
        assert_eq!((app.state.lang, app.settings().language), (Lang::Hant, None), "the system's, until one is chosen");
        tap(&mut app, &f, &i, Action::Open(Screen::Language));
        tap(&mut app, &f, &i, Action::Language(Lang::Vi));
        let kept = app.settings();
        assert_eq!(kept.language.as_deref(), Some("vi"));

        let mut next = App::new(Screen::Home, W, H, PPD);
        next.take_settings(Some(&kept), Some(Lang::Hans));
        assert_eq!(next.state.lang, Lang::Vi, "the next start speaks the language chosen");
        assert_eq!(next.settings(), kept, "and keeps it chosen");
    }

    #[test]
    fn with_no_language_chosen_a_start_speaks_the_system_s_or_else_the_one_last_spoken() {
        let spoke_vietnamese = Settings { spoken: Some("vi".into()), ..Settings::default() };
        let mut app = App::new(Screen::Home, W, H, PPD);
        app.take_settings(Some(&spoke_vietnamese), Some(Lang::Hant));
        assert_eq!(app.state.lang, Lang::Hant, "the one the system names");
        let mut fastboot = App::new(Screen::Fastboot, W, H, PPD);
        fastboot.take_settings(Some(&spoke_vietnamese), None);
        assert_eq!(fastboot.state.lang, Lang::Vi, "a start that names none, as fastboot's");
        let mut first = App::new(Screen::Fastboot, W, H, PPD);
        first.take_settings(None, None);
        assert_eq!(first.state.lang, Lang::En, "and with nothing kept, English");
    }

    #[test]
    fn the_other_pages_choices_are_kept_too() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Settings, &f, &i);
        tap(&mut app, &f, &i, Action::Toggle(Switch::Vibrate));
        app.state.check_signature = true;
        app.state.backup_to = Place::UsbDrive;
        app.state.logs_to = 1;
        let kept = app.settings();
        let mut next = App::new(Screen::Home, W, H, PPD);
        next.take_settings(Some(&kept), None);
        assert!(!next.state.vibrate && next.state.check_signature);
        assert_eq!((next.state.backup_to, next.state.logs_to), (Place::UsbDrive, 1));
    }

    #[test]
    fn logs_are_exported_as_ticked_where_they_can_go() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Logs, W, H, PPD);
        let both = Effect::ExportLogs { recovery: true, kernel: true, storage: false };
        assert_eq!(tap(&mut app, &f, &i, Action::ExportLogs), vec![both], "both, to the temporary folder, at first");
        tap(&mut app, &f, &i, Action::LogPart(0));
        tap(&mut app, &f, &i, Action::LogsTo(1));
        assert!(tap(&mut app, &f, &i, Action::ExportLogs).is_empty(), "internal storage waits for the data partition");
        app.tick(1000);
        assert_eq!(app.deadline(1000), Some(TOAST_MS), "and says so");
        app.data_waiting(None);
        assert_eq!(tap(&mut app, &f, &i, Action::ExportLogs), vec![Effect::ExportLogs { recovery: false, kernel: true, storage: true }]);
        tap(&mut app, &f, &i, Action::LogPart(1));
        assert!(tap(&mut app, &f, &i, Action::ExportLogs).is_empty(), "nothing ticked, nothing saved");
    }

    #[test]
    fn a_time_zone_chosen_is_the_one_kept() {
        let (f, i) = (fonts(), info());
        let mut app = App::new(Screen::Settings, W, H, PPD);
        tap(&mut app, &f, &i, Action::Open(Screen::TimeZone));
        assert_eq!(app.state.time_zone, 0, "China until another is chosen");
        let london = TIME_ZONES.iter().position(|&(_, _, name)| name == "Europe/London").expect("London is listed");
        tap(&mut app, &f, &i, Action::TimeZone(london));
        assert_eq!(app.state.time_zone, london);
        tap(&mut app, &f, &i, Action::TimeZone(0));
        assert_eq!(app.state.time_zone, 0, "and China again");
    }

    #[test]
    fn choosing_how_long_the_screen_stays_on() {
        let (f, i) = (fonts(), info());
        let mut app = opened(Screen::Settings, &f, &i);
        tap(&mut app, &f, &i, Action::Open(Screen::ScreenOff));
        assert_eq!(app.screen(), Screen::ScreenOff);
        let frame = app.frame(&f, &i);
        assert_eq!(frame.title, "Screen timeout");
        assert!(frame.back);
        // The choices sit in order, top to bottom.
        let actions: Vec<Action> = frame.hits.iter().map(|h| h.action).collect();
        assert_eq!(actions, (0..SCREEN_OFF.len()).map(Action::ScreenOff).collect::<Vec<_>>());
        tap(&mut app, &f, &i, Action::ScreenOff(0));
        assert_eq!(app.state.screen_off, 0);
        assert_eq!(app.screen(), Screen::ScreenOff, "the page stays");
        // Fifteen seconds, dim for the last three, once the row tapped has
        // faded back.
        app.tick(1000);
        assert_eq!(app.deadline(1000), Some(12_000));
        app.tick(15_000);
        assert!(!app.display_on());
    }
}
