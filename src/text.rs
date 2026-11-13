//! Text, laid out by DirectWrite and rasterised into the canvas.
//!
//! A String meter is a natural (not GDI-compatible) DirectWrite layout drawn
//! with aliased glyphs: a text format per font, measuring for auto-sized
//! meters, trimming for `ClipString=1`, and a line-gap adjustment.  The layout
//! is not handed to Direct2D; its glyph runs arrive through IDWriteTextRenderer
//! and each is rasterised with IDWriteGlyphRunAnalysis in the aliased mode.
//! The system's font engine and glyph positions, with no Direct2D.
//!
//! DirectWrite is COM; the handful of methods needed are called through their
//! vtable slots, which are fixed by dwrite.h.

use core::ffi::c_void;
use core::ptr::{null, null_mut};

use crate::canvas::{Canvas, Rect};
use crate::sys::{self, BOOL, HRESULT};

type Com = *mut c_void;

/// The function in slot `index` of a COM object's vtable.
#[inline]
unsafe fn slot<F: Copy>(obj: Com, index: usize) -> F {
    unsafe {
        let table = *(obj as *const *const *const c_void);
        core::mem::transmute_copy(&*table.add(index))
    }
}

unsafe fn release(obj: Com) {
    if !obj.is_null() {
        unsafe {
            let f: unsafe extern "system" fn(Com) -> u32 = slot(obj, 2);
            f(obj);
        }
    }
}

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
    wide: Vec<u16>,
    texture: Vec<u8>,
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
            let mut text = Text { factory, fonts: Vec::new(), wide: Vec::new(), texture: Vec::new() };
            let family = sys::wide(face);
            let locale = sys::wide("");
            for &(points, bold) in fonts {
                // A point is 4/3 of a DIP, whatever the display.
                let size = points * (4.0 / 3.0);
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

    unsafe fn layout(&self, format: Com, max_w: f32, max_h: f32) -> Com {
        unsafe {
            let create: unsafe extern "system" fn(Com, *const u16, u32, Com, f32, f32, *mut Com) -> HRESULT =
                slot(self.factory, 18);
            let mut layout: Com = null_mut();
            create(self.factory, self.wide.as_ptr(), self.wide.len() as u32, format, max_w.max(0.0), max_h.max(0.0), &mut layout);
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
        // Direct2D snaps a run's baseline to the pixel grid before rasterising.
        let hr = analyse(r.factory, run, 1.0, null(), RENDERING_ALIASED, measuring, x, (y + 0.5).floor(), &mut analysis);
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
