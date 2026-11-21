//! Win98 Widgets -- thirteen Windows 98 style desktop widgets as one small
//! native program.
//!
//! Date/Time Properties, Resource Meter, Sticky Note, Weather, Rain Window,
//! Disk Space, World Clock, Battery, Countdown, Claude Usage, Photo Frame,
//! Folders and Flip Clock, drawn from the .ini files in `skins/` -- with no
//! Direct2D and no script engine underneath.  One of each, and as many more Sticky Notes, Countdowns,
//! Photo Frames and Folders as are asked for.
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
//!   widgets/       what a widget is, and the thirteen there are
//!     widget         what the host asks of a widget -- the one thing about
//!                    them it knows
//!     field          a one-line field, typed on where it stands: the
//!                    weather's city, the world clock's three, a countdown's
//!                    name, a folder's name
//!     calendar       a month calendar with its month list: the Date/Time
//!                    widget's and the countdown's
//!     datetime       Date/Time Properties
//!     resources      Resource Meter
//!     notes          Sticky Note
//!     weather        Weather          (and the child that fetches it)
//!     rain           Rain Window      (animated, while it rains)
//!     disk           Disk Space
//!     world          World Clock      (and the child that finds a city's
//!                    time zone)
//!     battery        Battery
//!     countdown      Countdown
//!     claude         Claude Usage     (and the child that asks for it)
//!     photo          Photo Frame
//!     folders        Folders
//!     flip           Flip Clock       (animated, as a card falls; the one
//!                    widget with no window)
//!
//!   input/        typing
//!     keys           the window that holds the keyboard for a widget
//!     editor         a text and what the keys do to it: the note's, and a
//!                    one-line field's
//!
//!   net/           the network, which only a child ever touches
//!     geocode        a city by name: where it is, and its time zone
//!     http           one GET over WinHTTP
//!
//!   storage/       what is kept on disk
//!     settings       settings.ini
//!     usage          the on/unlocked journal the Date/Time graph is drawn from
//!
//!   graphics/      drawing
//!     skin           the compiled tables, and drawing and hit-testing them
//!     canvas         the software canvas they are drawn on
//!     text           DirectWrite text, rasterised into the canvas
//!     decode         a photo read by WIC and brought down small
//!     figures        3x5 pixel figures, for a ruler's numbers
//!     tint           the colour the user picked, and every colour turned
//!                    to it
//!
//!   platform/      Windows
//!     sys            the Win32 declarations all of it uses
//!     screens        the monitors, and keeping a window on them
//!     grid           the margin and the gap: snapping to them, and
//!                    re-spacing a layout when they change
//!     clock          time and time zones
//! ```

#![windows_subsystem = "windows"]

mod app;
mod graphics;
mod input;
mod net;
mod platform;
mod storage;
mod tools;
mod widgets;

// Each module is also known by its own name here at the root, so the code
// says `crate::canvas`, not `crate::graphics::canvas`.  Which folder a file
// is in says what it is for; moving one changes these lines and nothing in
// the file, or in any file that uses it.
use graphics::{canvas, decode, figures, skin, text, tint};
use input::{editor, keys};
use net::{geocode, http};
use platform::{clock, grid, screens, sys};
use storage::{settings, usage};
use tools::dump;
use widgets::{battery, calendar, claude, countdown, datetime, disk, field, flip, folders, notes, photo, rain, resources, weather, widget, world};

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
        // the short-lived children that talk to the network
        Some("--fetch-weather") => weather::fetch_main(&args[2..]),
        Some("--find-city") => weather::find_main(&args[2..]),
        Some("--find-zone") => world::find_main(&args[2..]),
        Some("--fetch-claude") => claude::fetch_main(&args[2..]),
        Some("--dump") => dump::run(&args[2..]),
        _ => app::run(),
    };
    std::process::exit(code);
}
