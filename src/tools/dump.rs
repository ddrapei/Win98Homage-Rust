//! `--dump <dir> [--state <file>]`: draw each widget off-screen, at a moment
//! and in a state given by the file, and write it out as a 32-bit BMP.
//!
//! This is how a change to a skin or to the drawing is checked: the same clock
//! time, the same usage log, the same note and the same readings go in, and
//! the pictures that come out before and after can be compared pixel for
//! pixel.  Nothing here touches the user's settings or data.
//!
//! The state file is `key=value` lines:
//!
//!   now=2026-11-11 17:30:45            the moment to draw
//!   scale=2                            each widget as a monitor at that
//!                                      scale shows it: every pixel a 2 x 2
//!                                      block (1 if not given)
//!   usage_log=C:\path\Usage.log        the journal to read (never written)
//!   live_start=2026-11-11 17:27:35     when the running interval began
//!   panel_at=2026-11-11 17:30:00       when the usage panel was last redrawn
//!   cpu=0.44   ram=0.95                the Resource Meter's two readings
//!   note_file=C:\path\Notes.txt
//!   note_file.2=C:\path\Other.txt      one more note, the one [Notes2] would
//!                                      be (and .3, .4, ...): drawn to
//!                                      notes2.bmp, with set.Notes2.*,
//!                                      do.notes2 and type.notes2 for it
//!   weather=9.3,3,0,10.1,4.9,Place     temp, code, is_day, max, min, city
//!   rain=2.5                           seconds into the shower (always the same one)
//!   disk=C,3,Windows,511000000000,118000000000
//!                                      letter, kind (2 removable, 3 fixed,
//!                                      4 network, 5 disc), label, bytes in
//!                                      all, bytes free
//!   battery=76,0,0,8100                percent, plugged in (0/1), charging
//!                                      (0/1), seconds left (empty or -1 for
//!                                      no estimate yet); battery=none, or no
//!                                      line at all, is a machine with none
//!   claude=42,2026-11-11 19:45,18,2026-11-14 09:00
//!                                      the Claude usage: the session's
//!                                      percentage and when it resets, then
//!                                      the week's (local times; either may
//!                                      be empty); claude=none, or no line
//!                                      at all, is a machine with no sign-in
//!   set.Notes.Created=30 Jul 2026      any settings.ini value
//!   set.General.Tint=0,0,128           among them the widgets' colour
//!   do.datetime=SetPage 1;Select 12    commands to run before drawing
//!   type.notes=press 60 50;type milk;key Shift+Home
//!                                      the note typed on, a step at a time:
//!                                      press / double / shift / drag X Y at a
//!                                      point of the window, type TEXT, and
//!                                      key [Shift+][Ctrl+]NAME.  Nothing is
//!                                      written to the note file by these.
//!   type.weather=press 87 167;type Lviv
//!                                      the same for the weather's city field;
//!                                      nothing is looked up.
//!   type.world=press 202 135;type Kyiv
//!                                      and for a city field of the world
//!                                      clock, whose three cities and zones
//!                                      are set.World.City1, set.World.Zone1
//!                                      and so on.
//!   set.Countdown.Date=2026-11-27      the countdown's day and name, as
//!   set.Countdown.Name=Birthday        settings.ini has them; drawn to
//!                                      countdown.bmp -- and any other
//!                                      countdown the state sets something
//!                                      for (set.Countdown2.Date=...) to
//!                                      countdown2.bmp, and so on
//!   do.countdown=Shift 1;Select 12     its commands (do.countdown2, ...)
//!   type.countdown=press 100 294;type Party
//!                                      and its name field typed on
//!                                      (type.countdown2, ...); nothing is
//!                                      written to settings.ini by these.

use std::path::{Path, PathBuf};

use crate::battery::Battery;
use crate::canvas::{self, Canvas, Rect};
use crate::claude::Claude;
use crate::clock;
use crate::countdown::{self, Countdown};
use crate::datetime::DateTime;
use crate::disk::Disk;
use crate::keys::Key;
use crate::notes::{self, Notes};
use crate::rain::Rain;
use crate::resources::Resources;
use crate::settings::Settings;
use crate::skin::{self, Hooks};
use crate::text::Text;
use crate::tint;
use crate::weather::Weather;
use crate::widget::{Ctx, Widget};
use crate::world::World;

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

/// The widget drawn whole, as its window shows it at a scale of `n`: its
/// skin's width times n pixels a row.
fn picture(widget: &dyn Widget, text: &mut Text, n: i32) -> Vec<u32> {
    let skin = widget.skin();
    let mut px = vec![0u32; (skin.w * skin.h) as usize];
    // SAFETY: the canvas is exactly the vector.
    let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), skin.w, skin.w, skin.h) };
    skin::draw(skin, widget as &dyn Hooks, &mut canvas, text, &mut Vec::new());
    drop(canvas);
    // put up as the window is, at the scale asked for
    let mut out = vec![0u32; px.len() * (n * n) as usize];
    canvas::scale_into(&px, skin.w as usize, Rect::new(0, 0, skin.w, skin.h), &mut out, (skin.w * n) as usize, n);
    out
}

fn render(widget: &dyn Widget, text: &mut Text, path: &Path, n: i32) {
    let skin = widget.skin();
    let _ = write_bmp(path, skin.w * n, skin.h * n, &picture(widget, text, n));
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

/// "press 60 50;type milk;key Shift+Home": a widget typed on -- the note, the
/// weather's city field -- through the same doors the window uses.
fn type_on(widget: &mut dyn Widget, script: &str) {
    for step in script.split(';').map(str::trim_start).filter(|s| !s.is_empty()) {
        let (verb, rest) = step.split_once(' ').unwrap_or((step, ""));
        let mut nums = rest.split_whitespace().filter_map(|p| p.parse::<i32>().ok());
        let (x, y) = (nums.next().unwrap_or(0), nums.next().unwrap_or(0));
        match verb {
            "press" => drop(widget.press(x, y, false, false)),
            "shift" => drop(widget.press(x, y, false, true)),
            "double" => {
                widget.press(x, y, false, false);
                widget.press(x, y, true, false);
            }
            "drag" => drop(widget.drag(x, y)),
            "type" => rest.chars().for_each(|c| drop(widget.typed(c))),
            "key" => {
                let mut name = rest.trim();
                let (mut shift, mut ctrl) = (false, false);
                loop {
                    if let Some(n) = name.strip_prefix("Shift+") {
                        (shift, name) = (true, n);
                    } else if let Some(n) = name.strip_prefix("Ctrl+") {
                        (ctrl, name) = (true, n);
                    } else {
                        break;
                    }
                }
                let key = match name {
                    "Left" => Key::Left,
                    "Right" => Key::Right,
                    "Up" => Key::Up,
                    "Down" => Key::Down,
                    "Home" => Key::Home,
                    "End" => Key::End,
                    "PageUp" => Key::PageUp,
                    "PageDown" => Key::PageDown,
                    "Backspace" => Key::Backspace,
                    "Delete" => Key::Delete,
                    "Enter" => Key::Enter,
                    "Tab" => Key::Tab,
                    "Escape" => Key::Escape,
                    "A" => Key::SelectAll,
                    "Z" => Key::Undo,
                    "Y" => Key::Redo,
                    _ => continue,
                };
                widget.key(key, shift, ctrl);
            }
            _ => {}
        }
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
    let n = get("scale").and_then(|v| v.parse::<i32>().ok()).unwrap_or(1).clamp(1, 8);
    let render = |w: &dyn Widget, text: &mut Text, path: &Path| render(w, text, path, n);
    let mut settings = Settings::in_memory();
    for (key, value) in &state {
        if let Some((section, name)) = key.strip_prefix("set.").and_then(|k| k.split_once('.')) {
            settings.set(section, name, value);
        }
    }
    // in the colour settings.ini would have them in: set.General.Tint=0,0,128
    tint::set(settings.tint());
    let Some(mut text) = Text::new(skin::datetime::SKIN.var("Font"), skin::FONTS) else { return 1 };
    let mut ctx = Ctx { now, settings: &mut settings, locked: false, signed_in: None };

    {
        let mut w = DateTime::new(&ctx, Some(get("usage_log").map(PathBuf::from)));
        if let Some(start) = get("live_start").and_then(clock::parse_local) {
            w.set_live(start, now);
        }
        // The usage panel is brought up to date on the minute, the clock on
        // the second: to draw a moment between the two, tick first at the
        // minute the panel last was.
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
    // the first note, then every other the state names a file for
    let first = (1, get("note_file").unwrap_or("no-such-note.txt"));
    let others = state.iter().filter_map(|(k, v)| Some((k.strip_prefix("note_file.")?.parse::<u32>().ok().filter(|&n| n > 1)?, v.as_str())));
    for (n, file) in core::iter::once(first).chain(others) {
        let tag = if n > 1 { format!("notes{n}") } else { String::from("notes") };
        let section = if n > 1 { format!("{}{n}", notes::FIRST) } else { String::from(notes::FIRST) };
        let mut w = Notes::new(&mut ctx, &section, Some(PathBuf::from(file)));
        perform(&mut w, get(&format!("do.{tag}")).unwrap_or(""), &mut ctx);
        type_on(&mut w, get(&format!("type.{tag}")).unwrap_or(""));
        render(&w, &mut text, &dir.join(format!("{tag}.bmp")));
    }
    {
        let mut w = Weather::new(&ctx);
        if let Some(v) = get("weather") {
            let p: Vec<&str> = v.split(',').collect();
            let num = |i: usize| p.get(i).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0);
            w.set_fixed(num(0), num(1) as i32, num(2) != 0.0, num(3), num(4), p.get(5).copied().unwrap_or(""));
        }
        type_on(&mut w, get("type.weather").unwrap_or(""));
        render(&w, &mut text, &dir.join("weather.bmp"));
    }
    {
        let mut w = Rain::new(&ctx);
        w.set_fixed(get("rain").and_then(|v| v.parse().ok()).unwrap_or(2.5));
        render(&w, &mut text, &dir.join("rain.bmp"));
    }
    {
        // never this machine's drives: with no figures given, a drive that has none
        let mut w = Disk::new(&ctx);
        let p: Vec<&str> = get("disk").unwrap_or("C,3,,0,0").split(',').map(str::trim).collect();
        let num = |i: usize| p.get(i).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let letter = p.first().and_then(|s| s.bytes().next()).unwrap_or(b'C');
        w.set_fixed(letter, num(1) as u32, p.get(2).copied().unwrap_or(""), num(3), num(4));
        render(&w, &mut text, &dir.join("disk.bmp"));
    }
    {
        // the three cities settings.ini has (set.World.City1, set.World.Zone1,
        // ...), at the moment given; nothing is looked up
        let mut w = World::new(&ctx);
        w.set_fixed();
        type_on(&mut w, get("type.world").unwrap_or(""));
        render(&w, &mut text, &dir.join("world.bmp"));
    }
    {
        // never this machine's battery: with no figures given, no battery
        let mut w = Battery::new(&ctx);
        let reading = get("battery").filter(|v| !v.eq_ignore_ascii_case("none")).map(|v| {
            let p: Vec<&str> = v.split(',').map(str::trim).collect();
            let num = |i: usize| p.get(i).and_then(|s| s.parse::<i64>().ok());
            let flag = |i: usize| num(i).is_some_and(|n| n != 0);
            let percent = num(0).map_or(0, |n| n.clamp(0, 255) as u8);
            (percent, flag(1), flag(2), num(3).filter(|&n| n >= 0).map(|n| n.min(u32::MAX as i64) as u32))
        });
        w.set_fixed(reading);
        render(&w, &mut text, &dir.join("battery.bmp"));
    }
    // the first countdown, then every other the state sets anything for
    let sections: Vec<String> = core::iter::once(String::from(countdown::FIRST)).chain(countdown::others(ctx.settings)).collect();
    for section in sections {
        let tag = section.to_ascii_lowercase();
        let mut w = Countdown::new(&ctx, &section);
        perform(&mut w, get(&format!("do.{tag}")).unwrap_or(""), &mut ctx);
        type_on(&mut w, get(&format!("type.{tag}")).unwrap_or(""));
        render(&w, &mut text, &dir.join(format!("{tag}.bmp")));
    }
    {
        // never this machine's sign-in: with no figures given, there is none
        let mut w = Claude::new(&ctx);
        // a local time, with or without its seconds; empty for none
        let at = |s: &str| clock::parse_local(s).or_else(|| clock::parse_local(&format!("{s}:00")));
        let reading = get("claude").filter(|v| !v.eq_ignore_ascii_case("none")).map(|v| {
            let p: Vec<&str> = v.split(',').map(str::trim).collect();
            let pct = |i: usize| p.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let when = |i: usize| p.get(i).and_then(|s| at(s));
            (pct(0), when(1), pct(2), when(3))
        });
        w.set_fixed(reading, now);
        render(&w, &mut text, &dir.join("claude.bmp"));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of every kind of widget, made as `--dump` makes them, showing the
    /// same thing whenever it is made: in whatever tint is set.
    fn every_widget(ctx: &mut Ctx, note: &Path) -> Vec<Box<dyn Widget>> {
        let mut datetime = DateTime::new(ctx, Some(None));
        datetime.tick(ctx);
        perform(&mut datetime, "Select 12", ctx);
        let mut resources = Resources::new();
        resources.set_fixed(0.44, 0.95);
        resources.tick(ctx);
        let notes = Notes::new(ctx, notes::FIRST, Some(note.to_path_buf()));
        let mut weather = Weather::new(ctx);
        weather.set_fixed(9.3, 3, false, 10.1, 4.9, "Lviv");
        // something selected in the city field, for its colours too
        type_on(&mut weather, "double 87 167");
        let mut rain = Rain::new(ctx);
        rain.set_fixed(2.5);
        let mut disk = Disk::new(ctx);
        disk.set_fixed(b'C', 3, "Windows", 511_000_000_000, 118_000_000_000);
        let mut world = World::new(ctx);
        world.set_fixed();
        let mut battery = Battery::new(ctx);
        battery.set_fixed(Some((35, true, true, None)));
        // a day in this month, so that the ring is drawn, and something
        // selected in the name field, for its colours too
        ctx.settings.set(countdown::FIRST, "Date", "2026-11-27");
        ctx.settings.set(countdown::FIRST, "Name", "Birthday");
        let mut countdown = Countdown::new(ctx, countdown::FIRST);
        type_on(&mut countdown, "double 115 294");
        // both bars part full, so that blocks and the field over the rest
        // are both there
        let mut claude = Claude::new(ctx);
        claude.set_fixed(Some((42.0, Some(ctx.now + 8040), 18.0, Some(ctx.now + 3 * 86_400))), ctx.now);
        vec![
            Box::new(datetime),
            Box::new(resources),
            Box::new(notes),
            Box::new(weather),
            Box::new(rain),
            Box::new(disk),
            Box::new(world),
            Box::new(battery),
            Box::new(countdown),
            Box::new(claude),
        ]
    }

    #[test]
    fn a_widget_given_another_tint_is_drawn_as_one_made_in_it() {
        let dir = std::env::temp_dir().join(format!("win98-dump-test-{}-tint", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("Notes.txt");
        std::fs::write(&note, "Buy milk\r\n[ ] call home\r\n[x] done\r\n").unwrap();
        let mut text = Text::new(skin::datetime::SKIN.var("Font"), skin::FONTS).unwrap();
        let mut settings = Settings::in_memory();
        settings.set("Notes", "Ruled", "1");
        settings.set("Notes", "Created", "30 Jul 2026");
        let now = clock::parse_local("2026-11-11 17:30:45").unwrap();
        let mut ctx = Ctx { now, settings: &mut settings, locked: false, signed_in: None };

        tint::set(tint::BASE);
        let mut widgets = every_widget(&mut ctx, &note);
        let before: Vec<Vec<u32>> = widgets.iter().map(|w| picture(w.as_ref(), &mut text, 1)).collect();

        let navy = tint::PRESETS.iter().find(|p| p.0 == "Navy").unwrap().1;
        tint::set(navy);
        for w in &mut widgets {
            w.retint();
        }
        let fresh = every_widget(&mut ctx, &note);
        for ((w, made), before) in widgets.iter().zip(&fresh).zip(&before) {
            let skin = w.skin();
            let name = w.about().lines().next().unwrap_or("?");
            let after = picture(w.as_ref(), &mut text, 1);
            let want = picture(made.as_ref(), &mut text, 1);
            if let Some(at) = after.iter().zip(&want).position(|(a, b)| a != b) {
                let (x, y) = (at as i32 % skin.w, at as i32 / skin.w);
                panic!("{name} ({}x{}): retinted, {x},{y} is {:08X}; made in the tint, {:08X}", skin.w, skin.h, after[at], want[at]);
            }
            assert_ne!(&after, before, "{name}: the tint changed nothing");
            // nothing left over in the skins' own accent or title bar
            for var in ["Accent2", "TitleB"] {
                let old = skin::parse_colour(skin.var(var)).unwrap();
                assert!(!after.contains(&old), "{name}: still has {var} ({old:08X}) somewhere");
            }
        }

        // And what the tint leaves alone it has left alone: every pixel that
        // was one of the skin's untinted colours -- the chrome's greys and
        // whites, the inks on the accents, the note's paper, the weather's
        // earth -- is still that colour.  Nearly: one of them is within 1
        // (the sheet lies under the note's click target, black at 1/255),
        // and stays within 3, for the edge of an accent that touches a
        // pixel of the chrome by a hair turns with the accent.  A colour the
        // tint had turned would be tens out.
        let within = |d: i32| move |p: u32, c: u32| p >> 24 == c >> 24 && (0..3).all(|k| ((p >> (8 * k) & 255) as i32 - (c >> (8 * k) & 255) as i32).abs() <= d);
        let (near, close) = (within(1), within(3));
        let own = |skin: &skin::Skin, vars: &[&str]| -> Vec<u32> { vars.iter().filter_map(|v| skin::parse_colour(skin.var(v))).collect() };
        for (i, w) in widgets.iter().enumerate() {
            let (skin, name) = (w.skin(), w.about().lines().next().unwrap_or("?"));
            let kept = own(skin, skin.fixed);
            let after = picture(w.as_ref(), &mut text, 1);
            let mut seen = 0;
            for (at, (&was, &is)) in before[i].iter().zip(&after).enumerate() {
                if let Some(&c) = kept.iter().find(|&&c| near(was, c)) {
                    let (x, y) = (at as i32 % skin.w, at as i32 / skin.w);
                    assert!(close(is, c), "{name}: {x},{y} was {was:08X}, untinted {c:08X}, and the tint made it {is:08X}");
                    seen += 1;
                }
            }
            // the chrome is a good part of every window
            assert!(seen > 1000, "{name} shows next to nothing of its untinted colours ({seen} px)");
        }
        // among them the paper and its margin, and the earth
        for (i, vars) in [(2, &["Paper1", "NoteMargin"][..]), (3, &["IcoOutline", "IcoLand", "IcoRain", "IcoIce"][..])] {
            let colours = own(widgets[i].skin(), vars);
            let seen = before[i].iter().filter(|&&p| colours.iter().any(|&c| near(p, c))).count();
            assert!(seen > 20, "window {i} shows next to nothing of {vars:?}");
        }

        // Back to the skins' own -- from the tint they were given, and from
        // one they were made in -- is exactly what there was before any tint,
        // pixel for pixel.
        let same_as_before = |widgets: &mut Vec<Box<dyn Widget>>, text: &mut Text, from: &str| {
            tint::set(tint::BASE);
            for (w, before) in widgets.iter_mut().zip(&before) {
                w.retint();
                let name = w.about().lines().next().unwrap_or("?");
                assert!(picture(w.as_ref(), text, 1) == *before, "{name}: back from {from}, not as it was");
            }
        };
        same_as_before(&mut widgets, &mut text, "Navy");
        for (name, rgb) in &tint::PRESETS[1..] {
            tint::set(*rgb);
            let mut made = every_widget(&mut ctx, &note);
            same_as_before(&mut made, &mut text, name);
        }

        tint::set(tint::BASE);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
