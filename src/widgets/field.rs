//! A city field: one line of text in a sunken box, typed on where it stands.
//! The weather has one and the world clock has three.
//!
//! There is one picture of it -- a name, centred in its box -- and while it
//! has the keyboard a selection and a caret are drawn on top.  Nothing is
//! laid over it and the name does not move when the typing starts.  The
//! text, the caret and what the keys do are the note's own (`editor::Doc`,
//! as one line); the keyboard reaches it the way it reaches the note
//! (`keys`), through the widget it is part of.
//!
//!   click    the field takes the keyboard, the name in it all selected
//!   Enter    take the name as it stands
//!   click anywhere else
//!            take it, if it is not what the typing started from
//!   Escape   leave things as they were
//!
//! It knows nothing of what the name is for.  The skin says where it is --
//! a meter that covers the inside of the box -- and how it looks: CityInset,
//! CityDrop, SelectBg and SelectInk, beside the font and the ink.

use crate::canvas::{Canvas, Rect};
use crate::editor::{self, Doc};
use crate::keys::Key;
use crate::skin::{self, Skin};
use crate::text::{Spec, Text, CENTER};
use crate::widget::{Dirty, Outcome, Request};

/// Characters the field takes: no place name is longer.
pub const LIMIT: usize = 64;
/// Px of sunken edge round the field that are still the field to a click.
const BEVEL: i32 = 2;
/// The height of the name's line, as the skins' other one-line meters have it.
const LINE_H: i32 = 14;

/// A fractional x as the pixel column it falls on.
fn px(x: f32) -> i32 {
    (x + 0.5).floor() as i32
}

/// Px from the start of `s` to each place a caret can stand in it: one more
/// entry than it has characters.  They are the font's own advances, so a
/// caret, a selection and a centred name all land where the glyphs do.
fn stops(text: Option<&mut Text>, font: usize, s: &str) -> Vec<f32> {
    let mut each = Vec::new();
    match text {
        Some(text) => text.advances(font, s, &mut each),
        // nothing to measure with: the width of a digit, as good a guess as any
        None => each.extend(s.chars().map(|_| 6.0)),
    }
    let mut at = 0.0;
    let mut out = Vec::with_capacity(each.len() + 1);
    out.push(at);
    for w in each {
        at += w;
        out.push(at);
    }
    out
}

/// The colours a field draws in, from its skin, in the tint: the ink, and
/// what is selected with the ink on it.
fn colours(skin: &Skin) -> [u32; 3] {
    ["Ink", "SelectBg", "SelectInk"].map(|c| skin.colour(c))
}

/// One city field.  It draws whatever name it is handed while nobody is
/// typing, and its own text while somebody is -- by the same code, from the
/// same x, so the one turns into the other without a pixel moving.
pub struct Field {
    /// The inside of the box, in the window: what is drawn on.
    pub rect: Rect,
    /// Px between its sides and the column the name is centred in.
    inset: i32,
    /// Px from its top to the top of the name's line.
    down: i32,
    /// The skin it is part of, and the colours it draws in from it: the ink,
    /// and what is selected with the ink on it.
    skin: &'static Skin,
    ink: u32,
    select_bg: u32,
    select_ink: u32,
    /// The font the name is drawn in: its face, then as the host's `Text`
    /// numbers it, and its size in points.
    face: &'static str,
    font: usize,
    points: f32,

    /// The field has the keyboard.
    pub editing: bool,
    doc: Doc,
    /// What the typing started from.
    before: String,
    /// Enter was pressed: the name is wanted as it stands.
    entered: bool,
    /// Escape was: it is not wanted at all.
    cancelled: bool,
    /// The press now held put the caret down, so dragging it selects.
    sweeping: bool,
    caret_on: bool,
    /// The same font, to measure with, for as long as the typing lasts.
    measure: Option<Text>,
    /// `stops` of the text, and the revision of it they were measured from.
    stops: Vec<f32>,
    laid: u64,
    /// The text fits the column, and is centred in it; otherwise it runs
    /// from the left and scrolls to keep the caret in sight.
    fits: bool,
    /// Px from the left of the column to the start of the text, either way.
    /// Centred, that is wherever the middle puts it, between pixels or not.
    left: f32,
}

impl Field {
    /// The field whose inside is the meter of this name in `skin`.
    pub fn new(skin: &'static Skin, meter: &str) -> Field {
        let points = skin.num("FontSize", 8.0) as f32;
        let [ink, select_bg, select_ink] = colours(skin);
        Field {
            // the meter is the inside of the box; its bounds are its place
            rect: skin.meter(meter).map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3])),
            inset: skin.int("CityInset", 2),
            down: skin.int("CityDrop", 3) + skin.int("TextDY", 0),
            skin,
            ink,
            select_bg,
            select_ink,
            face: skin.var("Font"),
            font: skin::FONTS.iter().position(|&(pt, bold)| pt == points && !bold).unwrap_or(0),
            points,
            editing: false,
            doc: Doc::one_line(),
            before: String::new(),
            entered: false,
            cancelled: false,
            sweeping: false,
            caret_on: false,
            measure: None,
            stops: Vec::new(),
            laid: 0,
            fits: true,
            left: 0.0,
        }
    }

    /// The tint has changed: its colours again, from its skin.
    pub fn retint(&mut self) {
        [self.ink, self.select_bg, self.select_ink] = colours(self.skin);
    }

    /// The column the name is centred in: its left edge in the window, and
    /// its width.
    fn column(&self) -> (i32, i32) {
        (self.rect.x0 + self.inset, (self.rect.x1 - self.rect.x0 - 2 * self.inset).max(1))
    }

    /// The box with its sunken edge: what a click has to land in.
    pub fn reach(&self) -> Rect {
        let r = &self.rect;
        if r.is_empty() { *r } else { Rect::new(r.x0 - BEVEL, r.y0 - BEVEL, r.x1 + BEVEL, r.y1 + BEVEL) }
    }

    pub fn text(&self) -> String {
        self.doc.text().iter().collect()
    }

    /// Start typing, from `name` with all of it selected, so that typing
    /// replaces it.  The answer asks the host for the keyboard.
    pub fn begin(&mut self, name: &str) -> Outcome {
        self.doc.load(name.chars().filter(|c| !c.is_control()).take(LIMIT).collect());
        self.doc.select_all();
        self.before = self.text().trim().to_owned();
        self.editing = true;
        self.entered = false;
        self.cancelled = false;
        self.sweeping = false;
        if self.measure.is_none() {
            self.measure = Text::new(self.face, &[(self.points, false)]);
        }
        self.stops.clear();
        self.left = 0.0;
        Outcome { request: Request::TakeKeys, ..self.edited() }
    }

    /// The typing is over.  What was typed, if it is an answer: Enter takes
    /// the name as it stands, a click elsewhere takes it only if it is not
    /// what the typing started from, and Escape never does.
    pub fn end(&mut self) -> Option<String> {
        self.editing = false;
        self.measure = None;
        self.stops = Vec::new();
        self.doc.deselect();
        let typed = self.text().trim().to_owned();
        (!self.cancelled && (self.entered || typed != self.before)).then_some(typed)
    }

    /// The characters there is still room for, what is selected counting as
    /// already gone.
    fn free(&self) -> usize {
        let (a, b) = self.doc.range();
        LIMIT.saturating_sub(self.doc.text().len() - (b - a))
    }

    /// After anything the keyboard or the pointer did: measure the text
    /// again if it changed, find where it starts, and draw.
    fn edited(&mut self) -> Outcome {
        if self.doc.rev() != self.laid || self.stops.is_empty() {
            let text = self.text();
            self.stops = stops(self.measure.as_mut(), 0, &text);
            self.laid = self.doc.rev();
        }
        self.caret_on = true;

        let (_, room) = self.column();
        let chars = self.doc.text();
        // blanks at the end hang past the name: they do not pull it off centre
        let inked = chars.len() - chars.iter().rev().take_while(|&&c| editor::blank(c)).count();
        let width = self.stops[inked];
        self.fits = width <= room as f32;
        if self.fits {
            // where the layout's own centring puts it (see `draw`): the
            // widths are whole px, and the half an odd room leaves is
            // dropped, as GDI drops it
            self.left = ((room as f32 - width) / 2.0).floor();
        } else {
            // Too long to show whole: it runs from the left edge and scrolls,
            // no further than leaves a pixel for the caret after its end, and
            // always far enough to show the caret.
            let caret = px(self.stops[self.doc.caret()]);
            let full = px(self.stops[chars.len()]);
            let scroll = (-self.left as i32).clamp(0, (full + 1 - room).max(0)).clamp(caret - (room - 1), caret);
            self.left = -scroll as f32;
        }
        Outcome::dirty(Dirty::Part(self.rect))
    }

    /// The pixel column of the window that caret position `i` falls on.
    fn column_of(&self, i: usize) -> i32 {
        self.column().0 + px(self.left + self.stops.get(i).copied().unwrap_or(0.0))
    }

    /// The place in the text nearest an x of the window.
    fn place_at(&self, x: i32) -> usize {
        let at = (x - self.column().0) as f32 - self.left;
        let mut best = 0;
        for (i, stop) in self.stops.iter().enumerate() {
            if (stop - at).abs() < (self.stops[best] - at).abs() {
                best = i;
            }
        }
        best
    }

    /// Where the caret is drawn, in the window.  After blanks that run past
    /// the column it waits at the column's edge.
    fn caret_box(&self) -> Rect {
        let (x, room) = self.column();
        let cx = self.column_of(self.doc.caret()).clamp(x, x + room - 1);
        Rect::new(cx, self.rect.y0 + 1, cx + 1, self.rect.y1 - 1)
    }

    // ---- the pointer ------------------------------------------------------------

    /// A press in the field while it is being typed in: the caret goes there,
    /// or with Shift the selection reaches to it; a double-click takes a word.
    pub fn press(&mut self, x: i32, double: bool, shift: bool) -> Outcome {
        let pos = self.place_at(x);
        if double {
            self.doc.select_word(pos);
        } else {
            self.doc.click(pos, false, shift);
        }
        self.sweeping = !double;
        self.edited()
    }

    /// The selection follows the pointer, and the name scrolls under it.
    pub fn drag(&mut self, x: i32) -> Dirty {
        if !self.editing || !self.sweeping {
            return Dirty::None;
        }
        self.doc.drag(self.place_at(x), false);
        self.edited().dirty
    }

    // ---- the keyboard -----------------------------------------------------------

    pub fn typed(&mut self, c: char) -> Outcome {
        if !self.editing || c.is_control() || self.free() == 0 {
            return Outcome::none();
        }
        self.doc.type_char(c);
        self.edited()
    }

    pub fn key(&mut self, key: Key, shift: bool, ctrl: bool) -> Outcome {
        if !self.editing {
            return Outcome::none();
        }
        match key {
            // one line: up and down are along it
            Key::Left | Key::Up => self.doc.left(ctrl, shift),
            Key::Right | Key::Down => self.doc.right(ctrl, shift),
            Key::Home | Key::PageUp => self.doc.home(&[], true, shift),
            Key::End | Key::PageDown => self.doc.end(&[], true, shift),
            Key::Backspace => self.doc.backspace(ctrl),
            Key::Delete => self.doc.delete(ctrl),
            Key::SelectAll => self.doc.select_all(),
            Key::Undo => {
                self.doc.undo();
            }
            Key::Redo => {
                self.doc.redo();
            }
            Key::Enter => {
                self.entered = true;
                return Outcome::request(Request::DropKeys);
            }
            Key::Escape => {
                self.cancelled = true;
                return Outcome::request(Request::DropKeys);
            }
            // a tab has no place in a name; the clipboard's three are the host's
            Key::Tab | Key::Copy | Key::Cut | Key::Paste => return Outcome::none(),
        }
        self.edited()
    }

    pub fn copy(&self) -> Option<String> {
        self.doc.selection().filter(|_| self.editing)
    }

    pub fn cut(&mut self) -> (Option<String>, Outcome) {
        let Some(taken) = self.copy() else { return (None, Outcome::none()) };
        self.doc.backspace(false);
        (Some(taken), self.edited())
    }

    /// A name is one line: of what is pasted, the first that has anything on
    /// it, and as much of that as there is room for.
    pub fn paste(&mut self, text: &str) -> Outcome {
        if !self.editing {
            return Outcome::none();
        }
        let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let name: String = line.chars().filter(|c| !c.is_control()).take(self.free()).collect();
        if name.is_empty() {
            return Outcome::none();
        }
        self.doc.insert(&name);
        self.edited()
    }

    /// The keyboard's timer: the caret blinks, if it is one that does.
    pub fn pulse(&mut self, blink: bool) -> Dirty {
        let on = !blink || !self.caret_on;
        if !self.editing || on == self.caret_on {
            return Dirty::None;
        }
        self.caret_on = on;
        Dirty::Part(self.caret_box())
    }

    // ---- drawing ----------------------------------------------------------------

    /// The name in the box: `resting` while nobody is typing, the text being
    /// typed while somebody is -- with whatever of it is selected drawn again
    /// light on dark, and the caret last.
    pub fn draw(&self, resting: &str, canvas: &mut Canvas, text: &mut Text) {
        let outer = canvas.clip;
        let clip = outer.intersect(&self.rect);
        if clip.is_empty() {
            return;
        }
        if !self.editing {
            self.run(canvas, text, resting, true, self.ink);
            return;
        }
        if self.stops.len() != self.doc.text().len() + 1 {
            return;
        }

        canvas.clip = clip;
        let string = self.text();
        self.run(canvas, text, &string, self.fits, self.ink);

        let (a, b) = self.doc.range();
        let band = Rect::new(self.column_of(a), self.rect.y0 + 1, self.column_of(b), self.rect.y1 - 1).intersect(&clip);
        if !band.is_empty() {
            canvas.fill(band.x0, band.y0, band.x1 - band.x0, band.y1 - band.y0, self.select_bg);
            canvas.clip = band;
            self.run(canvas, text, &string, self.fits, self.select_ink);
            canvas.clip = clip;
        }
        if self.caret_on {
            let c = self.caret_box();
            // at the head of a selection it stands on the dark
            let colour = if a != b && self.doc.caret() == a { self.select_ink } else { self.ink };
            canvas.fill(c.x0, c.y0, 1, c.y1 - c.y0, colour);
        }
        canvas.clip = outer;
    }

    /// The name's glyphs, in one colour.
    ///
    /// `centred` is a name that fits, or one nobody is typing: it is laid out
    /// centred in the column, exactly as a String meter with StringAlign=Center
    /// and ClipString=1 would be -- so a name too long to show whole ends in
    /// an ellipsis, and a name looks the same at rest and with a caret in it.
    /// Otherwise it is being typed and is too long: it runs from `left`.
    fn run(&self, canvas: &mut Canvas, text: &mut Text, string: &str, centred: bool, colour: u32) {
        if string.is_empty() {
            return;
        }
        let (x, room) = self.column();
        let spec = Spec { text: string, font: self.font, colour, x, y: self.rect.y0 + self.down, align: 0, w: room, h: LINE_H, clip: false, solid: None, pad: [0; 4] };
        text.draw(canvas, &if centred {
            Spec { x: x + room / 2, align: CENTER, clip: true, ..spec }
        } else {
            // wide enough never to matter: the canvas's clip is what ends it
            Spec { x: x + self.left as i32, w: 4000, ..spec }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The weather's.
    fn field() -> Field {
        Field::new(&skin::weather::SKIN, "MeterCity")
    }

    fn type_in(field: &mut Field, s: &str) {
        for c in s.chars() {
            field.typed(c);
        }
    }

    #[test]
    fn typing_in_the_field_replaces_the_name_and_enter_takes_it() {
        let mut f = field();
        assert!(matches!(f.begin("Lviv, UA"), Outcome { request: Request::TakeKeys, .. }));
        // all of it selected, so that typing replaces it
        assert_eq!(f.doc.range(), (0, 8));
        type_in(&mut f, "Kyiv");
        assert_eq!(f.text(), "Kyiv");
        assert!(matches!(f.key(Key::Enter, false, false), Outcome { request: Request::DropKeys, .. }));
        assert_eq!(f.end().as_deref(), Some("Kyiv"));
        assert!(!f.editing);
    }

    #[test]
    fn a_name_left_as_it_was_is_an_answer_only_to_enter() {
        let mut f = field();
        // clicked in, clicked away
        f.begin("Lviv, UA");
        assert_eq!(f.end(), None);
        // typed on and put back
        f.begin("Lviv, UA");
        type_in(&mut f, "x");
        f.key(Key::Undo, false, false);
        assert_eq!(f.text(), "Lviv, UA");
        assert_eq!(f.end(), None);
        f.begin("Lviv, UA");
        f.key(Key::Enter, false, false);
        assert_eq!(f.end().as_deref(), Some("Lviv, UA"));
        // Escape takes nothing, whatever was typed
        f.begin("Lviv, UA");
        type_in(&mut f, "Kyiv");
        f.key(Key::Escape, false, false);
        assert_eq!(f.end(), None);
        // emptied: an answer, and the one that unpins the place
        f.begin("Lviv, UA");
        f.key(Key::Delete, false, false);
        assert_eq!(f.end().as_deref(), Some(""));
    }

    #[test]
    fn the_field_takes_one_line_and_no_more_than_a_name() {
        let mut f = field();
        f.begin("");
        f.paste("\r\n  \r\nNew York\r\nLondon");
        assert_eq!(f.text(), "New York");
        type_in(&mut f, &"x".repeat(100));
        assert_eq!(f.doc.text().len(), LIMIT);
        // what is selected makes room for what replaces it
        f.key(Key::SelectAll, false, false);
        f.paste(&"y".repeat(100));
        assert_eq!(f.text(), "y".repeat(LIMIT));
        // brackets are brackets here, and a tab is nothing
        f.key(Key::SelectAll, false, false);
        type_in(&mut f, "[ ] x\t");
        assert_eq!(f.text(), "[ ] x");
    }

    #[test]
    fn a_short_name_is_centred_and_a_click_finds_its_letters() {
        let mut f = field();
        f.begin("Lviv");
        let (x, room) = f.column();
        assert!(f.fits && f.left > 0.0);
        // as much room to its left as to its right
        assert!((f.column_of(0) - x - (x + room - f.column_of(4))).abs() <= 1);
        // the press that started the typing selected the name: moving the
        // pointer before letting go does not undo that
        assert_eq!(f.drag(f.column_of(1)), Dirty::None);
        assert_eq!(f.doc.range(), (0, 4));
        f.press(f.column_of(2), false, false);
        assert_eq!((f.doc.caret(), f.doc.range()), (2, (2, 2)));
        f.drag(f.column_of(4));
        assert_eq!(f.copy().as_deref(), Some("iv"));
        f.press(f.column_of(1), true, false);
        assert_eq!(f.copy().as_deref(), Some("Lviv"));
    }

    #[test]
    fn a_name_too_long_for_the_field_scrolls_under_the_caret() {
        let mut f = field();
        f.begin("");
        type_in(&mut f, "Llanfairpwllgwyngyllgogerychwyrndrobwllllantysiliogogogoch");
        let (x, room) = f.column();
        assert!(!f.fits && f.left < 0.0);
        // the end, where the caret is, is in sight
        assert!((x..x + room).contains(&f.caret_box().x0));
        f.key(Key::Home, false, false);
        assert_eq!((f.left, f.caret_box().x0), (0.0, x));
        // and back again, a word at a time
        f.key(Key::End, false, false);
        assert!(f.left < 0.0 && (x..x + room).contains(&f.caret_box().x0));
        // a click puts the caret under the pointer, wherever the name has got to
        f.press(x + 40, false, false);
        assert!((f.caret_box().x0 - (x + 40)).abs() <= 4);
    }
}
