//! The tint: one colour, picked by the user, that every widget's accents are
//! drawn in.
//!
//! The skins' accents are all drawn around one colour, their `Tint=` -- the
//! rust of the title bars.  A tint turns each of them by the difference
//! between that colour and the one picked, in OKLCH: the hue is turned by
//! the difference in hue, the colourfulness is scaled by the ratio of the
//! two, and the lightness is left exactly as it was.  So every accent keeps
//! its light and its shade, and the text on it stays as far from it as it
//! was: whatever is picked, nothing becomes hard to read.  A grey turns the
//! accents grey; a very pale or very dark colour gives its hue, not its
//! lightness.
//!
//! What is turned is a colour as it is looked up -- one in a skin's tables,
//! or one a widget reads from its skin -- never the pixels once drawn: a
//! picture (the weather's icons) is left as it is, and so is every colour a
//! skin names in `Untinted=`: the greys and whites of the windows, the text,
//! the note's paper.  And the skins' own tint leaves every colour exactly as
//! it was, bit for bit.
//!
//! The tint belongs to the thread that draws.  The program draws on one
//! thread; tests, which run side by side on several, each have their own.

use core::cell::{Cell, RefCell};

use crate::skin;

/// The colour the skins' palettes are drawn around, as 0xRRGGBB: with it,
/// every colour is drawn as the skin gives it.
pub const BASE: u32 = skin::TINT;

/// However colourful the colour picked, the palette is made at most this
/// many times as colourful as it was: enough for a navy to be navy, not so
/// much that the window's grey-gold face turns loud.
const K_MAX: f64 = 1.5;

/// What the menu offers, by name: the skins' own first, then the other warm
/// ones -- the earthy, muted colours of a retro editor palette -- and then
/// the cool ones.  Only a colour's hue and how colourful it is
/// count (see the head of this file), so a name says what the widgets come
/// out as, not what the figures are.  A tint that is none of these is a
/// custom one.
pub const PRESETS: [(&str, u32); 11] = [
    ("Rust", BASE),
    ("Brick", 0xCC_241D),
    ("Orange", 0xDA_7E1E),
    ("Mustard", 0xD7_9921),
    ("Olive", 0x98_971A),
    ("Mauve", 0xB1_6286),
    ("Taupe", 0x99_816E),
    ("Navy", 0x00_0080),
    ("Teal", 0x00_8080),
    ("Slate", 0x50_6078),
    ("Graphite", 0x80_8080),
];

thread_local! {
    static CURRENT: Cell<u32> = const { Cell::new(BASE) };
    /// Colours already turned to the current tint: a skin has a few dozen,
    /// looked up over and over as it is drawn.
    static MEMO: RefCell<Vec<(u32, u32)>> = const { RefCell::new(Vec::new()) };
}

/// The most colours remembered before starting over: far more than all the
/// skins have between them.
const MEMO_MAX: usize = 512;

/// Draw in `rgb` (0xRRGGBB) from now on.
pub fn set(rgb: u32) {
    let rgb = rgb & 0xFF_FFFF;
    if CURRENT.replace(rgb) != rgb {
        MEMO.with_borrow_mut(Vec::clear);
    }
}

/// The tint drawn in now, as 0xRRGGBB.
pub fn current() -> u32 {
    CURRENT.get()
}

/// `argb` as the current tint has it.
pub fn apply(argb: u32) -> u32 {
    let target = current();
    if target == BASE {
        return argb;
    }
    MEMO.with_borrow_mut(|memo| {
        if let Some(&(_, out)) = memo.iter().find(|(from, _)| *from == argb) {
            return out;
        }
        let out = apply_with(target, argb);
        if memo.len() >= MEMO_MAX {
            memo.clear();
        }
        memo.push((argb, out));
        out
    })
}

/// `argb` as tint `target` (0xRRGGBB) has it.  Its alpha is kept.
pub fn apply_with(target: u32, argb: u32) -> u32 {
    let target = target & 0xFF_FFFF;
    if target == BASE || argb >> 24 == 0 {
        return argb;
    }
    let (from, to, c) = (Lch::of(BASE), Lch::of(target), Lch::of(argb));
    let k = if from.c > 1e-6 { (to.c / from.c).clamp(0.0, K_MAX) } else { 0.0 };
    let turned = Lch { l: c.l, c: c.c * k, h: c.h + (to.h - from.h) };
    argb & 0xFF00_0000 | turned.to_rgb()
}

/// The menu's name for `rgb`, if it is one of the presets.
pub fn preset_name(rgb: u32) -> Option<&'static str> {
    PRESETS.iter().find(|p| p.1 == rgb & 0xFF_FFFF).map(|p| p.0)
}

/// A colour as settings.ini has it: "R,G,B", "#RRGGBB" or "RRGGBB".
pub fn parse(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.contains(',') {
        let mut rgb = 0u32;
        let mut n = 0;
        for part in s.split(',') {
            let v: u32 = part.trim().parse().ok().filter(|&v| v <= 255)?;
            rgb = rgb << 8 | v;
            n += 1;
        }
        return (n == 3).then_some(rgb);
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// `rgb` as "R,G,B", the way the skins write a colour.
pub fn format(rgb: u32) -> String {
    format!("{},{},{}", rgb >> 16 & 255, rgb >> 8 & 255, rgb & 255)
}

// ---------------------------------------------------------------------------
//  OKLab (Björn Ottosson, 2020), in polar form
// ---------------------------------------------------------------------------

/// Lightness 0..1, chroma (how colourful), hue in radians.
#[derive(Clone, Copy, Debug)]
struct Lch {
    l: f64,
    c: f64,
    h: f64,
}

fn to_linear(c: u32) -> f64 {
    let c = (c & 255) as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_byte(c: f64) -> u32 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u32
}

fn oklab(rgb: u32) -> [f64; 3] {
    let (r, g, b) = (to_linear(rgb >> 16), to_linear(rgb >> 8), to_linear(rgb));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// Linear sRGB, each 0..1 if the colour can be shown at all.
fn linear_rgb(l: f64, a: f64, b: f64) -> [f64; 3] {
    let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_,
        -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701_0 * s_,
    ]
}

impl Lch {
    fn of(rgb: u32) -> Lch {
        let [l, a, b] = oklab(rgb);
        Lch { l, c: a.hypot(b), h: b.atan2(a) }
    }

    fn linear(&self, c: f64) -> [f64; 3] {
        linear_rgb(self.l, c * self.h.cos(), c * self.h.sin())
    }

    /// As 0xRRGGBB.  A colour the screen cannot show is made less colourful,
    /// at the same lightness and hue, until it can.
    fn to_rgb(self) -> u32 {
        const EPS: f64 = 1e-7;
        let shows = |v: &[f64; 3]| v.iter().all(|&x| (-EPS..=1.0 + EPS).contains(&x));
        let mut lin = self.linear(self.c);
        if !shows(&lin) {
            let (mut lo, mut hi) = (0.0, self.c);
            for _ in 0..32 {
                let mid = (lo + hi) / 2.0;
                if shows(&self.linear(mid)) { lo = mid } else { hi = mid }
            }
            lin = self.linear(lo);
        }
        to_byte(lin[0]) << 16 | to_byte(lin[1]) << 8 | to_byte(lin[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKINS: [&skin::Skin; 7] = [
        &skin::datetime::SKIN,
        &skin::resources::SKIN,
        &skin::notes::SKIN,
        &skin::weather::SKIN,
        &skin::rain::SKIN,
        &skin::disk::SKIN,
        &skin::world::SKIN,
    ];

    /// Every colour any skin's variables hold, alpha and all.
    fn palette() -> Vec<u32> {
        let mut out: Vec<u32> = SKINS
            .iter()
            .flat_map(|s| s.vars.iter().filter_map(|(_, v)| skin::parse_colour(v)))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Targets all round the wheel, from grey to as colourful as sRGB goes.
    fn targets() -> Vec<u32> {
        let mut out: Vec<u32> = PRESETS.iter().map(|p| p.1).collect();
        for hue in (0..360).step_by(15) {
            for (l, c) in [(0.3, 0.05), (0.5, 0.1), (0.6, 0.2), (0.7, 0.4), (0.9, 0.05)] {
                out.push(Lch { l, c, h: (hue as f64).to_radians() }.to_rgb());
            }
        }
        out.extend([0x000000, 0xFFFFFF, 0x0000FF, 0xFF0000, 0x00FF00, 0xFFFF00, 0x336699]);
        out
    }

    fn channels(c: u32) -> [i32; 3] {
        [(c >> 16 & 255) as i32, (c >> 8 & 255) as i32, (c & 255) as i32]
    }

    #[test]
    fn the_skins_own_tint_changes_nothing() {
        for c in palette() {
            assert_eq!(apply_with(BASE, c), c);
        }
        set(BASE);
        for c in palette() {
            assert_eq!(apply(c), c);
        }
    }

    #[test]
    fn a_colour_comes_back_from_oklab_as_it_went_in() {
        for c in palette().into_iter().chain(targets()) {
            let back = Lch::of(c).to_rgb();
            for (a, b) in channels(c).iter().zip(channels(back)) {
                assert!((a - b).abs() <= 1, "{c:06X} came back as {back:06X}");
            }
        }
    }

    #[test]
    fn every_tint_keeps_lightness_and_alpha_and_stays_on_screen() {
        for target in targets() {
            for c in palette() {
                let out = apply_with(target, c);
                assert_eq!(out >> 24, c >> 24, "alpha of {c:08X} under {target:06X}");
                if c >> 24 == 0 {
                    continue;
                }
                let (l0, l1) = (Lch::of(c).l, Lch::of(out).l);
                assert!((l0 - l1).abs() < 0.01, "{c:08X} under {target:06X}: lightness {l0} became {l1}");
            }
        }
    }

    #[test]
    fn the_accent_takes_the_hue_picked() {
        let accent = skin::parse_colour(skin::datetime::SKIN.var("Accent2")).unwrap();
        assert_eq!(accent & 0xFF_FFFF, BASE);
        for target in [0x00_0080, 0x00_8080, 0x60_7028, 0x70_2860, 0x33_6699, 0xC0_2040] {
            let out = Lch::of(apply_with(target, accent));
            let want = Lch::of(target).h;
            let off = (out.h - want).sin().abs();
            assert!(off < 0.05, "under {target:06X} the accent's hue is {} for {}", out.h, want);
        }
    }

    #[test]
    fn the_chrome_stays_and_the_accents_turn() {
        // the greys and whites, and the inks on the accents, in every skin
        let chrome = ["Face", "Hilite", "Light", "Shadow", "DkShadow", "FieldBg", "Ink", "TitleInk"];
        for skin in SKINS {
            for name in chrome {
                assert!(skin.is_fixed(name), "{name} is tinted in a skin");
            }
            for name in ["TitleA", "TitleB", "Accent2"] {
                assert!(!skin.is_fixed(name), "{name} is not tinted in a skin");
            }
        }
        // and what is only written as one of them follows it
        assert!(skin::notes::SKIN.is_fixed("NoteRule") && skin::notes::SKIN.is_fixed("SelectInk"));
        assert!(!skin::notes::SKIN.is_fixed("SelectBg"));
        set(0x00_0080);
        let dt = &skin::datetime::SKIN;
        assert_eq!(dt.colour("Face"), skin::parse_colour(dt.var("Face")).unwrap());
        assert_ne!(dt.colour("TitleB"), skin::parse_colour(dt.var("TitleB")).unwrap());
        set(BASE);
    }

    #[test]
    fn a_grey_tint_greys_everything() {
        for grey in [0x80_8080, 0x20_2020, 0xE0_E0E0] {
            for c in palette() {
                let [r, g, b] = channels(apply_with(grey, c));
                assert!((r - g).abs() <= 1 && (g - b).abs() <= 1, "{c:08X} under {grey:06X} is {r},{g},{b}");
            }
        }
    }

    /// WCAG's contrast ratio of two colours.
    fn contrast(a: u32, b: u32) -> f64 {
        let y = |c: u32| 0.2126 * to_linear(c >> 16) + 0.7152 * to_linear(c >> 8) + 0.0722 * to_linear(c);
        let (ya, yb) = (y(a), y(b));
        (ya.max(yb) + 0.05) / (ya.min(yb) + 0.05)
    }

    #[test]
    fn text_stays_readable_whatever_the_tint() {
        let dt = &skin::datetime::SKIN;
        let col = |name: &str| skin::parse_colour(dt.var(name)).unwrap();
        // text, and what it is drawn on
        let pairs = [
            ("TitleInk", "TitleA"),
            ("TitleInk", "TitleB"),
            ("SelInk", "Sel"),
            ("CalHeadInk", "CalHead"),
            ("Ink", "FieldBg"),
            ("Ink", "Face"),
        ];
        for target in targets() {
            // each as the program draws it: tinted, unless the skin keeps it
            let drawn = |name: &str| if dt.is_fixed(name) { col(name) } else { apply_with(target, col(name)) };
            for (ink, on) in pairs {
                let before = contrast(col(ink), col(on));
                let after = contrast(drawn(ink), drawn(on));
                assert!(
                    after >= before * 0.8 && after >= 3.0,
                    "{ink} on {on} under {target:06X}: contrast {before:.2} became {after:.2}"
                );
            }
        }
    }

    #[test]
    fn a_tint_is_set_per_thread_and_remembered_until_it_changes() {
        let accent = BASE | 0xFF00_0000;
        set(0x00_0080);
        let navy = apply(accent);
        assert_eq!(navy, apply_with(0x00_0080, accent));
        assert_eq!(apply(accent), navy);
        set(0x00_8080);
        assert_eq!(apply(accent), apply_with(0x00_8080, accent));
        // another thread draws in the skins' own
        assert_eq!(std::thread::spawn(move || apply(accent)).join().unwrap(), accent);
        set(BASE);
        assert_eq!(apply(accent), accent);
    }

    #[test]
    fn a_tint_is_written_three_ways_and_nothing_else_will_do() {
        assert_eq!(parse("0,0,128"), Some(0x00_0080));
        assert_eq!(parse(" 51 , 102 ,153 "), Some(0x33_6699));
        assert_eq!(parse("#336699"), Some(0x33_6699));
        assert_eq!(parse("336699"), Some(0x33_6699));
        assert_eq!(parse("aBcDeF"), Some(0xAB_CDEF));
        for junk in ["", "  ", "1,2", "1,2,3,4", "256,0,0", "-1,0,0", "1.5,2,3", "#12345", "#1234567", "12345g", "red"] {
            assert_eq!(parse(junk), None, "{junk:?}");
        }
        assert_eq!(format(0x33_6699), "51,102,153");
        assert_eq!(parse(&format(BASE)), Some(BASE));
    }

    #[test]
    fn the_presets_are_all_different_and_the_first_is_the_skins_own() {
        assert_eq!(PRESETS[0].1, BASE);
        for (i, a) in PRESETS.iter().enumerate() {
            assert!(PRESETS[i + 1..].iter().all(|b| a.1 != b.1 && a.0 != b.0));
            assert_eq!(preset_name(a.1), Some(a.0));
        }
        assert_eq!(preset_name(0x12_3456), None);
    }

    #[test]
    fn the_warm_presets_come_first_and_the_cool_ones_after() {
        // Rust to Taupe
        const WARM: usize = 7;
        // warm: a hue from the pinks through the reds and yellows to the
        // olives; a grey has no hue to speak of, and counts as cool
        let warm = |rgb: u32| {
            let c = Lch::of(rgb);
            let h = c.h.to_degrees().rem_euclid(360.0);
            c.c > 0.01 && (h >= 330.0 || h < 120.0)
        };
        for (name, rgb) in &PRESETS[..WARM] {
            assert!(warm(*rgb), "{name} is not warm");
        }
        for (name, rgb) in &PRESETS[WARM..] {
            assert!(!warm(*rgb), "{name} is warm");
        }
    }

    #[test]
    fn every_preset_looks_unlike_every_other() {
        // The accent each gives, against every other's: far enough apart in
        // OKLab to be told apart at a glance.  The tint keeps lightness and
        // the screen caps colourfulness, so two colours that differ on paper
        // can come out all but the same -- and a preset would look like
        // nothing happened.
        let accent = 0xFF00_0000 | BASE;
        let made: Vec<(&str, [f64; 3])> = PRESETS.iter().map(|p| (p.0, oklab(apply_with(p.1, accent)))).collect();
        for (i, (a, [l0, a0, b0])) in made.iter().enumerate() {
            for (b, [l1, a1, b1]) in &made[i + 1..] {
                let d = ((l1 - l0).powi(2) + (a1 - a0).powi(2) + (b1 - b0).powi(2)).sqrt();
                assert!(d > 0.03, "{a} and {b} come out only {d:.3} apart");
            }
        }
    }
}
