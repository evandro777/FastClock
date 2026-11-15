//! User settings in one TOML file under the XDG data dir (`~/.local/share/FastClock`).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::clock::Alarm;
use crate::files::{read_or_backup, write_atomic};

/// Folder name under the XDG data dir; `prepare.sh` rewrites it.
pub const APP_DIR: &str = "FastClock";
pub const MIN_SIZE: (u32, u32) = (400, 300);
/// Upper bound for a hand-edited size; a huge value would fail at window creation.
pub const MAX_SIZE: u32 = 8192;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)] // older files keep loading when a field is added
pub struct Settings {
    /// "" follows the system locale; otherwise a gettext code such as "pt_BR".
    pub language: String,
    /// "" follows the system light/dark mode; otherwise "light" or "dark".
    pub theme: String,
    pub width: u32,
    pub height: u32,
    pub alarms: Vec<Alarm>,
    /// Path of the alarm sound; "" is the built-in bell.
    pub sound: String,
    /// Minutes an alarm sleeps after Snooze.
    pub snooze_min: u32,
    /// Pomodoro lengths in minutes.
    pub work_min: u32,
    pub short_min: u32,
    pub long_min: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: String::new(),
            theme: String::new(),
            width: 600,
            height: 450,
            alarms: Vec::new(),
            sound: String::new(),
            snooze_min: 5,
            work_min: 25,
            short_min: 5,
            long_min: 15,
        }
    }
}

fn parse(text: &str) -> Result<Settings, toml::de::Error> {
    toml::from_str(text)
}

/// `None` without `XDG_DATA_HOME` or `HOME`: better no settings than a folder dropped in the current directory.
fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join(APP_DIR))
}

impl Settings {
    pub fn load() -> Self {
        data_dir().map_or_else(Self::default, |dir| Self::load_from(&dir))
    }

    fn load_from(dir: &Path) -> Self {
        // The file is hand-edited; a bad one is moved aside so the next save() must not overwrite what the user wrote.
        let mut s = read_or_backup(&dir.join("settings.toml"), |t| parse(t).map_err(|e| e.to_string())).0;
        s.normalize();
        s
    }

    fn normalize(&mut self) {
        if !matches!(self.theme.as_str(), "light" | "dark") {
            self.theme.clear();
        }
        self.width = self.width.clamp(MIN_SIZE.0, MAX_SIZE);
        self.height = self.height.clamp(MIN_SIZE.1, MAX_SIZE);
        self.alarms.iter_mut().for_each(Alarm::normalize);
        // A zero length would make a timer ring the instant it starts.
        self.snooze_min = self.snooze_min.clamp(1, 120);
        self.work_min = self.work_min.clamp(1, 240);
        self.short_min = self.short_min.clamp(1, 240);
        self.long_min = self.long_min.clamp(1, 240);
    }

    /// Atomic write so a crash can never leave a half-written file. Failures are reported on stderr;
    /// the app keeps working with the in-memory settings.
    pub fn save(&self) {
        let Some(dir) = data_dir() else { return };
        if let Err(e) = self.save_to(&dir) {
            eprintln!("cannot save settings to {}: {e}", dir.display());
        }
    }

    fn save_to(&self, dir: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&dir.join("settings.toml"), text)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn old_files_still_load() {
        let parsed: Settings = toml::from_str("width = 700").unwrap();
        assert_eq!((parsed.width, parsed.language.as_str()), (700, ""));
    }

    #[test]
    fn invalid_file_is_an_error_not_defaults() {
        assert!(parse("width = \"wide\"").is_err());
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fastclock-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let s = Settings {
            language: "pt_BR".into(),
            theme: "dark".into(),
            width: 700,
            height: 500,
            alarms: vec![Alarm {
                hour: 6,
                minute: 45,
                label: "Gym".into(),
                days: 0b0011111,
                enabled: false,
            }],
            sound: "/home/a/ding.ogg".into(),
            ..Settings::default()
        };
        s.save_to(&dir).unwrap();
        let loaded = Settings::load_from(&dir);
        assert_eq!(
            (
                loaded.language.as_str(),
                loaded.theme.as_str(),
                loaded.width,
                loaded.height
            ),
            ("pt_BR", "dark", 700, 500)
        );
        assert_eq!(loaded.alarms, s.alarms);
        assert_eq!(loaded.sound, "/home/a/ding.ogg");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_file_is_kept_as_bad_and_defaults_are_used() {
        let dir = temp_dir("bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.toml"), "width = \"wide\"").unwrap();
        assert_eq!(Settings::load_from(&dir).width, Settings::default().width);
        assert!(
            fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("settings.toml.bad."))
        );
        assert!(!dir.join("settings.toml").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_defaults_to_system_and_unknown_values_are_reset() {
        let mut old: Settings = toml::from_str("width = 700").unwrap();
        old.normalize();
        assert_eq!(old.theme, "");
        let mut odd: Settings = toml::from_str("theme = \"blue\"").unwrap();
        odd.normalize();
        assert_eq!(odd.theme, "");
        let mut dark: Settings = toml::from_str("theme = \"dark\"").unwrap();
        dark.normalize();
        assert_eq!(dark.theme, "dark");
    }

    #[test]
    fn clock_options_default_and_are_kept_sane() {
        let mut old: Settings = toml::from_str("width = 700").unwrap();
        old.normalize();
        assert_eq!(
            (old.alarms.len(), old.sound.as_str(), old.snooze_min, old.work_min),
            (0, "", 5, 25)
        );
        let mut odd: Settings =
            toml::from_str("snooze_min = 0\nwork_min = 999999\n[[alarms]]\nhour = 99\nminute = 99\ndays = 255")
                .unwrap();
        odd.normalize();
        assert_eq!((odd.snooze_min, odd.work_min), (1, 240));
        let a = &odd.alarms[0];
        assert_eq!((a.hour, a.minute, a.days, a.enabled), (23, 59, 0x7f, true));
    }

    #[test]
    fn window_size_is_clamped_both_ways() {
        let mut s: Settings = toml::from_str("width = 4000000000\nheight = 1").unwrap();
        s.normalize();
        assert_eq!((s.width, s.height), (MAX_SIZE, MIN_SIZE.1));
    }
}
