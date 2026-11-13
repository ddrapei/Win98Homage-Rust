//! Win98 Widgets -- five Windows 98 style desktop widgets as one small native
//! program.
//!
//! Date/Time Properties, Resource Meter, Sticky Note, Weather and Rain Window,
//! drawn from the .ini files in `skins/` -- with no Direct2D and no script
//! engine underneath.  One of each, and as many more Sticky Notes as are
//! asked for.
//!
//! # Where things are
//!
//! The files are in folders by what they are for, and each folder leans only
//! on the ones listed under it here:
//!
//! ```text
//!   build.rs       the .ini files in skins/ compiled into static tables
//!
//!   main.rs        this: the way in, and the list of modules
//!   app.rs         the host: windows, mouse, menus, tray, the once-a-second
//!                  tick and the frames in between
//!
//!   tools/         for working on the program
//!     dump           off-screen rendering, for checking a change
//!
//!   widgets/       what a widget is, and the five there are
//!     widget         what the host asks of a widget -- the one thing about
//!                    them it knows
//!     datetime       Date/Time Properties
//!     resources      Resource Meter
//!     notes          Sticky Note
//!     weather        Weather          (and the child that fetches it)
//!     rain           Rain Window      (the one that is animated)
//!
//!   input/        typing
//!     keys           the window that holds the keyboard for a widget
//!     editor         a text and what the keys do to it: the note's, and the
//!                    weather's city field
//!
//!   storage/       what is kept on disk
//!     settings       settings.ini
//!     usage          the on/unlocked journal the Date/Time graph is drawn from
//!
//!   graphics/      drawing
//!     skin           the compiled tables, and drawing and hit-testing them
//!     canvas         the software canvas they are drawn on
//!     text           DirectWrite text, rasterised into the canvas
//!
//!   platform/      Windows
//!     sys            the Win32 declarations all of it uses
//!     clock          time and time zones
//! ```

#![windows_subsystem = "windows"]

mod app;
mod graphics;
mod input;
mod platform;
mod storage;
mod tools;
mod widgets;

// Each module is also known by its own name here at the root, so the code
// says `crate::canvas`, not `crate::graphics::canvas`.  Which folder a file
// is in says what it is for; moving one changes these lines and nothing in
// the file, or in any file that uses it.
use graphics::{canvas, skin, text};
use input::{editor, keys};
use platform::{clock, sys};
use storage::{settings, usage};
use tools::dump;
use widgets::{datetime, notes, rain, resources, weather, widget};

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
