//! What the window host knows about a widget: a skin to draw, a tick once a
//! second, and commands -- the names the skins' mouse actions call.

use std::path::PathBuf;

use crate::canvas::Rect;
use crate::settings::Settings;
use crate::skin::{Hooks, Skin};

/// How much of the window a change touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dirty {
    None,
    Part(Rect),
    All,
}

impl Dirty {
    pub fn and(self, other: Dirty) -> Dirty {
        match (self, other) {
            (Dirty::All, _) | (_, Dirty::All) => Dirty::All,
            (Dirty::None, d) | (d, Dirty::None) => d,
            (Dirty::Part(a), Dirty::Part(b)) => Dirty::Part(a.union(&b)),
        }
    }
}

/// Something only the host can do, asked for by a command.
#[derive(Debug, PartialEq)]
pub enum Request {
    None,
    About,
    Close,
    Open(PathBuf),
    /// Put the edit box over the note, showing this line of the file at the
    /// top and this many wrapped rows into it.
    Edit { line: usize, down: usize },
    /// Finish the edit, as a click outside the box would.
    LeaveEdit,
}

pub struct Outcome {
    pub dirty: Dirty,
    pub request: Request,
}

impl Outcome {
    pub fn none() -> Outcome {
        Outcome { dirty: Dirty::None, request: Request::None }
    }
    pub fn all() -> Outcome {
        Outcome { dirty: Dirty::All, request: Request::None }
    }
    pub fn dirty(dirty: Dirty) -> Outcome {
        Outcome { dirty, request: Request::None }
    }
    pub fn request(request: Request) -> Outcome {
        Outcome { dirty: Dirty::None, request }
    }
}

/// The world a widget runs in.  Time is handed in rather than read, so the
/// test harness can render any moment.
pub struct Ctx<'a> {
    pub now: i64,
    pub settings: &'a mut Settings,
    /// The workstation is locked.
    pub locked: bool,
    /// When this user signed in to Windows, if it could be found out.
    pub signed_in: Option<i64>,
}

pub struct MenuItem {
    pub id: u32,
    pub label: &'static str,
    pub checked: bool,
}

pub trait Widget: Hooks {
    fn skin(&self) -> &'static Skin;
    fn about(&self) -> &'static str;
    /// Once a second.
    fn tick(&mut self, ctx: &mut Ctx) -> Dirty;
    /// A mouse action from the skin.  `x`, `y` are relative to the meter that
    /// carried it.
    fn command(&mut self, cmd: &str, arg: i32, x: i32, y: i32, ctx: &mut Ctx) -> Outcome;
    /// Its own entries for the right-click menu.
    fn menu(&self, _items: &mut Vec<MenuItem>) {}
    fn menu_command(&mut self, _id: u32, _ctx: &mut Ctx) -> Outcome {
        Outcome::none()
    }
    /// The program is closing, or the widget is.
    fn shutdown(&mut self, _ctx: &mut Ctx) {}
}
