//! Compiles the five widgets' skins into static Rust tables.
//!
//! The .ini files in `skins/` are the single source of truth for every
//! pixel: each meter's position, shapes, colours, text options and mouse
//! actions are read here, at build time, and written out as constants that
//! `src/skin.rs` includes.  Nothing parses an .ini while the program runs.
//!
//! What is resolved here: every `#Variable#` that only ever has the value the
//! .ini gives it (the palette, `Pad`, the layout figures).  What is left for
//! the program: the variables that change at run time -- those stay in the
//! tables by name (`Col::V("NoteBg")`, `Src::V("TimeText")`, a `hidden` or
//! position formula), and the widget's own module in `src/` answers for
//! them.  A meter whose *geometry* depends on one (the clock hands, the usage
//! bars, the scrollbar thumb) is emitted as `Kind::Custom` and drawn by hand.
//!
//! The weather icons are decoded here as well, into premultiplied BGRA, so the

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

// ---------------------------------------------------------------------------
//  .ini reading
// ---------------------------------------------------------------------------

struct Section {
    name: String,
    /// (lower-cased key, value) in file order; the first of a duplicate wins.
    opts: Vec<(String, String)>,
}

impl Section {
    fn get(&self, key: &str) -> Option<&str> {
        self.opts.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn read_text(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let wide: Vec<u16> =
            bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&wide)
    } else {
        let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        String::from_utf8_lossy(body).into_owned()
    }
}

fn parse_ini(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && !line.contains('=') {
            out.push(Section { name: line[1..line.len() - 1].to_string(), opts: Vec::new() });
        } else if let (Some(eq), Some(sec)) = (line.find('='), out.last_mut()) {
            let key = line[..eq].trim().to_ascii_lowercase();
            let val = line[eq + 1..].trim().to_string();
            if !sec.opts.iter().any(|(k, _)| *k == key) {
                sec.opts.push((key, val));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
//  Variables and formulas
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    vars: HashMap<String, String>,
    dynamic: &'a dyn Fn(&str) -> bool,
}

impl Ctx<'_> {
    /// Replace every `#Name#` that has a fixed value; leave the rest in place.
    fn resolve(&self, s: &str) -> String {
        self.resolve_depth(s, 0)
    }

    fn resolve_depth(&self, s: &str, depth: u32) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(start) = rest.find('#') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let Some(len) = after.find('#') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let name = &after[..len];
            let is_name =
                !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_');
            if !is_name {
                // not a variable: keep the '#' and look again from the next one
                out.push('#');
                rest = after;
                continue;
            }
            match self.vars.get(&name.to_ascii_lowercase()) {
                Some(v) if !(self.dynamic)(name) && depth < 8 => {
                    out.push_str(&self.resolve_depth(v, depth + 1));
                }
                _ => {
                    out.push('#');
                    out.push_str(name);
                    out.push('#');
                }
            }
            rest = &after[len + 1..];
        }
        out.push_str(rest);
        out
    }
}

/// `+ - * /`, parentheses and numbers: all a position formula in these skins
/// uses once its variables are in.
fn eval(s: &str) -> Option<f64> {
    struct P<'a> {
        s: &'a [u8],
        i: usize,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
        }
        fn eat(&mut self, c: u8) -> bool {
            self.ws();
            if self.i < self.s.len() && self.s[self.i] == c {
                self.i += 1;
                true
            } else {
                false
            }
        }
        fn expr(&mut self) -> Option<f64> {
            let mut v = self.term()?;
            loop {
                if self.eat(b'+') {
                    v += self.term()?;
                } else if self.eat(b'-') {
                    v -= self.term()?;
                } else {
                    return Some(v);
                }
            }
        }
        fn term(&mut self) -> Option<f64> {
            let mut v = self.factor()?;
            loop {
                if self.eat(b'*') {
                    v *= self.factor()?;
                } else if self.eat(b'/') {
                    v /= self.factor()?;
                } else {
                    return Some(v);
                }
            }
        }
        fn factor(&mut self) -> Option<f64> {
            if self.eat(b'-') {
                return Some(-self.factor()?);
            }
            if self.eat(b'(') {
                let v = self.expr()?;
                return self.eat(b')').then_some(v);
            }
            self.ws();
            let start = self.i;
            while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.')
            {
                self.i += 1;
            }
            std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
        }
    }
    let mut p = P { s: s.as_bytes(), i: 0 };
    let v = p.expr()?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

/// Split on `sep` where it is not inside parentheses.
fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c == sep && depth == 0 {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().to_string());
    out
}

fn numbers(s: &str) -> Option<Vec<f64>> {
    split_top(s, ',').iter().map(|p| eval(p)).collect()
}

/// "R,G,B[,A]" or "RRGGBB[AA]" as 0xAARRGGBB.
fn parse_colour(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.contains(',') {
        let v = numbers(s)?;
        if v.len() < 3 {
            return None;
        }
        let c = |x: f64| x.clamp(0.0, 255.0).round() as u32;
        let a = if v.len() > 3 { c(v[3]) } else { 255 };
        Some(a << 24 | c(v[0]) << 16 | c(v[1]) << 8 | c(v[2]))
    } else if s.len() == 6 || s.len() == 8 {
        let rgb = u32::from_str_radix(&s[..6], 16).ok()?;
        let a = if s.len() == 8 { u32::from_str_radix(&s[6..], 16).ok()? } else { 255 };
        Some(a << 24 | rgb)
    } else {
        None
    }
}

/// A colour option as Rust: fixed, or the name of the variable that holds it.
fn colour_code(resolved: &str) -> Option<String> {
    let s = resolved.trim();
    if let Some(inner) = s.strip_prefix('#').and_then(|r| r.strip_suffix('#')) {
        if !inner.contains('#') {
            return Some(format!("Col::V({inner:?})"));
        }
    }
    parse_colour(s).map(|c| format!("Col::S(0x{c:08X})"))
}

// ---------------------------------------------------------------------------
//  Meters
// ---------------------------------------------------------------------------

/// A meter's options with its MeterStyles folded in: the meter's own win, and
/// among styles a later one wins over an earlier one.
fn effective(sec: &Section, all: &[Section]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let put = |k: &str, v: &str, out: &mut Vec<(String, String)>| {
        match out.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v.to_string(),
            None => out.push((k.to_string(), v.to_string())),
        }
    };
    if let Some(styles) = sec.get("meterstyle") {
        for style in styles.split('|').map(str::trim) {
            if let Some(s) = all.iter().find(|s| s.name.eq_ignore_ascii_case(style)) {
                for (k, v) in &s.opts {
                    put(k, v, &mut out);
                }
            }
        }
    }
    for (k, v) in &sec.opts {
        put(k, v, &mut out);
    }
    out
}

fn opt<'a>(opts: &'a [(String, String)], key: &str) -> Option<&'a str> {
    opts.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[derive(Default)]
struct Bounds {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    any: bool,
}

impl Bounds {
    fn add(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if !self.any {
            *self = Bounds { x0, y0, x1, y1, any: true };
        } else {
            self.x0 = self.x0.min(x0);
            self.y0 = self.y0.min(y0);
            self.x1 = self.x1.max(x1);
            self.y1 = self.y1.max(y1);
        }
    }
}

/// One `Shape=` line as a `Sh::...` expression, or None if the program has to
/// draw it (it moves, turns, or is written whole by the logic).
fn shape_code(val: &str, opts: &[(String, String)], ctx: &Ctx, b: &mut Bounds) -> Option<String> {
    let val = ctx.resolve(val);
    let parts: Vec<&str> = val.split('|').map(str::trim).collect();
    let head = parts[0];
    let (kind, params) = head.split_once(char::is_whitespace).unwrap_or((head, ""));
    let params = params.trim();

    let mut fill = "255,255,255,255".to_string();
    let mut gradient: Option<String> = None;
    let mut stroke = "0,0,0,255".to_string();
    let mut stroke_width = 1.0;
    let mut dashes: Option<Vec<f64>> = None;
    let mut rotate: Option<String> = None;
    for part in &parts[1..] {
        let lower = part.to_ascii_lowercase();
        if let Some(_) = lower.strip_prefix("fill color") {
            fill = part["fill color".len()..].trim().to_string();
        } else if lower.starts_with("fill lineargradient") {
            gradient = Some(part["fill lineargradient".len()..].trim().to_string());
        } else if lower.starts_with("strokewidth") {
            stroke_width = eval(part["strokewidth".len()..].trim())?;
        } else if lower.starts_with("stroke color") {
            stroke = part["stroke color".len()..].trim().to_string();
        } else if lower.starts_with("strokedashes") {
            dashes = Some(numbers(part["strokedashes".len()..].trim())?);
        } else if lower.starts_with("rotate") {
            rotate = Some(part["rotate".len()..].trim().to_string());
        }
    }

    // A shape that turns: its outline as a polygon, the variable holding the
    // angle, and the pivot -- which is measured from the corner of the
    // shape's own bounds.
    if let Some(rot) = rotate {
        let args = split_top(&rot, ',');
        let var = args[0].strip_prefix('#')?.strip_suffix('#')?.to_string();
        let pts: Vec<(f64, f64)> = match kind.to_ascii_lowercase().as_str() {
            "rectangle" => {
                let v = numbers(params)?;
                vec![(v[0], v[1]), (v[0] + v[2], v[1]), (v[0] + v[2], v[1] + v[3]), (v[0], v[1] + v[3])]
            }
            "path" => {
                let def = ctx.resolve(opt(opts, &params.to_ascii_lowercase())?);
                let mut pts = Vec::new();
                for seg in def.split('|').map(str::trim) {
                    let lower = seg.to_ascii_lowercase();
                    if lower.starts_with("closepath") {
                        continue;
                    }
                    let coords = if lower.starts_with("lineto") { seg["lineto".len()..].trim() } else { seg };
                    let v = numbers(coords)?;
                    pts.push((v[0], v[1]));
                }
                pts
            }
            _ => return None,
        };
        let minx = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        let miny = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
        let maxx = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
        let maxy = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        let (cx, cy) = match (args.get(1).and_then(|a| eval(a)), args.get(2).and_then(|a| eval(a))) {
            (Some(ax), Some(ay)) => (minx + ax, miny + ay),
            _ => ((minx + maxx) / 2.0, (miny + maxy) / 2.0),
        };
        let reach = pts.iter().map(|p| ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()).fold(0.0, f64::max) + 1.0;
        b.add(cx - reach, cy - reach, cx + reach, cy + reach);
        let mut code = String::new();
        for p in &pts {
            write!(code, "({:?},{:?}),", p.0, p.1).unwrap();
        }
        return Some(format!("Sh::Rot(&[{code}],{},{var:?},{cx:?},{cy:?})", colour_code(&fill)?));
    }

    match kind.to_ascii_lowercase().as_str() {
        "rectangle" => {
            let v = numbers(params)?;
            let (x, y, w, h) = (v[0], v[1], v[2], v[3]);
            b.add(x, y, x + w, y + h);
            if let Some(name) = gradient {
                // "angle | colour ; position | colour ; position"
                let def = ctx.resolve(opt(opts, &name.to_ascii_lowercase())?);
                let stops: Vec<&str> = def.split('|').map(str::trim).collect();
                let angle = eval(stops[0])?;
                assert!(angle == 180.0 && stops.len() == 3, "only a two-stop 180 degree gradient is supported");
                let c0 = parse_colour(stops[1].split(';').next()?)?;
                let c1 = parse_colour(stops[2].split(';').next()?)?;
                return Some(format!("Sh::Grad({},{},{},{},0x{c0:08X},0x{c1:08X})", x as i32, y as i32, w as i32, h as i32));
            }
            Some(format!("Sh::Rect({},{},{},{},{})", x as i32, y as i32, w as i32, h as i32, colour_code(&fill)?))
        }
        "line" => {
            let v = numbers(params)?;
            assert!(v[1] == v[3], "only horizontal lines are supported");
            let (on, off) = match dashes {
                Some(d) if d.len() >= 2 => (d[0], d[1]),
                _ => (0.0, 0.0),
            };
            let half = stroke_width / 2.0;
            b.add(v[0], v[1] - half, v[2], v[1] + half);
            Some(format!(
                "Sh::Line({:?},{:?},{:?},{:?},{},{:?},{:?})",
                v[0], v[1], v[2], stroke_width, colour_code(&stroke)?, on * stroke_width, off * stroke_width
            ))
        }
        "ellipse" => {
            let v = numbers(params)?;
            let (cx, cy, rx) = (v[0], v[1], v[2]);
            let ry = v.get(3).copied().unwrap_or(rx);
            b.add(cx - rx, cy - ry, cx + rx, cy + ry);
            Some(format!("Sh::Ellipse({cx:?},{cy:?},{rx:?},{ry:?},{})", colour_code(&fill)?))
        }
        "path" => {
            let def = ctx.resolve(opt(opts, &params.to_ascii_lowercase())?);
            if def.contains('#') {
                return None;
            }
            let mut pts = String::new();
            for seg in def.split('|').map(str::trim) {
                let lower = seg.to_ascii_lowercase();
                if lower.starts_with("closepath") {
                    continue;
                }
                let coords = if lower.starts_with("lineto") { seg["lineto".len()..].trim() } else { seg };
                let v = numbers(coords)?;
                b.add(v[0], v[1], v[0], v[1]);
                write!(pts, "({:?},{:?}),", v[0], v[1]).unwrap();
            }
            Some(format!("Sh::Poly(&[{pts}],{})", colour_code(&fill)?))
        }
        _ => None,
    }
}

/// A mouse action, `Select(7)` or `Close()`, as (command, argument): the
/// command is the widget's own, or About / Close / OpenFile, and the argument
/// is one whole number, 0 if there is none.  Every command is handed the
/// pointer's x and y as well, so a click that needs them asks for nothing.
fn action(v: &str) -> Option<(String, i32)> {
    let (cmd, args) = v.trim().strip_suffix(')')?.split_once('(')?;
    let (cmd, args) = (cmd.trim(), args.trim());
    let is_name = !cmd.is_empty() && cmd.bytes().all(|c| c.is_ascii_alphanumeric());
    let arg = if args.is_empty() { 0 } else { args.parse().ok()? };
    is_name.then(|| (cmd.to_string(), arg))
}

const EVENTS: [(&str, &str); 6] = [
    ("leftmouseupaction", "Ev::Up"),
    ("leftmousedoubleclickaction", "Ev::Dbl"),
    ("mousescrollupaction", "Ev::WheelUp"),
    ("mousescrolldownaction", "Ev::WheelDown"),
    ("mouseoveraction", "Ev::Over"),
    ("mouseleaveaction", "Ev::Leave"),
];

fn actions_code(section: &str, opts: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    for (key, ev) in EVENTS {
        if let Some(v) = opts(key) {
            let (cmd, arg) = action(&v).unwrap_or_else(|| panic!("[{section}] {key}={v} is not Command(number)"));
            write!(out, "Act{{ev:{ev},cmd:{cmd:?},arg:{arg}}},").unwrap();
        }
    }
    out
}

struct SkinDef {
    module: &'static str,
    file: &'static str,
    /// Variables the skin's logic writes while it runs.
    dynamic: fn(&str) -> bool,
}

/// `prefix` followed by nothing but digits: Row7, Tick12 -- but not Row1Y.
fn numbered(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|r| !r.is_empty() && r.bytes().all(|c| c.is_ascii_digit()))
}

fn dyn_datetime(n: &str) -> bool {
    const NAMES: &[&str] = &[
        "Page", "MLHide", "MLMonth", "MonthName", "YearText", "TimeText", "Today", "TZName",
        "TZOffset", "UTCTime", "TZDst", "UseLegend", "UseTotals", "SelCol", "SelRow", "SelHide",
        "AngleHour", "AngleMin", "AngleSec", "BarNow", "HideSeconds", "HideRing",
    ];
    NAMES.contains(&n)
        || numbered(n, "D")
        || numbered(n, "C")
        || numbered(n, "W")
        || (n.starts_with("Bar") && n[3..].starts_with(|c: char| c.is_ascii_digit()))
}

fn dyn_resources(n: &str) -> bool {
    matches!(n, "CpuCover" | "RamCover" | "CpuPct" | "RamPct")
}

fn dyn_notes(n: &str) -> bool {
    matches!(n, "NoteBg" | "Words" | "ThumbY" | "ThumbH" | "UpInk" | "DownInk" | "RuleInk" | "Created")
}

fn dyn_weather(n: &str) -> bool {
    matches!(n, "TempNow" | "Condition" | "HighLowText" | "LocationLabel" | "Lat" | "Lon")
}

const SKINS: [SkinDef; 5] = [
    SkinDef { module: "datetime", file: "Win98DateTime.ini", dynamic: dyn_datetime },
    SkinDef { module: "resources", file: "Win98Resources.ini", dynamic: dyn_resources },
    SkinDef { module: "notes", file: "Win98Notes.ini", dynamic: dyn_notes },
    SkinDef { module: "weather", file: "Win98Weather.ini", dynamic: dyn_weather },
    // nothing in the rain's skin changes while it runs: rain.rs draws the one meter that moves
    SkinDef { module: "rain", file: "Win98Rain.ini", dynamic: |_| false },
];

fn compile_skin(def: &SkinDef, root: &Path, fonts: &mut Vec<(String, bool)>, out: &mut String) {
    let path = root.join(def.file);
    println!("cargo:rerun-if-changed={}", path.display());
    let sections = parse_ini(&read_text(&path));

    let mut vars = HashMap::new();
    let mut var_order = Vec::new();
    if let Some(v) = sections.iter().find(|s| s.name.eq_ignore_ascii_case("Variables")) {
        // keys are lower-cased by the parser; recover their spelling for the table
        let text = read_text(&path);
        for (k, val) in &v.opts {
            vars.insert(k.clone(), val.clone());
            let spelled = text
                .lines()
                .filter_map(|l| l.split_once('='))
                .map(|(key, _)| key.trim())
                .find(|key| key.eq_ignore_ascii_case(k))
                .unwrap_or(k)
                .to_string();
            var_order.push((spelled, k.clone()));
        }
    }
    let ctx = Ctx { vars, dynamic: &def.dynamic };

    let mut meters = String::new();
    let mut size = (0, 0);

    for sec in &sections {
        let Some(kind) = sec.get("meter") else { continue };
        let opts = effective(sec, &sections);
        let get = |k: &str| opt(&opts, k).map(|v| ctx.resolve(v));

        // position: a number, or a formula the program works out as it draws
        let coord = |key: &str| -> (i32, String) {
            match get(key) {
                None => (0, String::new()),
                Some(v) if v.contains('#') => (0, v),
                Some(v) => (eval(&v).unwrap_or_else(|| panic!("[{}] {key}={v}", sec.name)).round() as i32, String::new()),
            }
        };
        let (x, xf) = coord("x");
        let (y, yf) = coord("y");
        let hidden = match get("hidden") {
            Some(h) if h.trim() != "0" => h,
            _ => String::new(),
        };
        let acts = actions_code(&sec.name, &|k| get(k));

        let mut bounds = Bounds::default();
        let kind_code = match kind.to_ascii_lowercase().as_str() {
            "shape" => {
                let mut keys: Vec<(u32, &str)> = opts
                    .iter()
                    .filter_map(|(k, v)| {
                        let n = k.strip_prefix("shape")?;
                        let index = if n.is_empty() { 1 } else { n.parse().ok()? };
                        Some((index, v.as_str()))
                    })
                    .collect();
                keys.sort_by_key(|k| k.0);
                let mut code = String::new();
                let mut custom = false;
                for (_, v) in keys {
                    match shape_code(v, &opts, &ctx, &mut bounds) {
                        Some(c) => {
                            code.push_str(&c);
                            code.push(',');
                        }
                        None => custom = true,
                    }
                }
                if custom { "Kind::Custom".to_string() } else { format!("Kind::Shape(&[{code}])") }
            }
            "string" => {
                let size_pt = get("fontsize").and_then(|v| eval(&v)).unwrap_or(10.0);
                let bold = get("stringstyle").is_some_and(|s| s.to_ascii_lowercase().contains("bold"));
                let key = (format!("{size_pt:?}"), bold);
                let font = fonts.iter().position(|f| *f == key).unwrap_or_else(|| {
                    fonts.push(key);
                    fonts.len() - 1
                });
                let align = match get("stringalign").unwrap_or_default().to_ascii_lowercase() {
                    a if a.starts_with("center") => 1,
                    a if a.starts_with("right") => 2,
                    _ => 0,
                };
                let dim = |k: &str| get(k).and_then(|v| eval(&v)).map_or(-1, |v| v.round() as i32);
                let (w, h) = (dim("w"), dim("h"));
                let clip = get("clipstring").and_then(|v| eval(&v)).unwrap_or(0.0) != 0.0;
                let col = colour_code(&get("fontcolor").unwrap_or_else(|| "0,0,0".into()))
                    .unwrap_or_else(|| panic!("[{}] FontColor", sec.name));
                let solid = match get("solidcolor") {
                    Some(c) => format!("Some({})", colour_code(&c).unwrap()),
                    None => "None".to_string(),
                };
                let pad = get("padding").and_then(|p| numbers(&p)).unwrap_or_default();
                let p = |i: usize| pad.get(i).copied().unwrap_or(0.0) as i32;
                let text = get("text").unwrap_or_default();
                let src = match text.strip_prefix('#').and_then(|t| t.strip_suffix('#')) {
                    Some(name) if !name.contains('#') => format!("Src::V({name:?})"),
                    _ => format!("Src::L({text:?})"),
                };
                // Generous on purpose: this only decides whether a repaint of
                // part of the window has to look at the meter at all.
                let line = (size_pt * 4.0 / 3.0 * 1.4).ceil() + 3.0;
                let hh = if h >= 0 { h as f64 } else { line };
                bounds.add(-2000.0, -2.0, 2000.0, hh + p(1) as f64 + p(3) as f64 + 2.0);
                format!(
                    "Kind::Text(TextM{{src:{src},col:{col},font:{font},align:{align},w:{w},h:{h},clip:{clip},solid:{solid},pad:[{},{},{},{}]}})",
                    p(0), p(1), p(2), p(3)
                )
            }
            "image" => {
                let dim = |k: &str| get(k).and_then(|v| eval(&v)).map_or(0.0, |v| v);
                bounds.add(0.0, 0.0, dim("w"), dim("h"));
                "Kind::Custom".to_string()
            }
            other => panic!("[{}] Meter={other} is not supported", sec.name),
        };

        // A meter the program draws, or one that moves, is looked at on every
        // repaint: its bounds are the whole window.
        let moving = !xf.is_empty() || !yf.is_empty();
        let custom_shape = kind_code == "Kind::Custom" && kind.eq_ignore_ascii_case("shape");
        let b = if !bounds.any || moving || custom_shape {
            "[-30000,-30000,30000,30000]".to_string()
        } else {
            format!(
                "[{},{},{},{}]",
                x + bounds.x0.floor() as i32,
                y + bounds.y0.floor() as i32,
                x + bounds.x1.ceil() as i32,
                y + bounds.y1.ceil() as i32
            )
        };
        if sec.name.eq_ignore_ascii_case("MeterCanvas") {
            size = ((x as f64 + bounds.x1) as i32, (y as f64 + bounds.y1) as i32);
        }

        writeln!(
            meters,
            "        Meter{{name:{:?},x:{x},y:{y},xf:{xf:?},yf:{yf:?},b:{b},hidden:{hidden:?},kind:{kind_code},acts:&[{acts}]}},",
            sec.name
        )
        .unwrap();
    }
    assert!(size.0 > 0 && size.1 > 0, "{}: no [MeterCanvas] to size the window from", def.file);

    // [Widget]: the mouse actions of the widget as a whole, for wherever no
    // meter claims them.
    let widget = sections.iter().find(|s| s.name.eq_ignore_ascii_case("Widget"));
    let skin_acts = actions_code("Widget", &|k| widget.and_then(|w| w.get(k)).map(|v| ctx.resolve(v)));

    writeln!(out, "pub mod {} {{", def.module).unwrap();
    writeln!(out, "    use super::*;").unwrap();
    writeln!(out, "    pub static SKIN: Skin = Skin {{ w: {}, h: {}, meters: METERS, acts: &[{skin_acts}], vars: VARS }};", size.0, size.1).unwrap();
    writeln!(out, "    static METERS: &[Meter] = &[\n{meters}    ];").unwrap();
    writeln!(out, "    static VARS: &[(&str, &str)] = &[").unwrap();
    for (spelled, key) in &var_order {
        writeln!(out, "        ({spelled:?}, {:?}),", ctx.resolve(&ctx.vars[key])).unwrap();
    }
    // The edit box is not a meter, but where it sits over the paper is
    // layout all the same.
    if let Some(input) = sections.iter().find(|s| s.name.eq_ignore_ascii_case("EditBox")) {
        for key in ["x", "y", "w", "h"] {
            if let Some(n) = input.get(key).and_then(|v| eval(&ctx.resolve(v))) {
                writeln!(out, "        (\"Input{}\", \"{n}\"),", key.to_ascii_uppercase()).unwrap();
            }
        }
        if let Some(p) = input.get("padding") {
            writeln!(out, "        (\"InputPadding\", {:?}),", ctx.resolve(p)).unwrap();
        }
    }
    writeln!(out, "    ];\n}}\n").unwrap();
}

// ---------------------------------------------------------------------------
//  Icons
// ---------------------------------------------------------------------------

fn compile_icons(root: &Path, out_dir: &Path, out: &mut String) {
    let dir = root.join("Icons");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
        .collect();
    names.sort();

    writeln!(out, "/// (name, width, height, premultiplied BGRA) for each PNG in skins/Icons.").unwrap();
    writeln!(out, "pub static ICONS: &[(&str, i32, i32, &[u8])] = &[").unwrap();
    for path in names {
        println!("cargo:rerun-if-changed={}", path.display());
        let mut decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(&path).unwrap()));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().expect("image too large")];
        let info = reader.next_frame(&mut buf).unwrap();
        let px = &buf[..info.buffer_size()];
        let channels = match info.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Grayscale => 1,
            png::ColorType::Indexed => unreachable!("EXPAND turns a palette into RGB"),
        };
        let mut bgra = Vec::with_capacity((info.width * info.height * 4) as usize);
        for p in px.chunks_exact(channels) {
            let (r, g, b, a) = match channels {
                4 => (p[0], p[1], p[2], p[3]),
                3 => (p[0], p[1], p[2], 255),
                2 => (p[0], p[0], p[0], p[1]),
                _ => (p[0], p[0], p[0], 255),
            };
            let pm = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
            bgra.extend_from_slice(&[pm(b), pm(g), pm(r), a]);
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let raw = out_dir.join(format!("{stem}.bgra"));
        fs::write(&raw, &bgra).unwrap();
        writeln!(
            out,
            "    ({stem:?}, {}, {}, include_bytes!({:?})),",
            info.width,
            info.height,
            raw.to_string_lossy()
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
}

/// English (US), as rc.exe would say.
const RES_LANG: u16 = 0x0409;

/// The resources as a .res file, which is what the MSVC linker takes.
fn res_file(items: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    // every .res opens with an empty resource
    let mut res = vec![0, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0];
    res.extend_from_slice(&[0; 16]);
    // One resource: a 32-byte header that names it by type and number, then
    // the data, padded to a multiple of four.
    for (kind, id, data) in items {
        res.extend_from_slice(&(data.len() as u32).to_le_bytes());
        res.extend_from_slice(&32u32.to_le_bytes());
        for n in [0xFFFF, *kind, 0xFFFF, *id] {
            res.extend_from_slice(&n.to_le_bytes());
        }
        res.extend_from_slice(&0u32.to_le_bytes()); // data version
        res.extend_from_slice(&0x1030u16.to_le_bytes()); // moveable, pure, discardable
        res.extend_from_slice(&RES_LANG.to_le_bytes());
        res.extend_from_slice(&[0; 8]); // version, characteristics
        res.extend_from_slice(data);
        while res.len() % 4 != 0 {
            res.push(0);
        }
    }
    res
}

/// The resources as an object file with one `.rsrc` section, which is what
/// the GNU linker takes: the tree the loader walks (type, then number, then
/// language), the data it points at, and a relocation for each pointer.
/// `items` must be in (type, number) order.
fn rsrc_object(machine: u16, rva_reloc: u16, items: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    const SUBDIR: usize = 0x8000_0000;
    fn word(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn long(out: &mut Vec<u8>, v: usize) {
        out.extend_from_slice(&(v as u32).to_le_bytes());
    }
    /// A directory's header; its `ids` entries of eight bytes follow it.
    fn dir(out: &mut Vec<u8>, ids: usize) {
        out.extend_from_slice(&[0; 14]); // characteristics, time, version, no named entries
        word(out, ids as u16);
    }

    assert!(items.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)), "resources out of order");
    let mut kinds: Vec<(u16, usize)> = Vec::new();
    for (kind, ..) in items {
        match kinds.last_mut() {
            Some((k, count)) if k == kind => *count += 1,
            _ => kinds.push((*kind, 1)),
        }
    }
    // Where each layer starts: after the root come a directory for each type,
    // a directory for each resource (its one language), a data entry for
    // each resource, and then the data.
    let kind_dirs = 16 + 8 * kinds.len();
    let lang_dirs = kind_dirs + kinds.iter().map(|&(_, count)| 16 + 8 * count).sum::<usize>();
    let entries = lang_dirs + 24 * items.len();
    let mut data_at = entries + 16 * items.len();

    let mut sec = Vec::new();
    dir(&mut sec, kinds.len());
    let mut at = kind_dirs;
    for &(kind, count) in &kinds {
        long(&mut sec, kind as usize);
        long(&mut sec, SUBDIR | at);
        at += 16 + 8 * count;
    }
    let mut i = 0;
    for &(_, count) in &kinds {
        dir(&mut sec, count);
        for _ in 0..count {
            long(&mut sec, items[i].1 as usize);
            long(&mut sec, SUBDIR | (lang_dirs + 24 * i));
            i += 1;
        }
    }
    for i in 0..items.len() {
        dir(&mut sec, 1);
        long(&mut sec, RES_LANG as usize);
        long(&mut sec, entries + 16 * i);
    }
    let mut relocs = Vec::new();
    for (_, _, data) in items {
        relocs.push(sec.len());
        long(&mut sec, data_at); // from the section's start; the linker adds where that is
        long(&mut sec, data.len());
        long(&mut sec, 0); // code page
        long(&mut sec, 0);
        data_at += data.len().next_multiple_of(4);
    }
    for (_, _, data) in items {
        sec.extend_from_slice(data);
        while sec.len() % 4 != 0 {
            sec.push(0);
        }
    }

    let relocs_at = 20 + 40 + sec.len();
    let symbols_at = relocs_at + 10 * relocs.len();
    let mut obj = Vec::new();
    // the file header
    word(&mut obj, machine);
    word(&mut obj, 1); // sections
    long(&mut obj, 0); // time
    long(&mut obj, symbols_at);
    long(&mut obj, 1); // symbols
    word(&mut obj, 0); // no optional header
    word(&mut obj, 0);
    // the section's header
    obj.extend_from_slice(b".rsrc\0\0\0");
    long(&mut obj, 0);
    long(&mut obj, 0);
    long(&mut obj, sec.len());
    long(&mut obj, 20 + 40);
    long(&mut obj, relocs_at);
    long(&mut obj, 0);
    word(&mut obj, relocs.len() as u16);
    word(&mut obj, 0);
    long(&mut obj, 0x4030_0040); // initialised data, read-only, aligned to four
    obj.extend_from_slice(&sec);
    for at in relocs {
        long(&mut obj, at);
        long(&mut obj, 0); // against the symbol below
        word(&mut obj, rva_reloc);
    }
    // the one symbol: the section itself
    obj.extend_from_slice(b".rsrc\0\0\0");
    long(&mut obj, 0);
    word(&mut obj, 1); // section 1
    word(&mut obj, 0);
    obj.extend_from_slice(&[3, 0]); // static, no auxiliary records
    long(&mut obj, 4); // an empty string table
    obj
}

fn compile_exe_icon(root: &Path, out_dir: &Path) {
    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;

    let path = root.join("Icons").join("gears_tweakui_b.ico");
    println!("cargo:rerun-if-changed={}", path.display());
    let ico = fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let word = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let long = |at: usize| u32::from_le_bytes([ico[at], ico[at + 1], ico[at + 2], ico[at + 3]]) as usize;
    // ICONDIR: reserved, type (1 = icon), count; then 16 bytes an image, the
    // last eight of them its length and where in the file it starts.
    assert!(ico.len() >= 6 && word(0) == 0 && word(2) == 1, "{} is not an .ico", path.display());
    let count = word(4) as usize;
    assert!(count > 0 && ico.len() >= 6 + 16 * count, "{} has no images", path.display());

    // The group is the .ico's directory with each image's place in the file
    // swapped for its resource number.
    let mut items: Vec<(u16, u16, Vec<u8>)> = Vec::new();
    let mut group = ico[..6].to_vec();
    for i in 0..count {
        let entry = 6 + 16 * i;
        let (len, at) = (long(entry + 8), long(entry + 12));
        let image = ico.get(at..at + len).unwrap_or_else(|| panic!("{}: image {i} runs off the end", path.display()));
        let id = i as u16 + 1;
        items.push((RT_ICON, id, image.to_vec()));
        group.extend_from_slice(&ico[entry..entry + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    items.push((RT_GROUP_ICON, 1, group));

    // The MSVC linker converts a .res itself; the GNU one wants an object
    // file, with the relocation that means "address, less the image base".
    let (name, bytes) = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        ("icon.res", res_file(&items))
    } else {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let (machine, rva_reloc) = match arch.as_str() {
            "x86_64" => (0x8664, 3),
            "x86" => (0x014C, 7),
            "aarch64" => (0xAA64, 2),
            _ => {
                println!("cargo:warning=no resource object for {arch}: the .exe is built without its icon");
                return;
            }
        };
        ("icon.o", rsrc_object(machine, rva_reloc, &items))
    };
    let file = out_dir.join(name);
    fs::write(&file, bytes).unwrap();
    println!("cargo:rustc-link-arg-bins={}", file.display());
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("skins");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=build.rs");

    let mut out = String::from("// Generated by build.rs from the skins' .ini files.  Do not edit.\n\n");
    let mut fonts: Vec<(String, bool)> = Vec::new();
    for def in &SKINS {
        compile_skin(def, &root, &mut fonts, &mut out);
    }
    writeln!(out, "/// (size in points, bold) for every font a String meter asks for.").unwrap();
    writeln!(out, "pub static FONTS: &[(f32, bool)] = &[").unwrap();
    for (size, bold) in &fonts {
        writeln!(out, "    ({size}, {bold}),").unwrap();
    }
    writeln!(out, "];\n").unwrap();
    compile_icons(&root, &out_dir, &mut out);
    compile_exe_icon(&root, &out_dir);

    fs::write(out_dir.join("skins.rs"), out).unwrap();
}
