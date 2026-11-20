//! Text, laid out by DirectWrite and rasterised into the canvas.
//!
//! A String meter is a DirectWrite layout drawn with aliased glyphs: a text
//! format per font, measuring for auto-sized meters, trimming for
//! `ClipString=1`, and a line-gap adjustment.  The layout is not handed to
//! Direct2D; its glyph runs arrive through IDWriteTextRenderer and each is
//! rasterised with IDWriteGlyphRunAnalysis in the aliased mode.
//!
//! The text is set the way Windows 98's GDI set it, because on/off pixels
//! leave no room for anything finer:
//!
//! - the em is a whole number of pixels (8 pt is 11 px, as GDI's
//!   `MulDiv(8, 96, 72)` has it), the sizes the font's hand-tuned widths
//!   (its `hdmx` table) and hinting were made for;
//! - the layout is GDI-compatible ("GDI classic"), so every letter is a whole
//!   number of pixels wide and every glyph starts on a pixel;
//! - nothing is kerned, so a letter is as wide whatever stands beside it.
//!
//! A natural layout instead puts letters at fractions of a pixel, and an
//! aliased glyph at x.4 and one at x.6 come out a whole pixel apart: the same
//! pair of letters then touches in one word and gapes in the next, and a
//! monitor at 2x doubles it.
//!
//! DirectWrite is COM; the handful of methods needed are called through their
//! vtable slots, which are fixed by dwrite.h.

use core::ffi::c_void;
use core::ptr::{null, null_mut};

use crate::canvas::{Canvas, Rect};
use crate::sys::{self, release, slot, Com, BOOL, HRESULT};

#[repr(C)]
#[derive(Default)]
struct TextMetrics {
    left: f32,
    top: f32,
    width: f32,
    width_including_trailing_whitespace: f32,
    height: f32,
    layout_width: f32,
    layout_height: f32,
    max_bidi_reordering_depth: u32,
    line_count: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct LineMetrics {
    length: u32,
    trailing_whitespace_length: u32,
    newline_length: u32,
    height: f32,
    baseline: f32,
    is_trimmed: BOOL,
}

#[repr(C)]
#[derive(Default)]
struct FontMetrics {
    design_units_per_em: u16,
    ascent: u16,
    descent: u16,
    line_gap: i16,
    cap_height: u16,
    x_height: u16,
    underline_position: i16,
    underline_thickness: u16,
    strikethrough_position: i16,
    strikethrough_thickness: u16,
}

#[repr(C)]
struct Trimming {
    granularity: u32,
    delimiter: u32,
    delimiter_count: u32,
}

/// DWRITE_FONT_FEATURE
#[repr(C)]
struct Feature {
    tag: u32,
    parameter: u32,
}

/// DWRITE_TEXT_RANGE
#[repr(C)]
struct Range {
    start: u32,
    length: u32,
}

#[repr(C)]
struct Matrix {
    m11: f32,
    m12: f32,
    m21: f32,
    m22: f32,
    dx: f32,
    dy: f32,
}

const WEIGHT_REGULAR: u32 = 400;
const WEIGHT_BOLD: u32 = 700;
const STYLE_NORMAL: u32 = 0;
const STRETCH_NORMAL: u32 = 5;
const ALIGN_LEADING: u32 = 0;
const ALIGN_TRAILING: u32 = 1;
const ALIGN_CENTER: u32 = 2;
const WRAP: u32 = 0;
const NO_WRAP: u32 = 1;
const TRIM_NONE: u32 = 0;
const TRIM_CHARACTER: u32 = 1;
const RENDERING_ALIASED: u32 = 1;
const TEXTURE_ALIASED_1X1: u32 = 0;
/// DWRITE_FONT_FEATURE_TAG_KERNING
const KERN: u32 = u32::from_le_bytes(*b"kern");

const IID_FACTORY: sys::GUID =
    sys::GUID { a: 0xb859ee5a, b: 0xd838, c: 0x4b5b, d: [0xa2, 0xe8, 0x1a, 0xdc, 0x7d, 0x93, 0xdb, 0x48] };

pub const CENTER: u8 = 1;
pub const RIGHT: u8 = 2;

/// A String meter, with its variables already filled in.
pub struct Spec<'a> {
    pub text: &'a str,
    pub font: usize,
    pub colour: u32,
    pub x: i32,
    pub y: i32,
    pub align: u8,
    /// -1 where the meter leaves it to the text.
    pub w: i32,
    pub h: i32,
    pub clip: bool,
    pub solid: Option<u32>,
    /// left, top, right, bottom
    pub pad: [i32; 4],
}

struct Font {
    format: Com,
    line_gap: f32,
    ellipsis: Com,
}

pub struct Text {
    factory: Com,
    fonts: Vec<Font>,
    /// Kerning off, put on every layout.
    typography: Com,
    wide: Vec<u16>,
    texture: Vec<u8>,
}

/// The em of a font of `points`, in pixels: a point is 4/3 of a pixel at
/// 96 DPI, rounded to whole pixels as GDI's `MulDiv(points, 96, 72)` rounds
/// it -- so 8 pt is the 11 px that Windows 98 drew it at.
fn em_of(points: f32) -> f32 {
    (points * (4.0 / 3.0)).round()
}

impl Text {
    /// One text format per (size in points, bold) pair the skins use.
    pub fn new(face: &str, fonts: &[(f32, bool)]) -> Option<Text> {
        type Create = unsafe extern "system" fn(u32, *const sys::GUID, *mut Com) -> HRESULT;
        unsafe {
            let create: Create = core::mem::transmute(sys::proc_address("dwrite.dll", b"DWriteCreateFactory\0")?);
            let mut factory: Com = null_mut();
            if create(0, &IID_FACTORY, &mut factory) < 0 || factory.is_null() {
                return None;
            }
            let mut text = Text { factory, fonts: Vec::new(), typography: null_mut(), wide: Vec::new(), texture: Vec::new() };

            let create_typography: unsafe extern "system" fn(Com, *mut Com) -> HRESULT = slot(factory, 16);
            if create_typography(factory, &mut text.typography) < 0 || text.typography.is_null() {
                return None;
            }
            let add_feature: unsafe extern "system" fn(Com, Feature) -> HRESULT = slot(text.typography, 3);
            if add_feature(text.typography, Feature { tag: KERN, parameter: 0 }) < 0 {
                return None;
            }

            let family = sys::wide(face);
            let locale = sys::wide("");
            for &(points, bold) in fonts {
                let size = em_of(points);
                let weight = if bold { WEIGHT_BOLD } else { WEIGHT_REGULAR };
                let create_format: unsafe extern "system" fn(
                    Com, *const u16, Com, u32, u32, u32, f32, *const u16, *mut Com,
                ) -> HRESULT = slot(factory, 15);
                let mut format: Com = null_mut();
                if create_format(factory, family.as_ptr(), null_mut(), weight, STYLE_NORMAL, STRETCH_NORMAL, size, locale.as_ptr(), &mut format) < 0 {
                    return None;
                }
                let line_gap = text.line_gap(format, &family, weight, size);
                text.fonts.push(Font { format, line_gap, ellipsis: null_mut() });
            }
            Some(text)
        }
    }

    /// The font's line gap in pixels at `size`: DirectWrite puts it above the
    /// first line, and it is taken back off, so a meter's Y is where its text
    /// starts.
    unsafe fn line_gap(&self, format: Com, family: &[u16], weight: u32, size: f32) -> f32 {
        unsafe {
            let get_collection: unsafe extern "system" fn(Com, *mut Com) -> HRESULT = slot(format, 19);
            let mut collection: Com = null_mut();
            if get_collection(format, &mut collection) < 0 || collection.is_null() {
                return 0.0;
            }
            let mut gap = 0.0;
            let find: unsafe extern "system" fn(Com, *const u16, *mut u32, *mut BOOL) -> HRESULT = slot(collection, 5);
            let (mut index, mut exists) = (0u32, 0);
            if find(collection, family.as_ptr(), &mut index, &mut exists) >= 0 && exists != 0 {
                let get_family: unsafe extern "system" fn(Com, u32, *mut Com) -> HRESULT = slot(collection, 4);
                let mut fam: Com = null_mut();
                if get_family(collection, index, &mut fam) >= 0 && !fam.is_null() {
                    let first: unsafe extern "system" fn(Com, u32, u32, u32, *mut Com) -> HRESULT = slot(fam, 7);
                    let mut font: Com = null_mut();
                    if first(fam, weight, STRETCH_NORMAL, STYLE_NORMAL, &mut font) >= 0 && !font.is_null() {
                        let metrics_of: unsafe extern "system" fn(Com, *mut FontMetrics) = slot(font, 11);
                        let mut m = FontMetrics::default();
                        metrics_of(font, &mut m);
                        if m.design_units_per_em != 0 {
                            gap = m.line_gap as f32 * (size / m.design_units_per_em as f32);
                        }
                        release(font);
                    }
                    release(fam);
                }
            }
            release(collection);
            gap
        }
    }

    /// Alignment, wrapping and trimming, as MeterStringBase::ApplyTextState
    /// and TextFormat::SetTrimming set them before measuring or drawing.
    unsafe fn prepare(&mut self, font: usize, align: u8, clip: bool) -> Com {
        unsafe {
            let f = &mut self.fonts[font];
            let set_align: unsafe extern "system" fn(Com, u32) -> HRESULT = slot(f.format, 3);
            set_align(f.format, match align {
                CENTER => ALIGN_CENTER,
                RIGHT => ALIGN_TRAILING,
                _ => ALIGN_LEADING,
            });
            if clip && f.ellipsis.is_null() {
                let sign: unsafe extern "system" fn(Com, Com, *mut Com) -> HRESULT = slot(self.factory, 20);
                sign(self.factory, f.format, &mut f.ellipsis);
            }
            let trimming = Trimming { granularity: if clip { TRIM_CHARACTER } else { TRIM_NONE }, delimiter: 0, delimiter_count: 0 };
            let set_trimming: unsafe extern "system" fn(Com, *const Trimming, Com) -> HRESULT = slot(f.format, 9);
            set_trimming(f.format, &trimming, if clip { f.ellipsis } else { null_mut() });
            let set_wrap: unsafe extern "system" fn(Com, u32) -> HRESULT = slot(f.format, 5);
            set_wrap(f.format, if clip { WRAP } else { NO_WRAP });
            f.format
        }
    }

    /// The one place a layout is made, for drawing and measuring alike: a
    /// GDI-compatible one at one pixel a DIP, in GDI's classic measuring mode
    /// (whole-pixel widths, from the font's hinting), with kerning off.
    unsafe fn layout(&self, format: Com, max_w: f32, max_h: f32) -> Com {
        unsafe {
            let create: unsafe extern "system" fn(Com, *const u16, u32, Com, f32, f32, f32, *const Matrix, BOOL, *mut Com) -> HRESULT =
                slot(self.factory, 19);
            let mut layout: Com = null_mut();
            let len = self.wide.len() as u32;
            if create(self.factory, self.wide.as_ptr(), len, format, max_w.max(0.0), max_h.max(0.0), 1.0, null(), 0, &mut layout) < 0 {
                return null_mut();
            }
            if !layout.is_null() {
                let set_typography: unsafe extern "system" fn(Com, Com, Range) -> HRESULT = slot(layout, 40);
                set_typography(layout, self.typography, Range { start: 0, length: len });
            }
            layout
        }
    }

    /// The size an auto-sized meter takes: (width, height), each already
    /// rounded up, less the line gap.
    unsafe fn measure(&mut self, format: Com, line_gap: f32) -> (f32, f32) {
        unsafe {
            let layout = self.layout(format, 10000.0, 10000.0);
            if layout.is_null() {
                return (0.0, 0.0);
            }
            let get: unsafe extern "system" fn(Com, *mut TextMetrics) -> HRESULT = slot(layout, 60);
            let mut m = TextMetrics::default();
            get(layout, &mut m);
            release(layout);
            if m.width > 0.0 {
                let gap = if self.wide.contains(&(b'\n' as u16)) { 0.0 } else { line_gap };
                (m.width.ceil(), m.height.ceil() - gap)
            } else {
                (0.0, 0.0)
            }
        }
    }

    /// How far each character of `s` moves the pen when the string is set on
    /// one line in `font`, in pixels: one entry a character, in order.  They
    /// are the layout's own figures, so text measured here breaks and takes a
    /// caret exactly where `draw` puts its glyphs.  A character that shares a
    /// glyph with the one before it -- a combining mark -- is 0 wide.
    pub fn advances(&mut self, font: usize, s: &str, out: &mut Vec<f32>) {
        /// DWRITE_CLUSTER_METRICS
        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct Cluster {
            width: f32,
            /// UTF-16 units
            length: u16,
            flags: u16,
        }
        out.clear();
        self.wide.clear();
        self.wide.extend(s.encode_utf16());
        if self.wide.is_empty() {
            return;
        }
        let mut clusters = vec![Cluster::default(); self.wide.len()];
        let mut count = 0u32;
        unsafe {
            let format = self.prepare(font, 0, false);
            let layout = self.layout(format, 100_000.0, 1000.0);
            if !layout.is_null() {
                let get: unsafe extern "system" fn(Com, *mut Cluster, u32, *mut u32) -> HRESULT = slot(layout, 62);
                if get(layout, clusters.as_mut_ptr(), clusters.len() as u32, &mut count) < 0 {
                    count = 0;
                }
                release(layout);
            }
        }
        // A cluster's width goes to the unit it starts on.
        let mut by_unit = vec![0f32; self.wide.len()];
        let mut unit = 0;
        for c in &clusters[..(count as usize).min(clusters.len())] {
            if let Some(w) = by_unit.get_mut(unit) {
                *w = c.width;
            }
            unit += c.length as usize;
        }
        let mut unit = 0;
        for c in s.chars() {
            // no layout to ask: the width of a digit, as good a guess as any
            out.push(if count == 0 { 6.0 } else { by_unit[unit] });
            unit += c.len_utf16();
        }
    }

    /// Draw a String meter; returns the rectangle the meter occupies.
    pub fn draw(&mut self, canvas: &mut Canvas, s: &Spec) -> Rect {
        self.wide.clear();
        self.wide.extend(s.text.encode_utf16().filter(|&c| c != b'\r' as u16));
        unsafe {
            let format = self.prepare(s.font, s.align, s.clip);
            let line_gap = self.fonts[s.font].line_gap;

            // Meter::m_W and m_H hold the padding as well as the text.
            let (pad_w, pad_h) = (s.pad[0] + s.pad[2], s.pad[1] + s.pad[3]);
            let (mut w, mut h) = (s.w + pad_w, s.h + pad_h);
            if s.w < 0 || s.h < 0 {
                let (tw, th) = self.measure(format, line_gap);
                if s.w < 0 {
                    w = tw as i32 + pad_w;
                }
                if s.h < 0 {
                    h = th as i32 + pad_h;
                }
            }

            // MeterStringBase::GetX: the anchor is the middle or the right edge.
            let x = match s.align {
                CENTER => s.x - w / 2,
                RIGHT => s.x - w,
                _ => s.x,
            };
            let meter = Rect::xywh(x, s.y, w, h);
            if let Some(solid) = s.solid {
                canvas.fill(x, s.y, w, h, solid);
            }
            if self.wide.is_empty() || !meter.intersects(&Rect::new(canvas.clip.x0 - 64, canvas.clip.y0 - 64, canvas.clip.x1 + 64, canvas.clip.y1 + 64)) {
                return meter;
            }

            // Meter::GetMeterRectPadding: the box the text is laid out in.
            let left = (x + s.pad[0]) as f32;
            let top = (s.y + s.pad[1]) as f32;
            let right = (x + w - s.pad[2]) as f32;
            let bottom = (s.y + h - s.pad[3]) as f32;

            // TextFormat::CreateLayout
            let layout = self.layout(format, right - left, bottom - top + if s.clip { 2.0 } else { 0.0 });
            if layout.is_null() {
                return meter;
            }
            if s.clip {
                // One visible line: stop wrapping, so trimming shows as much
                // of it as fits before the ellipsis.
                let lines_of: unsafe extern "system" fn(Com, *mut LineMetrics, u32, *mut u32) -> HRESULT = slot(layout, 59);
                let mut lines = [LineMetrics::default(); 2];
                let mut count = 0u32;
                let hr = lines_of(layout, lines.as_mut_ptr(), 2, &mut count);
                // E_NOT_SUFFICIENT_BUFFER still fills the two it was given.
                if (hr >= 0 || count > 2) && count >= 2 && lines[0].is_trimmed != 0 && lines[1].is_trimmed != 0 && lines[1].height == 0.0 {
                    let set_wrap: unsafe extern "system" fn(Com, u32) -> HRESULT = slot(layout, 5);
                    set_wrap(layout, NO_WRAP);
                }
            }

            // Canvas::DrawTextW: the layout goes down at the box's corner,
            // raised by the line gap, and is clipped to the box if trimmed.
            let within = if s.clip {
                Rect::new(left as i32, top as i32, right as i32, bottom as i32)
            } else {
                Rect::new(i32::MIN / 2, i32::MIN / 2, i32::MAX / 2, i32::MAX / 2)
            };
            let mut renderer = Renderer {
                vtable: &RENDERER_VTABLE,
                factory: self.factory,
                canvas,
                colour: s.colour,
                within,
                texture: &mut self.texture,
            };
            let draw: unsafe extern "system" fn(Com, *mut c_void, *mut Renderer, f32, f32) -> HRESULT = slot(layout, 58);
            draw(layout, null_mut(), &mut renderer, left, top - line_gap);
            release(layout);
            meter
        }
    }
}

impl Drop for Text {
    fn drop(&mut self) {
        unsafe {
            for f in &self.fonts {
                release(f.ellipsis);
                release(f.format);
            }
            release(self.typography);
            release(self.factory);
        }
    }
}

// ---------------------------------------------------------------------------
//  IDWriteTextRenderer, implemented on the stack for the length of one Draw
// ---------------------------------------------------------------------------

#[repr(C)]
struct Renderer<'a> {
    vtable: *const RendererVtable,
    factory: Com,
    canvas: &'a mut Canvas,
    colour: u32,
    within: Rect,
    texture: &'a mut Vec<u8>,
}

#[repr(C)]
struct RendererVtable {
    query_interface: unsafe extern "system" fn(*mut Renderer, *const sys::GUID, *mut Com) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut Renderer) -> u32,
    release: unsafe extern "system" fn(*mut Renderer) -> u32,
    is_pixel_snapping_disabled: unsafe extern "system" fn(*mut Renderer, *mut c_void, *mut BOOL) -> HRESULT,
    get_current_transform: unsafe extern "system" fn(*mut Renderer, *mut c_void, *mut Matrix) -> HRESULT,
    get_pixels_per_dip: unsafe extern "system" fn(*mut Renderer, *mut c_void, *mut f32) -> HRESULT,
    draw_glyph_run: unsafe extern "system" fn(*mut Renderer, *mut c_void, f32, f32, u32, *const c_void, *const c_void, Com) -> HRESULT,
    draw_underline: unsafe extern "system" fn(*mut Renderer, *mut c_void, f32, f32, *const c_void, Com) -> HRESULT,
    draw_strikethrough: unsafe extern "system" fn(*mut Renderer, *mut c_void, f32, f32, *const c_void, Com) -> HRESULT,
    draw_inline_object: unsafe extern "system" fn(*mut Renderer, *mut c_void, f32, f32, Com, BOOL, BOOL, Com) -> HRESULT,
}

static RENDERER_VTABLE: RendererVtable = RendererVtable {
    query_interface,
    add_ref,
    release: add_ref,
    is_pixel_snapping_disabled,
    get_current_transform,
    get_pixels_per_dip,
    draw_glyph_run,
    draw_underline: draw_decoration,
    draw_strikethrough: draw_decoration,
    draw_inline_object,
};

unsafe extern "system" fn query_interface(this: *mut Renderer, iid: *const sys::GUID, out: *mut Com) -> HRESULT {
    // IUnknown, IDWritePixelSnapping, IDWriteTextRenderer -- and nothing newer:
    // DirectWrite asks for IDWriteTextRenderer1 first, and saying yes to that
    // would have it call methods this table does not have.
    const MINE: [(u32, u16, u16, [u8; 8]); 3] = [
        (0x00000000, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]),
        (0xeaf3a2da, 0xecf4, 0x4d24, [0xb6, 0x44, 0xb3, 0x4f, 0x68, 0x42, 0x02, 0x4b]),
        (0xef8a8135, 0x5cc6, 0x45fe, [0x88, 0x25, 0xc5, 0xa0, 0x72, 0x4e, 0xb8, 0x19]),
    ];
    unsafe {
        let id = &*iid;
        if MINE.iter().any(|m| (id.a, id.b, id.c, id.d) == *m) {
            *out = this as Com;
            0
        } else {
            *out = null_mut();
            0x8000_4002u32 as HRESULT // E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn add_ref(_this: *mut Renderer) -> u32 {
    1
}

unsafe extern "system" fn is_pixel_snapping_disabled(_this: *mut Renderer, _ctx: *mut c_void, out: *mut BOOL) -> HRESULT {
    unsafe { *out = 0 };
    0
}

unsafe extern "system" fn get_current_transform(_this: *mut Renderer, _ctx: *mut c_void, out: *mut Matrix) -> HRESULT {
    unsafe { *out = Matrix { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 } };
    0
}

unsafe extern "system" fn get_pixels_per_dip(_this: *mut Renderer, _ctx: *mut c_void, out: *mut f32) -> HRESULT {
    unsafe { *out = 1.0 };
    0
}

unsafe extern "system" fn draw_glyph_run(
    this: *mut Renderer, _ctx: *mut c_void, x: f32, y: f32, measuring: u32,
    run: *const c_void, _description: *const c_void, _effect: Com,
) -> HRESULT {
    unsafe {
        let r = &mut *this;
        let analyse: unsafe extern "system" fn(Com, *const c_void, f32, *const Matrix, u32, u32, f32, f32, *mut Com) -> HRESULT =
            slot(r.factory, 23);
        let mut analysis: Com = null_mut();
        // Direct2D snaps a run's baseline to the pixel grid before rasterising;
        // its start goes on a pixel too.  The layout's widths and centring
        // are whole pixels already, so every glyph lands on one; this keeps it
        // so for what DirectWrite sets out by itself, such as the trimming
        // ellipsis.  Rounded as `field::px` rounds a caret.
        let hr = analyse(r.factory, run, 1.0, null(), RENDERING_ALIASED, measuring, (x + 0.5).floor(), (y + 0.5).floor(), &mut analysis);
        if hr < 0 || analysis.is_null() {
            return 0;
        }
        let bounds_of: unsafe extern "system" fn(Com, u32, *mut sys::RECT) -> HRESULT = slot(analysis, 3);
        let mut b = sys::RECT::default();
        if bounds_of(analysis, TEXTURE_ALIASED_1X1, &mut b) >= 0 && b.right > b.left && b.bottom > b.top {
            let (w, h) = (b.right - b.left, b.bottom - b.top);
            r.texture.clear();
            r.texture.resize((w * h) as usize, 0);
            let fill: unsafe extern "system" fn(Com, u32, *const sys::RECT, *mut u8, u32) -> HRESULT = slot(analysis, 4);
            if fill(analysis, TEXTURE_ALIASED_1X1, &b, r.texture.as_mut_ptr(), (w * h) as u32) >= 0 {
                r.canvas.mask(b.left, b.top, w, h, r.texture, r.colour, &r.within);
            }
        }
        release(analysis);
        0
    }
}

unsafe extern "system" fn draw_decoration(_this: *mut Renderer, _ctx: *mut c_void, _x: f32, _y: f32, _what: *const c_void, _effect: Com) -> HRESULT {
    0
}

/// The trimming ellipsis: it draws itself, back through this renderer.
unsafe extern "system" fn draw_inline_object(
    this: *mut Renderer, ctx: *mut c_void, x: f32, y: f32, object: Com, sideways: BOOL, rtl: BOOL, effect: Com,
) -> HRESULT {
    unsafe {
        if object.is_null() {
            return 0;
        }
        let draw: unsafe extern "system" fn(Com, *mut c_void, *mut Renderer, f32, f32, BOOL, BOOL, Com) -> HRESULT = slot(object, 3);
        draw(object, ctx, this, x, y, sideways, rtl, effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin;

    fn text() -> Text {
        Text::new(skin::datetime::SKIN.var("Font"), skin::FONTS).unwrap()
    }

    /// Which of the host's fonts is the skins' 8 pt, plain or bold.
    fn eight(bold: bool) -> usize {
        skin::FONTS.iter().position(|&f| f == (8.0, bold)).unwrap()
    }

    fn advances(text: &mut Text, font: usize, s: &str) -> Vec<f32> {
        let mut out = Vec::new();
        text.advances(font, s, &mut out);
        out
    }

    /// A blank 200 x 40 picture with each (string, x, align, room) drawn on
    /// it, at y 10, as a String meter would draw it.
    fn picture(text: &mut Text, font: usize, strings: &[(&str, i32, u8, i32)]) -> Vec<u32> {
        let (w, h) = (200, 40);
        let mut px = vec![0u32; (w * h) as usize];
        let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), w, w, h) };
        for &(s, x, align, room) in strings {
            text.draw(&mut canvas, &Spec {
                text: s,
                font,
                colour: 0xFF00_0000,
                x,
                y: 10,
                align,
                w: room,
                h: 14,
                clip: align == CENTER,
                solid: None,
                pad: [0; 4],
            });
        }
        drop(canvas);
        assert!(px.iter().any(|&p| p != 0), "nothing was drawn");
        px
    }

    #[test]
    fn every_letter_is_a_whole_number_of_pixels_wide() {
        let mut text = text();
        for font in 0..skin::FONTS.len() {
            let each = advances(&mut text, font, "Resource Meter CPU usage RAM usage Today's Weather 0123456789 % -4\u{b0}C");
            assert!(!each.is_empty());
            for (c, w) in "Resource Meter CPU usage RAM usage Today's Weather 0123456789 % -4\u{b0}C".chars().zip(&each) {
                assert!(*w > 0.0 && w.fract() == 0.0, "{c:?} in font {font} is {w} px wide");
            }
        }
    }

    #[test]
    fn a_letter_is_as_wide_whatever_stands_beside_it() {
        // Microsoft Sans Serif kerns no Latin pair, so Segoe UI, which kerns
        // all of these, shows nothing is kerned whatever font a skin names.
        for face in [skin::datetime::SKIN.var("Font"), "Segoe UI"] {
            let mut text = Text::new(face, skin::FONTS).unwrap();
            for font in 0..skin::FONTS.len() {
                for pair in ["AV", "AT", "AY", "Te", "To", "Ty", "LT", "LY", "Wa", "Yo", "P.", "r.", "F,"] {
                    let alone: Vec<f32> = pair.chars().map(|c| advances(&mut text, font, &c.to_string())[0]).collect();
                    assert_eq!(advances(&mut text, font, pair), alone, "{pair:?} in {face}, font {font}");
                }
            }
        }
    }

    #[test]
    fn a_string_is_drawn_exactly_as_its_letters_are_one_by_one() {
        // The report: letters ran into each other in one word and stood apart
        // in the next, because they fell between pixels.
        let mut text = text();
        for bold in [false, true] {
            let font = eight(bold);
            for s in ["Resource Meter", "CPU usage", "RAM usage", "Today's Weather"] {
                let whole = picture(&mut text, font, &[(s, 7, 0, 180)]);
                let each = advances(&mut text, font, s);
                let mut x = 7.0;
                let mut letters = Vec::new();
                for (i, c) in s.char_indices() {
                    letters.push((&s[i..i + c.len_utf8()], x as i32, 0, 180));
                    x += each[letters.len() - 1];
                }
                assert!(picture(&mut text, font, &letters) == whole, "{s:?}, bold {bold}");
            }
        }
    }

    #[test]
    fn a_centred_name_starts_on_the_column_a_caret_is_put_by() {
        // field.rs puts a centred name's caret stops (room - width) / 2 in,
        // with the half pixel an odd room leaves dropped, as GDI's DT_CENTER
        // drops it -- and so does the layout.  They have to agree in a room an
        // odd number of px wider than the name and in an even one.
        let mut text = text();
        let font = eight(false);
        for name in ["Lviv", "Kyiv", "Partly Cloudy"] {
            let width: f32 = advances(&mut text, font, name).iter().sum();
            for room in [100, 101] {
                let centred = picture(&mut text, font, &[(name, 20 + room / 2, CENTER, room)]);
                let left = 20 + ((room as f32 - width) / 2.0).floor() as i32;
                assert!(picture(&mut text, font, &[(name, left, 0, 180)]) == centred, "{name:?} in {room}");
            }
        }
    }
}
