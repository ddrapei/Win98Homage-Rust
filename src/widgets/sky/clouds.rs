//! Clouds as pixel art paints them: heaps of round puffs, each lit as a ball
//! is, in a few flat bands of colour.
//!
//! A cloud is its form -- the puffs, back to front, and the row its flat
//! foot stands on, if it has one -- and the form is shaded once for the
//! light it is in: every pixel gets one of five tones, from the shade under
//! a bump to the highlight on top of one.  Puffs painted later stand in
//! front of those painted earlier, so each bump's lit crown shows against
//! the shaded underside of the one behind it: the cauliflower look.  The
//! tones are coloured as the clouds are drawn, from whatever ramp the light
//! of the moment gives -- white and blue-grey by day, peach and violet at
//! dusk, navy by night, slate in a storm.
//!
//! The same shading makes the crowns of round trees, in leaves' colours.

use super::{Rng, BAYER};

/// One puff: a ball of cloud, as round or as flattened as `ry` against `rx`.
#[derive(Clone, Copy, Debug)]
pub struct Puff {
    pub x: f32,
    pub y: f32,
    pub rx: f32,
    pub ry: f32,
}

/// A cloud's form, in a box `w` by `h`: its puffs, back to front, and the
/// row its flat foot stands on -- below the box, for a form with none.
pub struct Form {
    pub w: i32,
    pub h: i32,
    pub puffs: Vec<Puff>,
    pub base: f32,
    /// How much lighter its crown is than its foot.
    pub rise: f32,
}

impl Form {
    /// Fair-weather cumulus: a row of broad puffs along a flat foot, and on
    /// them smaller ones, and on those smaller again, each on the upper side
    /// of the one it grows from, the middle rising highest.
    pub fn cumulus(rng: &mut Rng, w: i32, h: i32) -> Form {
        let (wf, hf) = (w as f32, h as f32);
        let base = hf - 0.5;
        let mut puffs = Vec::new();
        let n = ((wf / (hf * 0.7)).ceil() as usize).max(2);
        for i in 0..n {
            let fx = (i as f32 + 0.5) / n as f32;
            let middle = 1.0 - (2.0 * fx - 1.0).abs();
            let r = hf * (0.2 + 0.18 * middle) * rng.range(0.85, 1.15);
            puffs.push(Puff { x: fx * wf + rng.range(-2.0, 2.0), y: base - r * 0.5, rx: r * 1.25, ry: r });
        }
        heap(rng, &mut puffs, wf, hf, 3, |fx| hf * 0.92 * (1.0 - (1.0 - (2.0 * fx - 1.0).powi(2)).max(0.0).sqrt()));
        settle(&mut puffs, wf, hf);
        Form { w, h, puffs, base, rise: 0.24 }
    }

    /// A layer of cloud -- stratocumulus, the stuff of a grey day: broad
    /// rolls side by side, flatter than a cumulus, with a few bumps on top
    /// and the gaps between them showing the sky.
    pub fn layer(rng: &mut Rng, w: i32, h: i32) -> Form {
        let (wf, hf) = (w as f32, h as f32);
        let base = hf - 0.5;
        let mut puffs = Vec::new();
        let n = ((wf / (hf * 1.1)).ceil() as usize).max(2);
        for i in 0..n {
            let fx = (i as f32 + 0.5) / n as f32;
            let ry = hf * rng.range(0.28, 0.5);
            puffs.push(Puff { x: fx * wf + rng.range(-4.0, 4.0), y: base - ry * rng.range(0.55, 0.9), rx: ry * rng.range(1.4, 2.2), ry });
        }
        heap(rng, &mut puffs, wf, hf, 2, |_| 0.0);
        settle(&mut puffs, wf, hf);
        Form { w, h, puffs, base, rise: 0.24 }
    }

    /// A tree's crown: a cumulus with no flat foot, rounded all over.
    pub fn crown(rng: &mut Rng, w: i32, h: i32) -> Form {
        let (wf, hf) = (w as f32, h as f32);
        let mut puffs = Vec::new();
        let n = ((wf / (hf * 0.6)).ceil() as usize).max(2);
        for i in 0..n {
            let fx = (i as f32 + 0.5) / n as f32;
            let middle = 1.0 - (2.0 * fx - 1.0).abs();
            let r = hf * (0.22 + 0.16 * middle) * rng.range(0.9, 1.1);
            puffs.push(Puff { x: fx * wf, y: hf - r - 0.5 - (1.0 - middle) * hf * 0.12, rx: r * 1.1, ry: r });
        }
        heap(rng, &mut puffs, wf, hf, 3, |fx| hf * 0.9 * (1.0 - (1.0 - (2.0 * fx - 1.0).powi(2)).max(0.0).sqrt()));
        settle(&mut puffs, wf, hf);
        Form { w, h, puffs, base: hf + 100.0, rise: 0.24 }
    }
}

/// Grow `generations` of smaller puffs on the upper sides of the ones there
/// are, kept inside the box and under `roof` -- the highest a puff's top may
/// stand at a share of the width.
fn heap(rng: &mut Rng, puffs: &mut Vec<Puff>, wf: f32, hf: f32, generations: u32, roof: impl Fn(f32) -> f32) {
    let mut last: Vec<Puff> = puffs.clone();
    for generation in 0..generations {
        let mut next = Vec::new();
        for p in &last {
            let kids = if generation == 0 { 2 + (rng.unit() < 0.5) as usize } else { 1 + (rng.unit() < 0.55) as usize };
            for _ in 0..kids {
                // somewhere on its upper half, left to right
                let a = rng.range(-2.7, -0.45);
                let d = p.ry * rng.range(0.5, 0.85);
                let r = p.ry * rng.range(0.5, 0.78);
                let (x, y) = (p.x + a.cos() * d * (p.rx / p.ry), p.y + a.sin() * d);
                let rx = r * rng.range(1.0, 1.25);
                if r < 1.4 || x - rx < 0.0 || x + rx > wf || y - r < roof(x / wf) - 1.0 || y - r < 0.0 || y + r > hf + 2.0 {
                    continue;
                }
                next.push(Puff { x, y, rx, ry: r });
            }
        }
        puffs.extend(next.iter().copied());
        last = next;
    }
}

/// Every puff inside the box -- one that ran over its edge would be cut off
/// straight there -- and the high ones behind, the low ones in front: a
/// lower puff's lit top stands against the shaded foot of the one above it.
fn settle(puffs: &mut [Puff], wf: f32, hf: f32) {
    for p in puffs.iter_mut() {
        p.rx = p.rx.min(wf / 2.0 - 0.5).max(1.0);
        p.ry = p.ry.min(hf / 2.0).max(1.0);
        p.x = p.x.clamp(p.rx, (wf - p.rx).max(p.rx));
        p.y = p.y.max(p.ry);
    }
    puffs.sort_by(|a, b| (a.y + a.ry * 0.5).total_cmp(&(b.y + b.ry * 0.5)));
}

/// The form in light falling along `light` -- x to the right, y down, z out
/// of the picture, as a unit vector pointing back towards the light -- as a
/// tone a pixel: 0 for none, 1 the darkest to 5 the lightest.  `grain`
/// dithers the edges between tones a little, as a hand-made picture's are.
pub fn shade(form: &Form, light: [f32; 3], grain: f32) -> Vec<u8> {
    let (w, h) = (form.w, form.h);
    let mut px = vec![0u8; (w * h).max(0) as usize];
    for p in &form.puffs {
        let x0 = ((p.x - p.rx).floor() as i32).max(0);
        let x1 = ((p.x + p.rx).ceil() as i32).min(w);
        let y0 = ((p.y - p.ry).floor() as i32).max(0);
        let y1 = ((p.y + p.ry).ceil() as i32).min(h);
        for y in y0..y1 {
            let cy = y as f32 + 0.5;
            if cy > form.base {
                break;
            }
            let ny = (cy - p.y) / p.ry;
            for x in x0..x1 {
                let nx = (x as f32 + 0.5 - p.x) / p.rx;
                let d2 = nx * nx + ny * ny;
                if d2 > 1.0 {
                    continue;
                }
                let nz = (1.0 - d2).sqrt();
                let lambert = nx * light[0] + ny * light[1] + nz * light[2];
                let mut v = 0.5 + 0.5 * lambert;
                // the cloud as a whole lighter at its crown than at its foot
                v += form.rise * (0.4 - cy / h as f32);
                // and in its own shade along the foot
                let up = form.base - cy;
                if up < 2.5 {
                    v -= 0.1 * (2.5 - up);
                }
                v += grain * (BAYER[y as usize & 3][x as usize & 3] as f32 / 16.0 - 0.47);
                px[(y * w + x) as usize] = tone(v);
            }
        }
    }
    px
}

fn tone(v: f32) -> u8 {
    if v < 0.3 {
        1
    } else if v < 0.46 {
        2
    } else if v < 0.62 {
        3
    } else if v < 0.79 {
        4
    } else {
        5
    }
}

/// The light, from a sun or a moon standing `dx` to the right of the cloud
/// (in widths of the sky) and `up` above the horizon (0 on it, 1 overhead):
/// the low sun lights the clouds from the side, the high one from above.
pub fn light_from(dx: f32, up: f32) -> [f32; 3] {
    let up = up.clamp(0.0, 1.0);
    let v = [dx.clamp(-1.0, 1.0) * (1.1 - 0.6 * up), -(0.15 + 0.75 * up), 0.42];
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cloud_is_lit_on_the_side_the_light_comes_from() {
        let mut rng = Rng(5);
        let form = Form::cumulus(&mut rng, 60, 30);
        let mean = |px: &[u8], left: bool| {
            let tones: Vec<u32> = (0..30)
                .flat_map(|y| (0..60).map(move |x| (x, y)))
                .filter(|&(x, _)| (x < 30) == left)
                .map(|(x, y)| px[(y * 60 + x) as usize] as u32)
                .filter(|&t| t > 0)
                .collect();
            tones.iter().sum::<u32>() as f32 / tones.len() as f32
        };
        let from_left = shade(&form, light_from(-0.8, 0.3), 0.0);
        let from_right = shade(&form, light_from(0.8, 0.3), 0.0);
        assert!(mean(&from_left, true) > mean(&from_left, false));
        assert!(mean(&from_right, false) > mean(&from_right, true));
        // every tone is used, and nothing is drawn under the foot
        for t in 1..=5 {
            assert!(from_left.contains(&t), "no tone {t}");
        }
        assert!(from_left[29 * 60..].iter().all(|&t| t <= 2));
    }

    #[test]
    fn every_form_stays_in_its_box() {
        let mut rng = Rng(9);
        for (form, name) in [
            (Form::cumulus(&mut rng, 70, 34), "cumulus"),
            (Form::layer(&mut rng, 90, 18), "layer"),
            (Form::crown(&mut rng, 40, 32), "crown"),
        ] {
            let px = shade(&form, light_from(0.0, 1.0), 0.05);
            assert_eq!(px.len(), (form.w * form.h) as usize, "{name}");
            let filled = px.iter().filter(|&&t| t > 0).count();
            assert!(filled > px.len() / 4, "{name} is mostly empty ({filled} of {})", px.len());
        }
    }
}
