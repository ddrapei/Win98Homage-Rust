//! A software canvas: 32-bit premultiplied BGRA, the format a layered window
//! is updated from.
//!
//! Direct2D would cost tens of megabytes for what is, in these skins, a few
//! hundred pixel-aligned rectangles, a dozen small polygons and some bilevel
//! text.  This does that drawing in a few hundred lines: rectangles are plain
//! fills, and anything with an edge that is not on the pixel grid -- the clock
//! hands, the faces of the usage blocks, the half-pixel dashes of the graph's
//! floor -- is filled by exact area coverage.
//!
//! Everything honours `clip`, so repainting the clock every second touches
//! only the clock.

/// Half-open pixel rectangle: x0..x1 by y0..y1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    pub const fn xywh(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x0: x, y0: y, x1: x + w, y1: y + h }
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }

    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        !self.intersect(o).is_empty()
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// `argb` (straight alpha) at coverage `cov`, over a premultiplied pixel.
#[inline]
fn blend(dst: u32, argb: u32, cov: f32) -> u32 {
    let a = (argb >> 24) as f32 * (1.0 / 255.0) * cov;
    if a <= 0.0 {
        return dst;
    }
    let inv = 1.0 - a;
    let ch = |shift: u32| -> u32 {
        let s = ((argb >> shift) & 255) as f32;
        let d = ((dst >> shift) & 255) as f32;
        ((s * a + d * inv + 0.5) as u32).min(255)
    };
    let alpha = ((255.0 * a + (dst >> 24) as f32 * inv + 0.5) as u32).min(255);
    alpha << 24 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

pub struct Canvas {
    px: *mut u32,
    /// Pixels per row of the buffer, which may be wider than the drawing.
    stride: i32,
    pub w: i32,
    pub h: i32,
    pub clip: Rect,
    /// Scratch for the polygon filler, kept so a repaint allocates nothing.
    acc: Vec<f32>,
}

impl Canvas {
    /// # Safety
    /// `px` points at `stride * rows` pixels, `rows >= h`, that outlive the canvas.
    pub unsafe fn new(px: *mut u32, stride: i32, w: i32, h: i32) -> Canvas {
        Canvas { px, stride, w, h, clip: Rect::new(0, 0, w, h), acc: Vec::new() }
    }

    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r.intersect(&Rect::new(0, 0, self.w, self.h));
    }

    #[inline]
    fn row(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u32] {
        // A rectangle clipped away in one direction can still have rows in
        // the other; such a row is simply empty.
        if x1 <= x0 || y < 0 || y >= self.h || x0 < 0 || x1 > self.w {
            return &mut [];
        }
        // SAFETY: just checked to lie inside w x h, which the buffer covers.
        unsafe {
            core::slice::from_raw_parts_mut(self.px.add((y * self.stride + x0) as usize), (x1 - x0) as usize)
        }
    }

    /// Back to fully transparent.
    pub fn clear(&mut self, r: Rect) {
        let r = r.intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        for y in r.y0..r.y1 {
            self.row(y, r.x0, r.x1).fill(0);
        }
    }

    /// A pixel-aligned rectangle.
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, argb: u32) {
        let r = Rect::xywh(x, y, w, h).intersect(&self.clip);
        if r.is_empty() || argb >> 24 == 0 {
            return;
        }
        let opaque = argb >> 24 == 255;
        for yy in r.y0..r.y1 {
            let row = self.row(yy, r.x0, r.x1);
            if opaque {
                row.fill(argb);
            } else {
                for p in row {
                    *p = blend(*p, argb, 1.0);
                }
            }
        }
    }

    /// A rectangle with edges anywhere: each pixel gets the share of it the
    /// rectangle covers.
    pub fn fill_f(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, argb: u32) {
        let r = Rect::new(x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32, y1.ceil() as i32)
            .intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        for yy in r.y0..r.y1 {
            let cy = (y1.min(yy as f32 + 1.0) - y0.max(yy as f32)).clamp(0.0, 1.0);
            let row = self.row(yy, r.x0, r.x1);
            for (i, p) in row.iter_mut().enumerate() {
                let xx = (r.x0 + i as i32) as f32;
                let cx = (x1.min(xx + 1.0) - x0.max(xx)).clamp(0.0, 1.0);
                let cov = cx * cy;
                if cov > 0.0 {
                    *p = blend(*p, argb, cov);
                }
            }
        }
    }

    /// A horizontal line of `width`, solid or dashed (`on` drawn, `off`
    /// skipped, starting drawn at `x0`), with flat ends.
    pub fn hline(&mut self, x0: f32, y: f32, x1: f32, width: f32, on: f32, off: f32, argb: u32) {
        let (top, bottom) = (y - width / 2.0, y + width / 2.0);
        if on <= 0.0 || off <= 0.0 {
            return self.fill_f(x0, top, x1, bottom, argb);
        }
        let mut x = x0;
        while x < x1 {
            self.fill_f(x, top, (x + on).min(x1), bottom, argb);
            x += on + off;
        }
    }

    /// A left-to-right gradient between two opaque colours, sampled at pixel
    /// centres.
    pub fn hgrad(&mut self, x: i32, y: i32, w: i32, h: i32, c0: u32, c1: u32) {
        let r = Rect::xywh(x, y, w, h).intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        for xx in r.x0..r.x1 {
            let t = ((xx - x) as f32 + 0.5) / w as f32;
            let ch = |shift: u32| -> u32 {
                let a = ((c0 >> shift) & 255) as f32;
                let b = ((c1 >> shift) & 255) as f32;
                (a + (b - a) * t + 0.5) as u32
            };
            let c = 0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0);
            for yy in r.y0..r.y1 {
                self.row(yy, xx, xx + 1)[0] = c;
            }
        }
    }

    /// A closed polygon, filled by exact area coverage.
    pub fn poly(&mut self, pts: &[(f32, f32)], argb: u32) {
        if pts.len() < 3 {
            return;
        }
        let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in pts {
            minx = minx.min(x);
            miny = miny.min(y);
            maxx = maxx.max(x);
            maxy = maxy.max(y);
        }
        let (bx, by) = (minx.floor() as i32, miny.floor() as i32);
        let bw = (maxx.ceil() as i32 - bx).max(1);
        let bh = (maxy.ceil() as i32 - by).max(1);
        if !Rect::xywh(bx, by, bw, bh).intersects(&self.clip) {
            return;
        }

        // Signed-area accumulation: each edge deposits, per pixel it crosses,
        // the area to its right; a running sum along the row is the coverage.
        let aw = (bw + 3) as usize;
        let mut acc = core::mem::take(&mut self.acc);
        acc.clear();
        acc.resize(aw * bh as usize, 0.0);
        let n = pts.len();
        for i in 0..n {
            let (ax, ay) = pts[i];
            let (cx, cy) = pts[(i + 1) % n];
            edge(&mut acc, aw, bh as usize, ax - bx as f32, ay - by as f32, cx - bx as f32, cy - by as f32);
        }

        let r = Rect::xywh(bx, by, bw, bh).intersect(&self.clip);
        for yy in r.y0..r.y1.max(r.y0) {
            let line = &acc[(yy - by) as usize * aw..][..aw];
            let mut sum = 0.0f32;
            for (i, a) in line.iter().enumerate().take(bw as usize) {
                sum += *a;
                let xx = bx + i as i32;
                if xx < r.x0 || xx >= r.x1 {
                    continue;
                }
                let cov = sum.abs().min(1.0);
                if cov > 1.0 / 1024.0 {
                    let p = &mut self.row(yy, xx, xx + 1)[0];
                    *p = blend(*p, argb, cov);
                }
            }
        }
        self.acc = acc;
    }

    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, argb: u32) {
        const N: usize = 96;
        let mut pts = [(0.0f32, 0.0f32); N];
        for (i, p) in pts.iter_mut().enumerate() {
            let t = i as f32 * (core::f32::consts::TAU / N as f32);
            *p = (cx + rx * t.cos(), cy + ry * t.sin());
        }
        self.poly(&pts, argb);
    }

    /// A premultiplied BGRA image, unscaled.
    pub fn image(&mut self, x: i32, y: i32, w: i32, h: i32, bgra: &[u8]) {
        let r = Rect::xywh(x, y, w, h).intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        for yy in r.y0..r.y1 {
            let row = self.row(yy, r.x0, r.x1);
            for (i, p) in row.iter_mut().enumerate() {
                let s = (((yy - y) * w + (r.x0 - x) + i as i32) * 4) as usize;
                let (sb, sg, sr, sa) = (bgra[s] as u32, bgra[s + 1] as u32, bgra[s + 2] as u32, bgra[s + 3] as u32);
                if sa == 255 {
                    *p = 0xFF00_0000 | sr << 16 | sg << 8 | sb;
                } else if sa != 0 {
                    let (d, inv) = (*p, 255 - sa);
                    let over = |src: u32, shift: u32| (src + (((d >> shift) & 255) * inv + 127) / 255).min(255);
                    *p = over(sa, 24) << 24 | over(sr, 16) << 16 | over(sg, 8) << 8 | over(sb, 0);
                }
            }
        }
    }

    /// A bilevel glyph bitmap (one byte a pixel, 0 or 255) in one colour,
    /// kept inside `within` as well as the clip.
    pub fn mask(&mut self, x: i32, y: i32, w: i32, h: i32, alpha: &[u8], argb: u32, within: &Rect) {
        let r = Rect::xywh(x, y, w, h).intersect(&self.clip).intersect(within);
        if r.is_empty() {
            return;
        }
        let opaque = argb >> 24 == 255;
        for yy in r.y0..r.y1 {
            let row = self.row(yy, r.x0, r.x1);
            let src = &alpha[((yy - y) * w + (r.x0 - x)) as usize..];
            for (p, &a) in row.iter_mut().zip(src) {
                if a >= 128 {
                    *p = if opaque { argb } else { blend(*p, argb, 1.0) };
                }
            }
        }
    }
}

/// One polygon edge into the accumulation buffer (after font-rs).
fn edge(acc: &mut [f32], aw: usize, ah: usize, x0: f32, y0: f32, x1: f32, y1: f32) {
    if y0 == y1 {
        return;
    }
    let (dir, x0, y0, x1, y1) = if y0 < y1 { (1.0, x0, y0, x1, y1) } else { (-1.0, x1, y1, x0, y0) };
    let dxdy = (x1 - x0) / (y1 - y0);
    let mut x = x0;
    let first = y0.max(0.0) as usize;
    if y0 < 0.0 {
        x -= y0 * dxdy;
    }
    let last = (y1.ceil() as usize).min(ah);
    for y in first..last {
        let line = &mut acc[y * aw..(y + 1) * aw];
        let dy = ((y + 1) as f32).min(y1) - (y as f32).max(y0);
        let xnext = x + dxdy * dy;
        let d = dy * dir;
        let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let xa_floor = xa.floor();
        let xai = xa_floor as i32;
        let xb_ceil = xb.ceil();
        let xbi = xb_ceil as i32;
        let at = |i: i32| (i.max(0) as usize).min(aw - 1);
        if xbi <= xai + 1 {
            let xmf = 0.5 * (x + xnext) - xa_floor;
            line[at(xai)] += d - d * xmf;
            line[at(xai + 1)] += d * xmf;
        } else {
            let s = (xb - xa).recip();
            let xaf = xa - xa_floor;
            let a0 = 0.5 * s * (1.0 - xaf) * (1.0 - xaf);
            let xbf = xb - xb_ceil + 1.0;
            let am = 0.5 * s * xbf * xbf;
            line[at(xai)] += d * a0;
            if xbi == xai + 2 {
                line[at(xai + 1)] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - xaf);
                line[at(xai + 1)] += d * (a1 - a0);
                for xi in xai + 2..xbi - 1 {
                    line[at(xi)] += d * s;
                }
                let a2 = a1 + (xbi - xai - 3) as f32 * s;
                line[at(xbi - 1)] += d * (1.0 - a2 - am);
            }
            line[at(xbi)] += d * am;
        }
        x = xnext;
    }
}
