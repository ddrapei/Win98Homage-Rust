//! A skin, as build.rs compiled it from its .ini, and the little it takes to
//! draw one and to find what a click landed on.
//!
//! The tables are the layout.  What Rainmeter would resolve at run time from
//! variables -- a meter's text, a colour, whether it is hidden, where a
//! moving one sits -- is asked of the widget through [`Hooks`], and
//! the widget answers from plain fields.  Meters whose geometry itself is
//! computed (`Kind::Custom`) are handed to the widget whole.

use crate::canvas::{Canvas, Rect};
use crate::text::{Spec, Text};

pub struct Skin {
    pub w: i32,
    pub h: i32,
    pub meters: &'static [Meter],
    /// The [Rainmeter] section's own mouse actions: what the wheel does
    /// anywhere no meter claims it.
    pub acts: &'static [Act],
    pub vars: &'static [(&'static str, &'static str)],
}

pub struct Meter {
    pub name: &'static str,
    pub x: i32,
    pub y: i32,
    /// Position formulas, for the meters that move; empty for the rest.
    pub xf: &'static str,
    pub yf: &'static str,
    /// x0, y0, x1, y1: what a partial repaint tests against, and what a click
    /// has to land in.
    pub b: [i32; 4],
    /// The Hidden= formula, empty for a meter that is always shown.
    pub hidden: &'static str,
    pub kind: Kind,
    pub acts: &'static [Act],
}

pub enum Kind {
    Shape(&'static [Sh]),
    Text(TextM),
    Custom,
}

pub enum Sh {
    /// x, y, w, h
    Rect(i32, i32, i32, i32, Col),
    /// x, y, w, h, left colour, right colour
    Grad(i32, i32, i32, i32, u32, u32),
    /// x0, y, x1, width, colour, dash on, dash off
    Line(f32, f32, f32, f32, Col, f32, f32),
    /// cx, cy, rx, ry
    Ellipse(f32, f32, f32, f32, Col),
    Poly(&'static [(f32, f32)], Col),
    /// A polygon turned clockwise by the degrees in a variable, about (cx, cy).
    Rot(&'static [(f32, f32)], Col, &'static str, f32, f32),
}

#[derive(Clone, Copy)]
pub enum Col {
    /// 0xAARRGGBB
    S(u32),
    /// The widget knows.
    V(&'static str),
}

pub struct TextM {
    pub src: Src,
    pub col: Col,
    pub font: usize,
    pub align: u8,
    pub w: i32,
    pub h: i32,
    pub clip: bool,
    pub solid: Option<Col>,
    pub pad: [i32; 4],
}

pub enum Src {
    L(&'static str),
    V(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev {
    Up,
    Dbl,
    WheelUp,
    WheelDown,
    Over,
    Leave,
}

pub struct Act {
    pub ev: Ev,
    /// The command the skin's mouse action names, or About / Close / OpenFile.
    pub cmd: &'static str,
    pub arg: i32,
}

include!(concat!(env!("OUT_DIR"), "/skins.rs"));

impl Skin {
    pub fn var(&self, name: &str) -> &'static str {
        self.vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or("", |(_, v)| v)
    }

    pub fn num(&self, name: &str, default: f64) -> f64 {
        self.var(name).trim().parse().unwrap_or(default)
    }

    pub fn int(&self, name: &str, default: i32) -> i32 {
        self.num(name, default as f64) as i32
    }

    /// An "R,G,B[,A]" variable as 0xAARRGGBB.
    pub fn colour(&self, name: &str) -> u32 {
        parse_colour(self.var(name)).unwrap_or(0xFF00_0000)
    }

    pub fn meter(&self, name: &str) -> Option<&'static Meter> {
        self.meters.iter().find(|m| m.name == name)
    }
}

pub fn parse_colour(s: &str) -> Option<u32> {
    let mut v = [0u32, 0, 0, 255];
    let mut n = 0;
    for part in s.split(',') {
        if n == 4 {
            return None;
        }
        v[n] = part.trim().parse::<f64>().ok()?.clamp(0.0, 255.0).round() as u32;
        n += 1;
    }
    (n >= 3).then_some(v[3] << 24 | v[0] << 16 | v[1] << 8 | v[2])
}

/// What a widget answers for its skin while it is drawn or clicked.
pub trait Hooks {
    /// A variable used in a Hidden= or position formula.
    fn num(&self, _var: &str) -> f64 {
        0.0
    }
    /// Append the value of a text variable.
    fn text(&self, _var: &str, _out: &mut String) {}
    fn colour(&self, _var: &str) -> u32 {
        0
    }
    /// Draw a meter the tables do not describe.  `x`, `y` is its origin.
    fn custom(&self, _meter: &Meter, _x: i32, _y: i32, _canvas: &mut Canvas, _text: &mut Text) {}
}

fn colour(c: Col, hooks: &dyn Hooks) -> u32 {
    match c {
        Col::S(v) => v,
        Col::V(name) => hooks.colour(name),
    }
}

fn bounds(m: &Meter) -> Rect {
    Rect::new(m.b[0], m.b[1], m.b[2], m.b[3])
}

fn is_hidden(m: &Meter, hooks: &dyn Hooks) -> bool {
    !m.hidden.is_empty() && formula(m.hidden, hooks) != 0.0
}

fn origin(m: &Meter, hooks: &dyn Hooks) -> (i32, i32) {
    let x = if m.xf.is_empty() { m.x } else { formula(m.xf, hooks) as i32 };
    let y = if m.yf.is_empty() { m.y } else { formula(m.yf, hooks) as i32 };
    (x, y)
}

/// Every meter that touches the canvas's clip, in the order the .ini lists
/// them: later ones paint over earlier ones, as in Rainmeter.
/// Where each text meter was when it was last drawn, so that repainting part
/// of the window does not lay out text that is nowhere near that part.  Text
/// is the expensive thing to draw, and the clock is repainted every second.
pub type Seen = Vec<Rect>;

/// "Not known": drawn the next time anything near it might be.
const ANYWHERE: Rect = Rect::new(-30000, -30000, 30000, 30000);

pub fn draw(skin: &Skin, hooks: &dyn Hooks, canvas: &mut Canvas, text: &mut Text, seen: &mut Seen) {
    let mut buf = String::new();
    seen.resize(skin.meters.len(), ANYWHERE);
    for (i, m) in skin.meters.iter().enumerate() {
        if !bounds(m).intersects(&canvas.clip) || is_hidden(m, hooks) {
            continue;
        }
        let is_text = matches!(m.kind, Kind::Text(_));
        // a few pixels of grace: a glyph may overhang the box it is set in
        let last = seen[i];
        if is_text && !Rect::new(last.x0 - 4, last.y0 - 4, last.x1 + 4, last.y1 + 4).intersects(&canvas.clip) {
            continue;
        }
        let (x, y) = origin(m, hooks);
        let rect = draw_meter(m, x, y, hooks, canvas, text, &mut buf);
        if is_text {
            // a meter with nothing in it has no size yet: it could be anywhere
            seen[i] = if rect.is_empty() { ANYWHERE } else { rect };
        }
    }
}

/// One meter with its origin at (x, y).  For a text meter, the rectangle it
/// took up.
pub fn draw_meter(m: &Meter, x: i32, y: i32, hooks: &dyn Hooks, canvas: &mut Canvas, text: &mut Text, buf: &mut String) -> Rect {
    {
        match &m.kind {
            Kind::Shape(shapes) => {
                for s in *shapes {
                    match *s {
                        Sh::Rect(sx, sy, w, h, c) => canvas.fill(x + sx, y + sy, w, h, colour(c, hooks)),
                        Sh::Grad(sx, sy, w, h, c0, c1) => canvas.hgrad(x + sx, y + sy, w, h, c0, c1),
                        Sh::Line(x0, ly, x1, width, c, on, off) => {
                            canvas.hline(x as f32 + x0, y as f32 + ly, x as f32 + x1, width, on, off, colour(c, hooks))
                        }
                        Sh::Ellipse(cx, cy, rx, ry, c) => {
                            canvas.ellipse(x as f32 + cx, y as f32 + cy, rx, ry, colour(c, hooks))
                        }
                        Sh::Poly(pts, c) => {
                            let mut moved = [(0.0f32, 0.0f32); 16];
                            let n = pts.len().min(16);
                            for (d, s) in moved.iter_mut().zip(pts) {
                                *d = (s.0 + x as f32, s.1 + y as f32);
                            }
                            canvas.poly(&moved[..n], colour(c, hooks));
                        }
                        Sh::Rot(pts, c, var, cx, cy) => {
                            let (sin, cos) = (hooks.num(var) as f32).to_radians().sin_cos();
                            let mut turned = [(0.0f32, 0.0f32); 16];
                            let n = pts.len().min(16);
                            for (d, s) in turned.iter_mut().zip(pts) {
                                let (dx, dy) = (s.0 - cx, s.1 - cy);
                                *d = (x as f32 + cx + dx * cos - dy * sin, y as f32 + cy + dx * sin + dy * cos);
                            }
                            canvas.poly(&turned[..n], colour(c, hooks));
                        }
                    }
                }
            }
            Kind::Text(t) => {
                buf.clear();
                let string: &str = match t.src {
                    Src::L(s) => s,
                    Src::V(name) => {
                        hooks.text(name, buf);
                        buf.as_str()
                    }
                };
                return text.draw(canvas, &Spec {
                    text: string,
                    font: t.font,
                    colour: colour(t.col, hooks),
                    x,
                    y,
                    align: t.align,
                    w: t.w,
                    h: t.h,
                    clip: t.clip,
                    solid: t.solid.map(|c| colour(c, hooks)),
                    pad: t.pad,
                });
            }
            Kind::Custom => hooks.custom(m, x, y, canvas, text),
        }
    }
    Rect::EMPTY
}

/// The topmost shown meter under the point that has an action for `ev`, or
/// failing that the skin's own.  With it, the point relative to the meter.
pub fn hit(skin: &'static Skin, hooks: &dyn Hooks, x: i32, y: i32, ev: Ev) -> Option<(&'static Act, i32, i32)> {
    for m in skin.meters.iter().rev() {
        let Some(act) = m.acts.iter().find(|a| a.ev == ev) else { continue };
        if bounds(m).contains(x, y) && !is_hidden(m, hooks) {
            let (mx, my) = origin(m, hooks);
            return Some((act, x - mx, y - my));
        }
    }
    skin.acts.iter().find(|a| a.ev == ev).map(|a| (a, x, y))
}

/// The topmost shown meter under the point that reacts to the pointer
/// arriving, by index: what the hover tracking compares between moves.
pub fn hover(skin: &'static Skin, hooks: &dyn Hooks, x: i32, y: i32) -> Option<usize> {
    skin.meters.iter().enumerate().rev().find_map(|(i, m)| {
        (m.acts.iter().any(|a| a.ev == Ev::Over) && bounds(m).contains(x, y) && !is_hidden(m, hooks)).then_some(i)
    })
}

// ---------------------------------------------------------------------------
//  Formulas: the Hidden= and X=/Y= options that depend on run-time variables
// ---------------------------------------------------------------------------

/// `+ - * /`, parentheses, `a = b`, `c ? x : y` and `#Variable#`, which is all
/// the skins' formulas use.
pub fn formula(s: &str, hooks: &dyn Hooks) -> f64 {
    let mut p = Parser { s: s.as_bytes(), i: 0, hooks };
    p.ternary()
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    hooks: &'a dyn Hooks,
}

impl Parser<'_> {
    fn eat(&mut self, c: u8) -> bool {
        while self.i < self.s.len() && self.s[self.i] == b' ' {
            self.i += 1;
        }
        if self.i < self.s.len() && self.s[self.i] == c {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn ternary(&mut self) -> f64 {
        let cond = self.equality();
        if self.eat(b'?') {
            let yes = self.ternary();
            self.eat(b':');
            let no = self.ternary();
            if cond != 0.0 { yes } else { no }
        } else {
            cond
        }
    }

    fn equality(&mut self) -> f64 {
        let v = self.sum();
        if self.eat(b'=') {
            let w = self.sum();
            return if v == w { 1.0 } else { 0.0 };
        }
        v
    }

    fn sum(&mut self) -> f64 {
        let mut v = self.product();
        loop {
            if self.eat(b'+') {
                v += self.product();
            } else if self.eat(b'-') {
                v -= self.product();
            } else {
                return v;
            }
        }
    }

    fn product(&mut self) -> f64 {
        let mut v = self.factor();
        loop {
            if self.eat(b'*') {
                v *= self.factor();
            } else if self.eat(b'/') {
                v /= self.factor();
            } else {
                return v;
            }
        }
    }

    fn factor(&mut self) -> f64 {
        if self.eat(b'-') {
            return -self.factor();
        }
        if self.eat(b'(') {
            let v = self.ternary();
            self.eat(b')');
            return v;
        }
        if self.eat(b'#') {
            let start = self.i;
            while self.i < self.s.len() && self.s[self.i] != b'#' {
                self.i += 1;
            }
            let name = core::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
            self.i = (self.i + 1).min(self.s.len());
            return self.hooks.num(name);
        }
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            self.i += 1;
        }
        core::str::from_utf8(&self.s[start..self.i]).ok().and_then(|n| n.parse().ok()).unwrap_or(0.0)
    }
}

/// The digits a numbered variable ends in: "RowInk7" with prefix "RowInk" is 7.
pub fn index_of(var: &str, prefix: &str) -> Option<usize> {
    let rest = var.strip_prefix(prefix)?;
    (!rest.is_empty() && rest.bytes().all(|c| c.is_ascii_digit())).then(|| rest.parse().ok()).flatten()
}
