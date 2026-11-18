//! The figures a ruler is numbered with: the ten digits, and the A and P of
//! the 12-hour clock, three pixels wide and five tall, a pixel apart.  They
//! are set pixel by pixel, because no outline font comes out clean that
//! small: DirectWrite's aliased glyphs lose and thicken strokes below 8pt.

use crate::canvas::Canvas;

/// Rows of a figure.
pub const HEIGHT: i32 = 5;
const WIDTH: i32 = 3;
/// From one figure's left edge to the next.
const ADVANCE: i32 = WIDTH + 1;

/// A figure's five rows, top first; in each, the leftmost pixel is the
/// highest of the three bits.
fn glyph(c: char) -> Option<[u8; HEIGHT as usize]> {
    Some(match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b011, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        _ => return None,
    })
}

/// How wide `s` is set, in px.  A character with no figure takes no room.
pub fn width(s: &str) -> i32 {
    let n = s.chars().filter(|&c| glyph(c).is_some()).count() as i32;
    (n * ADVANCE - 1).max(0)
}

/// `s` with its middle on column `x` and its top row at `y`.  Every width is
/// odd, so the middle is a column, not between two.
pub fn draw_centred(canvas: &mut Canvas, s: &str, x: i32, y: i32, colour: u32) {
    let mut left = x - width(s) / 2;
    for rows in s.chars().filter_map(glyph) {
        for (dy, bits) in (0..).zip(rows) {
            for dx in 0..WIDTH {
                if bits >> (WIDTH - 1 - dx) & 1 == 1 {
                    canvas.fill(left + dx, y + dy, 1, 1, colour);
                }
            }
        }
        left += ADVANCE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `s` drawn centred on a canvas `w` px wide, as rows of '#' and '.'.
    fn drawn(s: &str, w: i32) -> Vec<String> {
        let mut px = vec![0u32; (w * HEIGHT) as usize];
        // SAFETY: the canvas is exactly the vector.
        let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), w, w, HEIGHT) };
        draw_centred(&mut canvas, s, w / 2, 0, 0xFF00_0000);
        px.chunks(w as usize).map(|row| row.iter().map(|&p| if p == 0 { '.' } else { '#' }).collect()).collect()
    }

    #[test]
    fn figures_are_three_by_five_a_pixel_apart() {
        assert_eq!(drawn("0123456789AP", 47), [
            "###..#..###.###.#.#.###.###.###.###.###..#..##.",
            "#.#.##....#...#.#.#.#...#.....#.#.#.#.#.#.#.#.#",
            "#.#..#..###..##.###.###.###...#.###.###.###.##.",
            "#.#..#..#.....#...#...#.#.#...#.#.#...#.#.#.#..",
            "###.###.###.###...#.###.###...#.###.###.#.#.#..",
        ]);
    }

    #[test]
    fn a_figure_is_centred_on_its_column() {
        assert_eq!((width(""), width("9"), width("07"), width("12P")), (0, 3, 7, 11));
        // nothing it has no figure for, and no room kept for it
        assert_eq!(width("1 2:"), width("12"));
        assert_eq!(drawn("07", 9), [".###.###.", ".#.#...#.", ".#.#...#.", ".#.#...#.", ".###...#."]);
    }
}
