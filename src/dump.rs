//! `--dump <dir> [--state <file>]`: draw each widget off-screen, at a moment
//! and in a state given by the file, and write it out as a 32-bit BMP.
//!
//! This is how the port is checked against Rainmeter: the same clock time,
//! the same usage log, the same note and the same readings go in, and the
//! picture that comes out is compared pixel for pixel with a capture of the
//! skin.  Nothing here touches the user's settings or data.
//!
//! The state file is `key=value` lines:
//!
//!   now=2026-11-11 17:30:45            the moment to draw
//!   usage_log=C:\path\Usage.log        the journal to read (never written)
//!   live_start=2026-11-11 17:27:35     when the running interval began
//!   panel_at=2026-11-11 17:30:00       when the usage panel was last redrawn
//!   cpu=0.44   ram=0.95                the Resource Meter's two readings
//!   note_file=C:\path\Notes.txt
//!   weather=9.3,3,0,10.1,4.9,Place     temp, code, is_day, max, min, city
//!   set.Notes.Created=30 Jul 2026      any settings.ini value
//!   do.datetime=SetPage 1;Select 12    commands to run before drawing

use std::path::{Path, PathBuf};

use crate::canvas::Canvas;
use crate::clock;
use crate::datetime::DateTime;
use crate::notes::Notes;
use crate::resources::Resources;
use crate::settings::Settings;
use crate::skin::{self, Hooks};
use crate::text::Text;
use crate::weather::Weather;
use crate::widget::{Ctx, Widget};

fn write_bmp(path: &Path, w: i32, h: i32, px: &[u32]) -> std::io::Result<()> {
    let size = (w * h * 4) as u32;
    let mut out = Vec::with_capacity(54 + size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + size).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&(-h).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    for p in px {
        out.extend_from_slice(&p.to_le_bytes());
    }
    std::fs::write(path, out)
}

fn render(widget: &dyn Widget, text: &mut Text, path: &Path) {
    let skin = widget.skin();
    let mut px = vec![0u32; (skin.w * skin.h) as usize];
    // SAFETY: the canvas is exactly the vector.
    let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), skin.w, skin.w, skin.h) };
    skin::draw(skin, widget as &dyn Hooks, &mut canvas, text, &mut Vec::new());
    let _ = write_bmp(path, skin.w, skin.h, &px);
}

/// "SetPage 1;Click 30 40": each command with its argument, or its point.
fn perform(widget: &mut dyn Widget, script: &str, ctx: &mut Ctx) {
    for step in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let mut parts = step.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let nums: Vec<i32> = parts.filter_map(|p| p.parse().ok()).collect();
        let (arg, x, y) = match nums[..] {
            [a] => (a, 0, a),
            [x, y] => (0, x, y),
            _ => (0, 0, 0),
        };
        widget.command(cmd, arg, x, y, ctx);
    }
}

pub fn run(args: &[String]) -> i32 {
    let Some(dir) = args.first().map(PathBuf::from) else { return 2 };
    let state: Vec<(String, String)> = args
        .iter()
        .position(|a| a == "--state")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())))
        .collect();
    let get = |key: &str| state.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    let now = get("now").and_then(clock::parse_local).unwrap_or_else(clock::now);
    let mut settings = Settings::in_memory();
    for (key, value) in &state {
        if let Some((section, name)) = key.strip_prefix("set.").and_then(|k| k.split_once('.')) {
            settings.set(section, name, value);
        }
    }
    let Some(mut text) = Text::new(skin::datetime::SKIN.var("Font"), skin::FONTS) else { return 1 };
    let mut ctx = Ctx { now, settings: &mut settings, locked: false, signed_in: None };

    {
        let mut w = DateTime::new(&ctx, Some(get("usage_log").map(PathBuf::from)));
        if let Some(start) = get("live_start").and_then(clock::parse_local) {
            w.set_live(start, now);
        }
        // The usage panel is brought up to date on the minute, the clock on
        // the second: to match a capture, tick at the minute it last was.
        if let Some(earlier) = get("panel_at").and_then(clock::parse_local) {
            ctx.now = earlier;
            w.tick(&mut ctx);
            ctx.now = now;
        }
        w.tick(&mut ctx);
        perform(&mut w, get("do.datetime").unwrap_or(""), &mut ctx);
        render(&w, &mut text, &dir.join("datetime.bmp"));
    }
    {
        let mut w = Resources::new();
        let reading = |key: &str| get(key).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        w.set_fixed(reading("cpu"), reading("ram"));
        w.tick(&mut ctx);
        render(&w, &mut text, &dir.join("resources.bmp"));
    }
    {
        let mut w = Notes::new(&mut ctx, Some(get("note_file").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("no-such-note.txt"))));
        perform(&mut w, get("do.notes").unwrap_or(""), &mut ctx);
        render(&w, &mut text, &dir.join("notes.bmp"));
    }
    {
        let mut w = Weather::new(&ctx);
        if let Some(v) = get("weather") {
            let p: Vec<&str> = v.split(',').collect();
            let num = |i: usize| p.get(i).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0);
            w.set_fixed(num(0), num(1) as i32, num(2) != 0.0, num(3), num(4), p.get(5).copied().unwrap_or(""));
        }
        render(&w, &mut text, &dir.join("weather.bmp"));
    }
    0
}
