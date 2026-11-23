//! Dither Frame: the photos in a folder, one after another, each brought down
//! to a single bit a pixel -- ink or paper and nothing between -- as a 1984
//! Macintosh showed a scanned photo.  The ink is the tint the widgets are
//! drawn in, so the pictures turn with every colour picked in the menu, and
//! the paper is the cream of the widgets' fields.  Each 1-bit pixel is one of
//! the skin's, so a frame is 202 x 160 of them on its side.
//!
//! The frame itself is the Photo Frame's -- the folder and the shuffled round
//! of its photos, the three buttons, the three shapes, and as many frames as
//! are asked for (`photo`, `Look::Dither`).  What is here is the picture:
//!
//! - `tones`: the photo's light, as a meter reads it.  Each pixel's colour is
//!   taken back to linear light and weighed as the eye weighs red, green and
//!   blue; then the range is stretched, so that the darkest hundredth of the
//!   picture is ink and the lightest hundredth paper.  Dithering in linear
//!   light keeps a patch of dots as bright, from a little way off, as the
//!   photo was there.  This is done where the photo is read, in the child
//!   that reads it, and the frame keeps the tones, 16 bits a pixel.
//! - `render`: the tones brought down to one bit, in the method chosen in the
//!   menu, and painted in the ink and the paper.  Kept apart from the tones,
//!   so a change of method or of tint redraws the photo up without reading
//!   it again.
//!
//! The methods:
//!
//! - Atkinson's, the Macintosh's own (Bill Atkinson, 1984): each pixel's
//!   error spread over six neighbours an eighth apiece, so only three
//!   quarters of it is passed on.  Crisp and contrasty, with clean blacks
//!   and whites; the faintest greys go to paper and ink.  The default.
//! - Floyd and Steinberg's (1976): all of the error passed on, to four
//!   neighbours, along each row and back along the next.  The most detail
//!   and the truest tones, in a grain like film's.
//! - Bayer's ordered dither, an 8 x 8 matrix: no error passed on at all,
//!   only a threshold that differs from pixel to pixel in a fixed pattern.
//!   Even, regular cross-hatching, the look of early games and printers.

use std::path::PathBuf;

use crate::photo::{self, Look};
use crate::settings::Settings;
use crate::skin::Skin;

/// The first frame's section; the others are `Dither2`, `Dither3`, ...
pub const FIRST: &str = "Dither";

/// The brightest tone: paper, all the way.
pub const TONE_MAX: u16 = u16::MAX;
/// What share of the picture is stretched past each end of the range: its
/// darkest hundredth is ink, and its lightest paper.
const CLIP: f32 = 0.01;

/// How a frame brings its photos down to one bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Atkinson,
    Floyd,
    Bayer,
}

impl Method {
    pub const ALL: [Method; 3] = [Method::Atkinson, Method::Floyd, Method::Bayer];

    /// What settings.ini calls it.
    pub fn key(self) -> &'static str {
        match self {
            Method::Atkinson => "atkinson",
            Method::Floyd => "floyd-steinberg",
            Method::Bayer => "bayer",
        }
    }

    /// What the menu calls it.
    pub fn label(self) -> &'static str {
        match self {
            Method::Atkinson => "Atkinson dither",
            Method::Floyd => "Floyd-Steinberg dither",
            Method::Bayer => "Bayer dither",
        }
    }

    /// The method a section asks for; a frame that says nothing, or nothing
    /// known, dithers as the skin says, and that failing, Atkinson's way.
    pub fn of(settings: &Settings, section: &str, skin: &Skin) -> Method {
        let key = settings.text(section, "Method", skin.var("Method"));
        Method::ALL.into_iter().find(|m| m.key().eq_ignore_ascii_case(key.trim())).unwrap_or(Method::Atkinson)
    }
}

/// The 8 x 8 Bayer matrix: the order in which the pixels of each 8 x 8
/// square tip over to paper as the tone rises.
const BAYER: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// A colour channel, 0 to 255 as it is stored, in linear light, 0 to 1.
fn linear(c: u32) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The light of each pixel of a photo (0xFFRRGGBB, opaque), 0 for ink to
/// `TONE_MAX` for paper: in linear light, weighed as the eye weighs the three
/// colours, with the range stretched so that the darkest and the lightest
/// hundredth of the picture are ink and paper.
pub fn tones(px: &[u32]) -> Vec<u16> {
    let table: Vec<f32> = (0..256).map(linear).collect();
    let light: Vec<f32> = px
        .iter()
        .map(|&p| 0.2126 * table[(p >> 16 & 255) as usize] + 0.7152 * table[(p >> 8 & 255) as usize] + 0.0722 * table[(p & 255) as usize])
        .collect();
    if light.is_empty() {
        return Vec::new();
    }
    let mut sorted = light.clone();
    sorted.sort_by(f32::total_cmp);
    let at = |share: f32| sorted[((sorted.len() - 1) as f32 * share).round() as usize];
    let (lo, hi) = (at(CLIP), at(1.0 - CLIP));
    // a picture of one tone is left as it is, not stretched to nothing
    let (lo, span) = if hi - lo > 1e-4 { (lo, hi - lo) } else { (0.0, 1.0) };
    light.iter().map(|&l| (((l - lo) / span).clamp(0.0, 1.0) * TONE_MAX as f32).round() as u16).collect()
}

/// The tones of a picture `w` wide brought down to one bit, by `method`:
/// true for paper, false for ink.
pub fn bits(tones: &[u16], w: usize, method: Method) -> Vec<bool> {
    if w == 0 {
        return Vec::new();
    }
    let h = tones.len() / w;
    let level = |t: u16| t as f32 / TONE_MAX as f32;
    let out = match method {
        Method::Bayer => tones
            .iter()
            .enumerate()
            .map(|(i, &t)| level(t) > (BAYER[i / w % 8][i % w % 8] as f32 + 0.5) / 64.0)
            .collect(),
        Method::Atkinson => {
            // an eighth each to six neighbours: two on, three below, one
            // further below; the last quarter is let go
            const SPREAD: [(isize, usize); 6] = [(1, 0), (2, 0), (-1, 1), (0, 1), (1, 1), (0, 2)];
            diffuse(tones, w, h, &SPREAD.map(|(dx, dy)| (dx, dy, 1.0 / 8.0)), false)
        }
        Method::Floyd => {
            const SPREAD: [(isize, usize, f32); 4] = [(1, 0, 7.0 / 16.0), (-1, 1, 3.0 / 16.0), (0, 1, 5.0 / 16.0), (1, 1, 1.0 / 16.0)];
            diffuse(tones, w, h, &SPREAD, true)
        }
    };
    // a ragged last row, short of a whole one, is ink
    out.into_iter().chain(core::iter::repeat(false)).take(tones.len()).collect()
}

/// Error diffusion: each pixel, taken in turn, goes to whichever of ink and
/// paper is nearer its tone with the error already handed to it, and hands
/// on what that misses by to the neighbours in `spread` (across, down, the
/// share).  With `back_and_forth`, every other row is taken right to left,
/// and the spread turned round with it, so that the error does not drift one
/// way across the picture.
fn diffuse(tones: &[u16], w: usize, h: usize, spread: &[(isize, usize, f32)], back_and_forth: bool) -> Vec<bool> {
    let mut light: Vec<f32> = tones.iter().map(|&t| t as f32 / TONE_MAX as f32).collect();
    let mut out = vec![false; w * h];
    for y in 0..h {
        let back = back_and_forth && y % 2 == 1;
        for k in 0..w {
            let x = if back { w - 1 - k } else { k };
            let i = y * w + x;
            let paper = light[i] >= 0.5;
            out[i] = paper;
            let error = light[i] - if paper { 1.0 } else { 0.0 };
            for &(dx, dy, share) in spread {
                let nx = x as isize + if back { -dx } else { dx };
                let ny = y + dy;
                if (0..w as isize).contains(&nx) && ny < h {
                    light[ny * w + nx as usize] += error * share;
                }
            }
        }
    }
    out
}

/// The tones of a picture `w` wide brought down to one bit by `method` and
/// painted: `ink` for ink, `paper` for paper, both 0xFFRRGGBB.
pub fn render(tones: &[u16], w: usize, method: Method, ink: u32, paper: u32) -> Vec<u32> {
    bits(tones, w, method).into_iter().map(|p| if p { paper } else { ink }).collect()
}

// ---- the frames, as the Photo Frame keeps them -----------------------------

/// What a frame is called in the menus: the first is the Dither Frame, and
/// the others have their numbers.
pub fn name(section: &str) -> String {
    photo::name_of(Look::Dither, section)
}

/// The sections of the frames after the first, in the order of their numbers.
pub fn others(settings: &Settings) -> Vec<String> {
    photo::others_of(Look::Dither, settings)
}

/// Start another frame; returns its section.
pub fn create(settings: &mut Settings) -> String {
    photo::create_in(Look::Dither, settings)
}

/// Take a frame out of settings.ini.  Its photos stay in their folder.
pub fn forget(settings: &mut Settings, section: &str) -> Vec<PathBuf> {
    photo::forget(settings, section)
}

/// The skin of the frame with this section: the one for its shape.
pub fn skin_of(settings: &Settings, section: &str) -> &'static Skin {
    photo::look_skin(Look::Dither, settings, section)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat grey of `tone` across a picture `w` x `h`.
    fn flat(tone: f32, w: usize, h: usize) -> Vec<u16> {
        vec![(tone * TONE_MAX as f32) as u16; w * h]
    }

    fn paper_share(bits: &[bool]) -> f32 {
        bits.iter().filter(|&&b| b).count() as f32 / bits.len() as f32
    }

    #[test]
    fn tones_are_linear_light_stretched_to_the_full_range() {
        // black, sRGB mid-grey and white: mid-grey is a fifth of the light
        // of white, not a half, and the ends are ink and paper
        let grey = |v: u32| 0xFF00_0000 | v << 16 | v << 8 | v;
        let mut px = vec![grey(0); 100];
        px.extend(vec![grey(128); 100]);
        px.extend(vec![grey(255); 100]);
        let t = tones(&px);
        assert_eq!((t[0], t[299]), (0, TONE_MAX));
        let mid = t[150] as f32 / TONE_MAX as f32;
        assert!((0.19..0.24).contains(&mid), "{mid}");
        // the eye's weights: green is much the lightest of the three
        let (r, g, b) = (0xFFFF_0000u32, 0xFF00_FF00, 0xFF00_00FF);
        let t = tones(&[r, g, b, grey(0), grey(255)]);
        assert!(t[1] > t[0] && t[0] > t[2]);
        // a dull, narrow picture is stretched to the full range
        let dull: Vec<u32> = (0..200).map(|i| grey(100 + i % 20)).collect();
        let t = tones(&dull);
        assert_eq!((*t.iter().min().unwrap(), *t.iter().max().unwrap()), (0, TONE_MAX));
        // and one of a single tone is left where it is
        assert!(tones(&[grey(255); 10]).iter().all(|&t| t == TONE_MAX));
    }

    #[test]
    fn floyd_steinberg_and_bayer_keep_a_grey_as_light_as_it_was() {
        let (w, h) = (64, 64);
        for tone in [0.25, 0.5, 0.75] {
            for method in [Method::Floyd, Method::Bayer] {
                let share = paper_share(&bits(&flat(tone, w, h), w, method));
                assert!((share - tone).abs() < 0.03, "{method:?} at {tone}: {share}");
            }
        }
    }

    #[test]
    fn atkinson_keeps_the_middle_and_pushes_the_rest_apart() {
        // Passing on only three quarters of the error, it keeps a mid grey
        // where it is, but lets a dark grey go darker and a light one
        // lighter: the Macintosh's contrast
        let (w, h) = (64, 64);
        let at = |tone: f32| paper_share(&bits(&flat(tone, w, h), w, Method::Atkinson));
        assert!((at(0.5) - 0.5).abs() < 0.03, "{}", at(0.5));
        assert!(at(0.25) < 0.23 && at(0.75) > 0.77, "{} {}", at(0.25), at(0.75));
        // but never so far that a grey turns over to the other side
        assert!(at(0.25) > 0.1 && at(0.75) < 0.9);
    }

    #[test]
    fn atkinson_lets_the_faintest_greys_go_where_floyd_steinberg_keeps_them() {
        let (w, h) = (64, 64);
        let faint = flat(0.04, w, h);
        let (atkinson, floyd) = (paper_share(&bits(&faint, w, Method::Atkinson)), paper_share(&bits(&faint, w, Method::Floyd)));
        assert!(atkinson < floyd / 2.0, "Atkinson {atkinson}, Floyd-Steinberg {floyd}");
        assert!((floyd - 0.04).abs() < 0.015, "{floyd}");
    }

    #[test]
    fn bayer_is_a_fixed_pattern_that_repeats_every_eight() {
        let (w, h) = (32, 16);
        let b = bits(&flat(0.3, w, h), w, Method::Bayer);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(b[y * w + x], b[(y % 8) * w + x % 8], "{x},{y}");
            }
        }
        // and every threshold of the 64 is a different one
        let mut seen = [false; 64];
        for row in BAYER {
            for v in row {
                seen[v as usize] = true;
            }
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn a_picture_is_painted_in_the_ink_and_the_paper_and_nothing_else() {
        let (ink, paper) = (0xFF4A_1A08, 0xFFF7_F3E9);
        let tones: Vec<u16> = (0..40 * 30).map(|i| ((i % 40) * TONE_MAX as u32 / 39) as u16).collect();
        for method in Method::ALL {
            let px = render(&tones, 40, method, ink, paper);
            assert_eq!(px.len(), tones.len());
            assert!(px.iter().all(|&p| p == ink || p == paper));
            // the dark left of the ramp is ink and the light right paper
            assert_eq!((px[0], px[39]), (ink, paper), "{method:?}");
        }
    }

    #[test]
    fn a_method_is_kept_by_its_name() {
        let mut settings = Settings::in_memory();
        let skin = &crate::skin::dither_wide::SKIN;
        assert_eq!(Method::of(&settings, FIRST, skin), Method::Atkinson);
        for method in Method::ALL {
            settings.set(FIRST, "Method", method.key());
            assert_eq!(Method::of(&settings, FIRST, skin), method);
        }
        settings.set(FIRST, "Method", "something else");
        assert_eq!(Method::of(&settings, FIRST, skin), Method::Atkinson);
    }
}
