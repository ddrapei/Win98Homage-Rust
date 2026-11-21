//! What the window host knows about a widget: a skin to draw, a tick once a
//! second -- and frames in between, for the one that asks -- and commands,
//! the names the skins' mouse actions call.  A widget with text to type on
//! takes the press that lands on it, asks for the keyboard, and is handed
//! what is typed until the keyboard goes elsewhere.

use std::path::PathBuf;

use crate::canvas::Rect;
use crate::keys::Key;
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
    /// Give the widget the keyboard: it is being typed on.
    TakeKeys,
    /// The widget is done with the keyboard, as a click elsewhere would say.
    DropKeys,
    /// The skin the widget is drawn from is another now (a photo frame
    /// turned to another shape): its window is made again, the size of that
    /// one, where it was.
    Reshape,
    /// Ask for a folder, starting from this one, with this line over the
    /// tree of folders: what is chosen comes back through `folder_chosen`.
    ChooseFolder(PathBuf, &'static str),
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

/// What a widget makes of the left button going down on it.
pub enum Pressed {
    /// Nothing of its own: dragging moves the window, and letting go fires
    /// the action of the meter underneath.
    No,
    /// The press is the widget's.  The pointer's moves go to `drag` until the
    /// button comes up, the window stays where it is, and no meter's action
    /// fires.
    Taken(Outcome),
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

impl MenuItem {
    /// The id of an entry that opens a submenu -- the entries after it, up
    /// to the next one with this id and no label, which closes it.
    pub const SUB: u32 = u32::MAX;

    pub fn new(id: u32, label: &'static str, checked: bool) -> MenuItem {
        MenuItem { id, label, checked }
    }

    /// A line between entries.
    pub fn line() -> MenuItem {
        MenuItem { id: 0, label: "", checked: false }
    }

    /// The submenu `label`: the entries that follow, up to `end_sub`.
    pub fn sub(label: &'static str) -> MenuItem {
        MenuItem { id: MenuItem::SUB, label, checked: false }
    }

    pub fn end_sub() -> MenuItem {
        MenuItem { id: MenuItem::SUB, label: "", checked: false }
    }
}

pub trait Widget: Hooks {
    fn skin(&self) -> &'static Skin;
    fn about(&self) -> &'static str;
    /// Once a second.
    fn tick(&mut self, ctx: &mut Ctx) -> Dirty;
    /// Milliseconds between frames, for a widget that moves more often than
    /// the tick; 0 for one that does not, or is not moving now.  The host
    /// runs a timer only while some widget asks for one.
    fn frame_ms(&self) -> u32 {
        0
    }
    /// One frame, `ms` after the last.
    fn frame(&mut self, _ms: u32) -> Dirty {
        Dirty::None
    }
    /// A mouse action from the skin.  `x`, `y` are relative to the meter that
    /// carried it.
    fn command(&mut self, cmd: &str, arg: i32, x: i32, y: i32, ctx: &mut Ctx) -> Outcome;
    /// The left button has gone down at this point of the window; `double`
    /// for the second press of a double-click, `shift` if Shift is held.
    fn press(&mut self, _x: i32, _y: i32, _double: bool, _shift: bool) -> Pressed {
        Pressed::No
    }
    /// The pointer has moved to this point of the window, with a press the
    /// widget took still held.
    fn drag(&mut self, _x: i32, _y: i32) -> Dirty {
        Dirty::None
    }
    /// The button has come up at this point of the window, ending a press
    /// the widget took -- or the press has ended out of the window's sight,
    /// and the point is `i32::MIN` both ways, over nothing.
    fn release(&mut self, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        Outcome::none()
    }
    /// Whether the pointer is over text, and should look it.
    fn ibeam(&self, _x: i32, _y: i32) -> bool {
        false
    }

    // What follows is for a widget that is typed on.  It asks for the
    // keyboard with `Request::TakeKeys`, and has it until it gives it back
    // with `Request::DropKeys` or the host takes it away -- a click somewhere
    // else, another window coming forward.  Either way `blur` is the end.

    /// Whether a press at this point of the window leaves the typing going:
    /// it is on the text, or on something that only moves it.
    fn typing_at(&self, _x: i32, _y: i32) -> bool {
        false
    }
    /// One typed character.
    fn typed(&mut self, _c: char) -> Outcome {
        Outcome::none()
    }
    /// A key that is not a character.  Copy, Cut and Paste never arrive: the
    /// host has the clipboard, and calls the three below.
    fn key(&mut self, _key: Key, _shift: bool, _ctrl: bool) -> Outcome {
        Outcome::none()
    }
    /// What is selected, for the clipboard.
    fn copy(&self) -> Option<String> {
        None
    }
    /// What is selected, taken out of the text.
    fn cut(&mut self) -> (Option<String>, Outcome) {
        (None, Outcome::none())
    }
    fn paste(&mut self, _text: &str) -> Outcome {
        Outcome::none()
    }
    /// The keyboard's timer, `ms` after the last; `blink` if the caret is one
    /// that does.
    fn pulse(&mut self, _ms: u32, _blink: bool) -> Dirty {
        Dirty::None
    }
    /// The keyboard has gone elsewhere: the typing is over.
    fn blur(&mut self, _ctx: &mut Ctx) -> Dirty {
        Dirty::None
    }

    /// The tint has changed: read again every colour taken from the skin,
    /// and make again whatever was drawn from them.  The host redraws the
    /// whole window after.  (The skin's own tables need nothing: they are
    /// tinted as they are drawn.)
    fn retint(&mut self) {}

    /// Whether it holds something a press anywhere outside its frame ends --
    /// a selection, as a press off a Win98 desktop icon ends that one.  For
    /// as long as any widget says so, the host watches every press on the
    /// screen (and no longer: that costs every click a call into this
    /// process).
    fn watches_presses(&self) -> bool {
        false
    }
    /// A mouse button has gone down outside its frame, anywhere on the
    /// screen -- another widget, the desktop, another program.
    fn pressed_elsewhere(&mut self) -> Dirty {
        Dirty::None
    }
    /// The right button has come up at this point of the window, and its
    /// menu is about to be shown: what the menu says may be about what is
    /// there.
    fn context(&mut self, _x: i32, _y: i32) -> Dirty {
        Dirty::None
    }
    /// Its own entries for the right-click menu; one with id 0 is a line
    /// between them, and `MenuItem::SUB` opens a submenu and closes it.
    fn menu(&self, _items: &mut Vec<MenuItem>) {}
    /// A picture to show beside one of its own entries, as near `px` px
    /// square as it has one: its side, and its premultiplied BGRA.
    fn menu_picture(&self, _id: u32, _px: i32) -> Option<(i32, &'static [u8])> {
        None
    }
    fn menu_command(&mut self, _id: u32, _ctx: &mut Ctx) -> Outcome {
        Outcome::none()
    }
    /// The folder chosen after it asked for one (`Request::ChooseFolder`).
    fn folder_chosen(&mut self, _folder: PathBuf, _ctx: &mut Ctx) -> Dirty {
        Dirty::None
    }
    /// The program is closing, or the widget is.
    fn shutdown(&mut self, _ctx: &mut Ctx) {}
}
