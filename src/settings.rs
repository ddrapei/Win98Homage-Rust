//! settings.ini: where each widget sits, whether it is shown, and the knobs
//! that start out at the values in the skins' [Variables].
//!
//! The file is kept as its own lines and edited in place: a value the program
//! writes back (a position after a drag, the paper colour) replaces one line
//! and leaves every comment and every other line as the user wrote them.

use std::path::{Path, PathBuf};

use crate::skin;

pub struct Settings {
    path: PathBuf,
    lines: Vec<String>,
    dirty: bool,
}

/// Where settings.ini lives: beside the program if one is already there (a
/// portable copy), otherwise %APPDATA%\Win98Widgets.
pub fn settings_dir() -> PathBuf {
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        if dir.join("settings.ini").is_file() {
            return dir;
        }
    }
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join("Win98Widgets"),
        None => PathBuf::from("."),
    }
}

/// Things the program keeps for itself and the user never edits.
pub fn data_dir() -> PathBuf {
    match std::env::var_os("LOCALAPPDATA") {
        Some(local) => PathBuf::from(local).join("Win98Widgets"),
        None => settings_dir(),
    }
}

/// Written beside the target and swapped in whole, so a file is never seen
/// half written.
pub fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

impl Settings {
    pub fn load() -> Settings {
        let dir = settings_dir();
        let path = dir.join("settings.ini");
        match std::fs::read_to_string(&path) {
            Ok(text) => Settings { path, lines: text.lines().map(str::to_owned).collect(), dirty: false },
            Err(_) => {
                let _ = std::fs::create_dir_all(&dir);
                let mut s = Settings { path, lines: default_file().lines().map(str::to_owned).collect(), dirty: true };
                s.save();
                s
            }
        }
    }

    /// For the test harness: defaults, never written anywhere.
    pub fn in_memory() -> Settings {
        Settings { path: PathBuf::new(), lines: default_file().lines().map(str::to_owned).collect(), dirty: false }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn dir(&self) -> PathBuf {
        self.path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The line holding `key` in `section`.
    fn find(&self, section: &str, key: &str) -> Option<usize> {
        let mut inside = false;
        for (i, line) in self.lines.iter().enumerate() {
            let t = line.trim();
            if t.starts_with('[') && t.ends_with(']') {
                inside = t[1..t.len() - 1].eq_ignore_ascii_case(section);
            } else if inside && !t.starts_with(';') {
                if t.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key)) {
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let line = &self.lines[self.find(section, key)?];
        line.split_once('=').map(|(_, v)| v.trim())
    }

    pub fn text(&self, section: &str, key: &str, default: &str) -> String {
        self.get(section, key).unwrap_or(default).to_owned()
    }

    pub fn int(&self, section: &str, key: &str, default: i32) -> i32 {
        self.get(section, key).and_then(|v| v.parse::<f64>().ok()).map_or(default, |v| v as i32)
    }

    pub fn flag(&self, section: &str, key: &str, default: bool) -> bool {
        self.int(section, key, default as i32) != 0
    }

    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let line = format!("{key}={value}");
        if let Some(i) = self.find(section, key) {
            if self.lines[i].trim() != line {
                self.lines[i] = line;
                self.dirty = true;
            }
            return;
        }
        // A new key goes at the end of its section, before the blank lines
        // that separate it from the next.
        let header = self.lines.iter().position(|l| {
            let t = l.trim();
            t.starts_with('[') && t.ends_with(']') && t[1..t.len() - 1].eq_ignore_ascii_case(section)
        });
        match header {
            Some(h) => {
                let mut at = self.lines[h + 1..]
                    .iter()
                    .position(|l| l.trim_start().starts_with('['))
                    .map_or(self.lines.len(), |n| h + 1 + n);
                while at > h + 1 && self.lines[at - 1].trim().is_empty() {
                    at -= 1;
                }
                self.lines.insert(at, line);
            }
            None => {
                if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    self.lines.push(String::new());
                }
                self.lines.push(format!("[{section}]"));
                self.lines.push(line);
            }
        }
        self.dirty = true;
    }

    pub fn set_int(&mut self, section: &str, key: &str, value: i32) {
        self.set(section, key, &value.to_string());
    }

    /// Write the file out if anything changed.  Best effort: the program
    /// carries on with what it has in memory if the folder is read-only.
    pub fn save(&mut self) {
        if !self.dirty || self.path.as_os_str().is_empty() {
            return;
        }
        let mut text = self.lines.join("\r\n");
        text.push_str("\r\n");
        if write_whole(&self.path, text.as_bytes()).is_ok() {
            self.dirty = false;
        }
    }
}

/// The file a first run writes: every knob at the value the skins ship with.
fn default_file() -> String {
    let dt = &skin::datetime::SKIN;
    let notes = &skin::notes::SKIN;
    let wx = &skin::weather::SKIN;
    format!(
        "\
; Win98 Widgets -- settings.
; Edit while the program is not running, or use \"Reload settings\" in the tray
; menu afterwards.  Positions, the paper colour and the 12/24-hour choice are
; written back here by the program; everything else is yours.

[General]
; desktop = behind every window, on the desktop itself
; normal  = an ordinary window that never takes the keyboard
; topmost = above every window
Position=desktop
SnapEdges=1
LockPosition=0

[DateTime]
Enabled=1
; 1 = Monday first, 0 = Sunday first
WeekStart={week_start}
Use24Hour={use24}
HideSeconds={hide_seconds}
HideRing={hide_ring}
; days of history kept in the usage log
HistoryDays={history}
; the hours of the day the usage graph covers
GraphFrom={graph_from}
GraphTo={graph_to}

[Resources]
Enabled=1

[Notes]
Enabled=1
; the note itself; a bare name is kept beside this file
NoteFile={note_file}
; 1 = canary paper, 2 = white
Colour={colour}
; shown in the left status panel; blank stamps today's date on the next start
Created={created}

[Weather]
Enabled=1
; celsius or fahrenheit
TempUnit={unit}
; fill both in to pin the forecast to one place and never ask ip-api.com
ManualLat={lat}
ManualLon={lon}
ManualLocation={place}
",
        week_start = dt.var("WeekStart"),
        use24 = dt.var("Use24Hour"),
        hide_seconds = dt.var("HideSeconds"),
        hide_ring = dt.var("HideRing"),
        history = dt.var("HistoryDays"),
        graph_from = dt.var("GraphFrom"),
        graph_to = dt.var("GraphTo"),
        note_file = notes.var("NoteFile"),
        colour = notes.var("Colour"),
        created = notes.var("Created"),
        unit = wx.var("TempUnit"),
        lat = wx.var("ManualLat"),
        lon = wx.var("ManualLon"),
        place = wx.var("ManualLocation"),
    )
}
