//! Win98 Widgets -- five Windows 98 style desktop widgets as one small native
//! program.
//!
//! Date/Time Properties, Resource Meter, Sticky Note, Weather and Rain Window,
//! drawn from the .ini files in `skins/` -- with no Direct2D and no script
//! engine underneath.
//!
//! # Where things are
//!
//! ```text
//!   build.rs     the .ini files in skins/ compiled into static tables
//!   skin         those tables, and drawing and hit-testing them
//!   canvas       the software canvas they are drawn on
//!   text         DirectWrite text, rasterised into the canvas
//!   widget       what the host asks of a widget
//!   datetime     Date/Time Properties  (with usage: the on/unlocked journal)
//!   resources    Resource Meter
//!   notes        Sticky Note           (with editor: its text and what the
//!                                       keys do to it; keys: the window that
//!                                       holds the keyboard for it)
//!   weather      Weather               (and the child that fetches it; entry:
//!                                       the box the city is typed in)
//!   rain         Rain Window           (the one that is animated)
//!   app          windows, mouse, menus, tray, the once-a-second tick and
//!                the frames in between
//!   settings     settings.ini
//!   clock        time and time zones
//!   sys          the Win32 declarations all of it uses
//!   dump         off-screen rendering, for checking a change
//! ```

#![windows_subsystem = "windows"]

mod app;
mod canvas;
mod clock;
mod datetime;
mod dump;
mod editor;
mod entry;
mod keys;
mod notes;
mod rain;
mod resources;
mod settings;
mod skin;
mod sys;
mod text;
mod usage;
mod weather;
mod widget;

fn main() {
    // There is no console to complain to: if something does go wrong, leave
    // what and where beside the weather cache.
    std::panic::set_hook(Box::new(|info| {
        let dir = settings::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("crash.txt"), info.to_string());
    }));
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        // the short-lived child that talks to the network
        Some("--fetch-weather") => weather::fetch_main(&args[2..]),
        Some("--find-city") => weather::find_main(&args[2..]),
        Some("--dump") => dump::run(&args[2..]),
        _ => app::run(),
    };
    std::process::exit(code);
}
