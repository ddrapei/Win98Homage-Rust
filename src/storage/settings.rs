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

/// The name in a `[Section]` line; None for any other line.
fn header(line: &str) -> Option<&str> {
    line.trim().strip_prefix('[')?.strip_suffix(']')
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

    /// The same, as if it were kept in `dir`: for what is looked for beside
    /// the file -- and where `save` then writes it.
    #[cfg(test)]
    pub fn in_memory_at(dir: &Path) -> Settings {
        Settings { path: dir.join("settings.ini"), ..Settings::in_memory() }
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

    /// The name of every section, in the order the file has them.
    pub fn sections(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().filter_map(|l| header(l))
    }

    /// Take a section out of the file: its header and every line down to the
    /// next one.
    pub fn remove_section(&mut self, section: &str) {
        let named = |l: &String| header(l).is_some_and(|n| n.eq_ignore_ascii_case(section));
        while let Some(h) = self.lines.iter().position(named) {
            let end = self.lines[h + 1..].iter().position(|l| header(l).is_some()).map_or(self.lines.len(), |n| h + 1 + n);
            self.lines.drain(h..end);
            // it was the last: the blank line that set it apart goes with it
            while h == self.lines.len() && self.lines.last().is_some_and(|l| l.trim().is_empty()) {
                self.lines.pop();
            }
            self.dirty = true;
        }
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
; menu afterwards.  Positions, the paper colour, the 12/24-hour choice and the
; weather's city are written back here by the program; everything else is yours.

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
; The first note.  Every other one (right-click, Widgets, Sticky Note +) has a section
; like this of its own -- [Notes2], [Notes3], ... -- and a file of its own.
Enabled=1
; the note itself; a bare name is kept beside this file
NoteFile={note_file}
; the paper: 1 canary, 2 white, 3 pink, 4 green, 5 blue, 6 orange, 7 lilac
; (the note's swatch button goes round them)
Colour={colour}
; 1 = ruled paper, 0 = plain
Ruled={ruled}
; shown in the left status panel; blank stamps today's date on the next start
Created={created}

[Weather]
Enabled=1
; celsius or fahrenheit
TempUnit={unit}
; the place the forecast is for.  Typing a city into the widget's city field
; fills these three in; so can you.  With the first two set, ip-api.com is
; never asked.  All three empty: wherever the public IP address appears to be
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
        ruled = notes.var("Ruled"),
        created = notes.var("Created"),
        unit = wx.var("TempUnit"),
        lat = wx.var("ManualLat"),
        lon = wx.var("ManualLon"),
        place = wx.var("ManualLocation"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str) -> Settings {
        Settings { path: PathBuf::new(), lines: text.lines().map(str::to_owned).collect(), dirty: false }
    }

    #[test]
    fn a_section_is_removed_with_its_lines_and_nothing_else() {
        let mut s = of("[A]\nx=1\n\n[Notes2]\n; a comment\nNoteFile=Notes2.txt\n\n[B]\ny=2");
        s.remove_section("notes2");
        assert_eq!(s.lines, ["[A]", "x=1", "", "[B]", "y=2"]);
        assert_eq!(s.sections().collect::<Vec<_>>(), ["A", "B"]);

        // the last one in the file leaves no blank line behind
        s.remove_section("B");
        assert_eq!(s.lines, ["[A]", "x=1"]);
        assert!(s.dirty);
    }

    #[test]
    fn removing_a_section_that_is_not_there_changes_nothing() {
        let mut s = of("[A]\nx=1\n");
        s.remove_section("Notes2");
        assert_eq!(s.lines, ["[A]", "x=1"]);
        assert!(!s.dirty);
    }
}
