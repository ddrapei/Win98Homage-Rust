//! The cities: a place for each that it could be nowhere else.
//!
//!   London     Westminster from the river: the Elizabeth Tower -- Big Ben --
//!              its clock telling London's time and its belfry lit green
//!              after dark; the Palace of Westminster in honey-coloured
//!              stone, floodlit gold at night; Westminster Bridge in its
//!              green, buses and cabs going over; the Thames; the London Eye
//!              turning beside County Hall; and the towers beyond, the Shard
//!              among them.
//!   Aberdeen   by day, Marischal College from across Broad Street: the
//!              granite front, bay after bay of tall windows between
//!              buttresses that run up into pinnacles, the gateway, the
//!              Mitchell Tower with the Saltire flying, the granite glinting
//!              in the sun, Robert the Bruce on his horse before it, and the
//!              buses going by.  After dark, the harbour: supply ships lit at
//!              their berths, cranes with their red lights, a tug crossing,
//!              and above the roofs the floodlit spires of Marischal College,
//!              the Town House and St Nicholas.
//!
//! Each takes the weather: snow lies on its ledges and roofs, and its
//! streets shine in the rain.

use super::super::sprites::Ride;
use super::{leaves, Brush, Wx, SALTIRE, UNION};


/// Westminster.
pub(super) fn london(b: &mut Brush, wx: Wx) {
    let (w, h) = (b.l.w, b.l.h);
    let (bank, river, near) = (136, 142, 170);
    let warm = b.c("WindowLit");
    let snow = b.c("Snow");
    let red = b.c("PaintRed");

    // ---- the city beyond the river: towers of glass, and the Shard
    b.haze = 130;
    let glass = b.cs(["SlateDark", "Slate", "Glass"]);
    for y in 42..bank {
        let f = (y - 42) as f32 / (bank - 42) as f32;
        let half = (1.0 + 10.0 * f) as i32;
        for dx in -half..=half {
            let c = if dx < -half / 3 { glass[1] } else if dx > half / 2 { glass[0] } else { glass[2] };
            b.set(176 + dx, y, c);
        }
        if y % 3 == 0 && b.chance(0.5) {
            let dx = b.int(-half, half + 1);
            b.light(176 + dx, y, warm);
        }
    }
    b.l.blinkers.push((176, 41, red));
    for &(x, top, tw) in &[(150, 80, 12), (163, 94, 9), (190, 72, 11), (203, 98, 14), (230, 84, 16), (118, 98, 10), (100, 106, 12)] {
        for y in top..bank {
            for xx in x..x + tw {
                let c = if xx == x { glass[1] } else if (xx - x) % 4 == 3 { glass[0] } else { glass[2] };
                b.set(xx, y, c);
            }
        }
        for y in (top + 2..bank - 2).step_by(3) {
            for xx in (x + 1..x + tw - 1).step_by(2) {
                if b.chance(0.3) {
                    b.light(xx, y, warm);
                }
            }
        }
        if top < 85 {
            b.l.blinkers.push((x + tw / 2, top - 1, red));
        }
    }

    // ---- County Hall on the south bank, and the plane trees before it
    b.haze = 45;
    let stone = b.cs(["StoneDark", "Stone", "StoneLight"]);
    let slate = b.cs(["Slate", "SlateDark"]);
    for y in 112..bank {
        for x in 156..w {
            let c = if y == 112 && wx.snow {
                snow
            } else if y < 117 {
                slate[((x + y) % 2) as usize]
            } else if (x - 156) % 6 == 0 {
                stone[2]
            } else if y == 126 {
                stone[0]
            } else {
                stone[1]
            };
            b.flood(x, y, c);
        }
    }
    for &y in &[119, 123, 128, 132] {
        for x in (158..w).step_by(3) {
            if b.chance(0.5) {
                b.light(x, y, warm);
                b.light(x, y + 1, warm);
            } else {
                b.set(x, y, b.c("Glass"));
                b.set(x, y + 1, b.c("Glass"));
            }
        }
    }
    let crown = leaves(b);
    for x in (162..w + 6).step_by(15) {
        b.tree(x, bank + 1, 13, 10, 2, crown);
    }

    // ---- the London Eye's legs (the wheel turns, and is drawn as it goes)
    b.haze = 20;
    let steel = b.c("Trim");
    let (ex, ey, er) = (214, 96, 32);
    for k in 0..2 {
        b.line(ex as f32 + k as f32, ey as f32, 199.0 + k as f32, bank as f32 + 2.0, steel);
    }
    b.line(ex as f32, ey as f32, 208.0, bank as f32 + 2.0, steel);
    b.l.wheel = Some((ex, ey, er));

    // ---- the Palace of Westminster: bays of gothic windows between
    // buttresses, string courses, a steep roof bristling with pinnacles,
    // its towers -- honey-coloured stone, floodlit after dark
    b.haze = 0;
    let roof = 110;
    let gold = b.cs(["Gold", "GoldDark"]);
    for x in 0..134 {
        for y in roof..bank {
            let c = match x % 6 {
                0 => stone[2],
                5 => stone[0],
                _ if matches!(y, 113 | 121 | 129) => stone[2],
                _ => stone[1],
            };
            b.flood(x, y, c);
        }
        for y in roof - 5..roof {
            b.set(x, y, if y == roof - 5 && wx.snow { snow } else { slate[((y + x) % 2) as usize] });
        }
    }
    for bay in (0..134).step_by(6) {
        for &(y0, y1) in &[(115, 120), (123, 128), (131, 134)] {
            let lit = b.chance(0.45);
            for x in bay + 2..bay + 4 {
                for y in y0..y1 {
                    if lit { b.light(x, y, warm) } else { b.set(x, y, b.c("Glass")) }
                }
            }
        }
        // a pinnacle on every buttress, taller on every other
        let tall = if bay % 12 == 0 { 12 } else { 8 };
        for y in roof - 5 - tall..roof - 5 {
            b.flood(bay, y, stone[2]);
        }
        b.flood(bay + 1, roof - 6, stone[0]);
        b.set(bay, roof - 6 - tall, if wx.snow { snow } else { gold[0] });
    }
    // its turrets, and the Central Tower's lantern and spire
    for &tx in &[28, 58, 96] {
        for y in 90..roof {
            for x in tx..tx + 6 {
                b.flood(x, y, if x == tx { stone[2] } else if x == tx + 5 { stone[0] } else { stone[1] });
            }
        }
        b.set(tx + 2, 96, b.c("Glass"));
        b.set(tx + 3, 96, b.c("Glass"));
        for r in 0..5 {
            for x in tx + r / 2..tx + 6 - r / 2 {
                b.set(x, 89 - r, slate[(x > tx + 2) as usize]);
            }
        }
        b.flood(tx, 86, stone[2]);
        b.flood(tx + 5, 86, stone[2]);
    }
    for y in 88..roof {
        for x in 64..73 {
            let window = (y - 90) % 5 < 3 && (x == 66 || x == 70);
            b.flood(x, y, if window { stone[0] } else if x == 64 { stone[2] } else { stone[1] });
        }
    }
    for r in 0..16 {
        let half = r / 4;
        for x in 68 - half..=68 + half {
            b.set(x, 72 + r, slate[(x > 68) as usize]);
        }
    }
    b.set(68, 71, gold[0]);
    // the Victoria Tower, at the far end, with the flag over it
    for y in 70..bank {
        for x in 2..20 {
            let c = match (x - 2) % 6 {
                0 => stone[2],
                5 => stone[0],
                _ if y % 9 == 0 => stone[2],
                _ => stone[1],
            };
            b.flood(x, y, c);
        }
        if (y - 74) % 9 < 5 && y < 128 {
            for x in [5, 6, 11, 12, 17] {
                b.set(x, y, b.c("Glass"));
            }
        }
    }
    for &x in &[2, 10, 19] {
        for y in 62..70 {
            b.flood(x, y, stone[2]);
        }
        b.set(x, 61, gold[0]);
    }
    for y in 54..70 {
        b.set(11, y, b.c("Pole"));
    }
    b.l.flag = Some((11, 54, UNION));

    // ---- the Elizabeth Tower: Big Ben
    let cx = 144;
    for y in 60..bank {
        for x in 138..151 {
            let c = match x {
                138 => stone[2],
                150 => stone[0],
                _ if (x - 138) % 3 == 0 => stone[0],
                _ if y % 8 == 0 => stone[2],
                _ => stone[1],
            };
            b.flood(x, y, c);
        }
        if (y - 66) % 10 < 4 && y < 130 {
            b.light(cx, y, warm);
            b.light(cx + 1, y, warm);
        }
    }
    // the clock stage, and the face -- lit after dark; the hands are the
    // time's
    for y in 44..60 {
        for x in 137..152 {
            b.flood(x, y, if x == 137 || y == 44 { stone[2] } else if x == 151 { stone[0] } else { stone[1] });
        }
    }
    let face = b.c("ClockFace");
    b.oval(cx as f32 + 0.5, 52.5, 6.0, 6.0, gold[1]);
    for y in 47..58 {
        for x in 139..150 {
            let (dx, dy) = (x as f32 + 0.5 - (cx as f32 + 0.5), y as f32 + 0.5 - 52.5);
            if dx * dx + dy * dy <= 25.0 {
                b.flood(x, y, face);
            }
        }
    }
    b.l.clock = Some((cx, 52, 5));
    for &x in &[137, 151] {
        for y in 39..44 {
            b.flood(x, y, stone[2]);
        }
        b.set(x, 38, gold[0]);
    }
    // the belfry, lit green after dark, and the gilded band over it
    let belfry = b.c("BelfryLight");
    for y in 34..44 {
        for x in 138..151 {
            let arch = (x - 139) % 3 != 2 && (36..42).contains(&y);
            if arch { b.light(x, y, belfry) } else { b.flood(x, y, if x == 138 { stone[2] } else { stone[1] }) }
        }
    }
    for x in 137..152 {
        b.set(x, 33, gold[(x > cx) as usize]);
    }
    // the spire, slate with gold at its corners, and the finial
    for y in 14..33 {
        let half = 1 + (y - 14) * 6 / 19;
        for x in cx - half..=cx + half {
            let c = if wx.snow && (y - 14) % 4 == 0 && x <= cx { snow } else { slate[(x > cx) as usize] };
            b.set(x, y, c);
        }
    }
    for &x in &[139, 149] {
        for y in 28..33 {
            b.set(x, y, gold[0]);
        }
    }
    for y in 8..14 {
        b.set(cx, y, gold[0]);
    }
    b.set(cx - 1, 10, gold[1]);
    b.set(cx + 1, 10, gold[1]);

    // ---- the embankment wall, granite, and its lamps
    let granite = b.cs(["GraniteDark", "Granite", "GraniteLight"]);
    for y in bank..river {
        for x in 0..w {
            let joint = (y - bank) % 3 == 2 || (x + (y - bank) / 3 * 4) % 9 == 0;
            b.set(x, y, if y == bank && wx.snow { snow } else if joint { granite[0] } else { granite[1] });
        }
    }
    let lamp = b.c("LampLit");
    let iron = b.c("Pole");
    for x in (12..w).step_by(26) {
        if (134..154).contains(&x) {
            continue;
        }
        for y in bank - 8..bank {
            b.set(x, y, iron);
        }
        b.light(x, bank - 9, lamp);
        b.light(x - 1, bank - 9, lamp);
        b.light(x + 1, bank - 9, lamp);
        b.l.lamps.push((x, bank - 9));
    }

    // ---- the Thames: behind the bridge it mirrors the Palace, in front
    // of it the bridge
    let (water, deep) = (b.c("Water"), b.c("WaterDeep"));
    for y in river..h {
        for x in 0..w {
            let f = (y - river) as f32 / (h - river) as f32;
            let depth = if y < near { y - river } else { y - near };
            b.wet(x, y, super::mix(water, deep, f), depth);
        }
    }

    // ---- Westminster Bridge: the green parapet with its lamps, the deck,
    // and green iron arches on granite piers
    let green = b.cs(["BridgeGreen", "BridgeGreenDark"]);
    let deck = 148;
    let piers = [6, 40, 74, 108, 142];
    for x in 0..146 {
        for y in deck - 3..deck + 2 {
            let c = if y == deck - 3 { if wx.snow { snow } else { stone[2] } } else if y == deck + 1 { green[1] } else if x % 3 == 0 { green[1] } else { green[0] };
            b.set(x, y, c);
        }
    }
    // each arch: its opening rising from the water at the piers to its
    // crown under the deck, the spandrel over it green iron, its rib dark
    for pair in piers.windows(2) {
        let (a, z) = (pair[0] + 6, pair[1]);
        let (mid, half) = ((a + z) as f32 / 2.0, (z - a) as f32 / 2.0);
        for x in a..z {
            let t = (x as f32 + 0.5 - mid) / half;
            let opening = near - ((near - deck - 4) as f32 * (1.0 - t * t).max(0.0).sqrt()).round() as i32;
            for y in deck + 2..opening {
                b.set(x, y, if (x * 3 + y) % 7 == 0 { green[1] } else { green[0] });
            }
            b.set(x, opening, green[1]);
        }
    }
    for &p in &piers {
        for y in deck + 2..near + 2 {
            for x in p..p + 6 {
                b.set(x, y, if x == p { granite[2] } else if x == p + 5 { granite[0] } else { granite[1] });
            }
        }
        // a cutwater, pointed, at the water
        b.set(p + 2, near + 2, granite[1]);
        b.set(p + 3, near + 2, granite[0]);
    }
    for x in (10..146).step_by(17) {
        for y in deck - 10..deck - 3 {
            b.set(x, y, green[1]);
        }
        for dx in -1..=1 {
            b.light(x + dx, deck - 11, lamp);
        }
        b.set(x, deck - 12, green[1]);
        b.l.lamps.push((x, deck - 11));
    }
    // the traffic over it, and a riverboat on the water nearer us
    b.lane(Ride::Bus, (deck - 2) as f32, 0.0, -20.0, 150.0, 15.0, true, 2);
    b.lane(Ride::Cab, (deck - 2) as f32, 0.0, -14.0, 150.0, 22.0, true, 2);
    b.lane(Ride::Boat, h as f32 - 6.0, 0.0, -30.0, w as f32 + 30.0, 7.0, true, 1);
    b.l.ground = (river, h - 1);
    b.l.fog = vec![118.0, 140.0, 162.0];
    let _ = wx.wet;
}

/// Aberdeen by day: Marischal College across Broad Street.
pub(super) fn aberdeen(b: &mut Brush, wx: Wx) {
    let (w, h) = (b.l.w, b.l.h);
    let foot = 146;
    let g = b.cs(["GraniteDark", "Granite", "GraniteLight"]);
    let glass = b.c("Glass");
    let snow = b.c("Snow");

    // ---- the Mitchell Tower, rising behind the middle of the front, the
    // Saltire flying over it
    b.haze = 25;
    let (tx, tw) = (156, 18);
    for y in 34..92 {
        for x in tx..tx + tw {
            let c = match x - tx {
                0 | 1 => g[2],
                k if k >= tw - 2 => g[0],
                _ if y % 11 == 0 => g[2],
                _ => g[1],
            };
            b.set(x, y, c);
        }
        if (40..54).contains(&y) || (60..74).contains(&y) {
            for &k in &[5, 6, 11, 12] {
                b.set(tx + k, y, glass);
            }
        }
    }
    for x in tx - 1..tx + tw + 1 {
        b.set(x, 33, g[2]);
        if x % 3 != 0 {
            b.set(x, 32, if wx.snow { snow } else { g[1] });
        }
    }
    for &(k, top) in &[(0, 16), (tw - 1, 16), (5, 22), (12, 22)] {
        for y in top..32 {
            b.set(tx + k, y, if (y - top) % 3 == 0 && y > top + 2 { g[0] } else { g[2] });
        }
        b.set(tx + k, top - 1, if wx.snow { snow } else { g[2] });
    }
    for y in 10..32 {
        b.set(tx + 9, y, b.c("Pole"));
    }
    b.l.flag = Some((tx + 9, 10, SALTIRE));

    // ---- the front: bay after bay of tall windows between buttresses,
    // each buttress running up past the battlements into a pinnacle
    b.haze = 0;
    let top = 88;
    for x in 0..w {
        for y in top..foot {
            let c = match x.rem_euclid(10) {
                0 => g[2],
                9 => g[0],
                _ if matches!(y, 100 | 116 | 132) => g[2],
                _ if matches!(y, 101 | 117 | 133) => g[0],
                _ => g[1],
            };
            b.set(x, y, c);
        }
        // the battlements
        b.set(x, top - 1, g[2]);
        if x % 4 < 2 {
            b.set(x, top - 2, g[1]);
            b.set(x, top - 3, if wx.snow { snow } else { g[2] });
        }
    }
    for bay in (0..w).step_by(10) {
        for &(y0, y1) in &[(103, 114), (119, 130), (135, 144)] {
            for x in bay + 3..bay + 8 {
                for y in y0..y1 {
                    let point = y == y0 && (x == bay + 3 || x == bay + 7);
                    if point {
                        continue;
                    }
                    let c = if x == bay + 5 || y == (y0 + y1) / 2 { g[1] } else { glass };
                    b.set(x, y, c);
                }
            }
        }
        // the buttress's pinnacle: a shaft three wide with a gablet over
        // its niche, then the spire, crocketed every other row, to a finial
        // -- and between buttresses a smaller one, so the skyline bristles
        for y in top - 11..top - 1 {
            b.set(bay - 1, y, g[2]);
            b.set(bay, y, g[1]);
            b.set(bay + 1, y, g[0]);
        }
        b.set(bay, top - 7, glass);
        b.set(bay, top - 6, glass);
        b.set(bay - 2, top - 11, g[2]);
        b.set(bay + 2, top - 11, g[0]);
        for y in top - 26..top - 11 {
            let thick = if y > top - 19 { 2 } else { 1 };
            b.set(bay, y, g[2]);
            if thick == 2 {
                b.set(bay + 1, y, g[0]);
            }
            if (y - top) % 2 == 0 {
                b.set(bay - 1, y, g[2]);
                b.set(bay + thick, y, g[0]);
            }
        }
        b.set(bay, top - 27, if wx.snow { snow } else { g[2] });
        for y in top - 10..top - 3 {
            b.set(bay + 5, y, if y % 2 == 0 { g[2] } else { g[1] });
        }
        b.set(bay + 5, top - 11, if wx.snow { snow } else { g[2] });
    }
    // ---- the gateway: its tower and turrets, the arch, the arms over it
    let (gx, gw) = (104, 30);
    for y in 62..foot {
        for x in gx..gx + gw {
            let c = if x == gx { g[2] } else if x == gx + gw - 1 { g[0] } else if y % 12 == 0 { g[2] } else { g[1] };
            b.set(x, y, c);
        }
    }
    for x in gx..gx + gw {
        if x % 4 < 2 {
            b.set(x, 60, if wx.snow { snow } else { g[2] });
            b.set(x, 61, g[1]);
        }
    }
    for &t in &[gx - 2, gx + gw - 3] {
        for y in 52..foot {
            for x in t..t + 5 {
                b.set(x, y, if x == t { g[2] } else if x == t + 4 { g[0] } else { g[1] });
            }
        }
        for y in 38..52 {
            b.set(t + 2, y, if (y - 38) % 3 == 0 { g[0] } else { g[2] });
        }
        b.set(t + 2, 37, if wx.snow { snow } else { g[2] });
    }
    // the oriel over the arch: a tall window of many lights
    for y in 70..96 {
        for x in gx + 9..gx + gw - 9 {
            let c = if (x - gx) % 3 == 0 || (y - 70) % 8 == 0 { g[1] } else { glass };
            b.set(x, y, c);
        }
    }
    // the arms, and the pointed arch of the gate
    let arms = b.cs(["Gold", "PaintRed", "PaintBlue"]);
    for y in 100..110 {
        for x in gx + 12..gx + gw - 12 {
            let c = if x == gx + 12 || y == 100 { arms[0] } else if (x + y) % 2 == 0 { arms[1] } else { arms[2] };
            b.set(x, y, c);
        }
    }
    let door = b.cs(["WoodDark", "Wood"]);
    for y in 116..foot {
        let narrow = (6 - (y - 116)).max(0) / 2;
        for x in gx + 8 + narrow..gx + gw - 8 - narrow {
            b.set(x, y, if (x - gx) % 4 == 0 { door[1] } else { door[0] });
        }
    }
    // mica in the granite, which glints when the sun is out
    for _ in 0..110 {
        let (x, y) = (b.int(0, w), b.int(30, foot));
        let c = b.get(x, y);
        if g.contains(&c) {
            b.l.glints.push((x, y));
        }
    }

    // ---- Robert the Bruce on horseback, on his plinth, before the gate
    for y in 128..foot {
        for x in 52..74 {
            b.set(x, y, if y == 128 { g[2] } else if y == 136 { g[1] } else if x == 73 { g[0] } else { g[0] });
        }
    }
    let bronze = b.cs(["Bronze", "BronzeLight"]);
    const BRUCE: [&str; 20] = [
        "..........l...........",
        "..........l...........",
        ".........l............",
        ".........l.b..........",
        "........lbbb..........",
        "........bbb...........",
        ".......bbbb...........",
        ".......bbbbb.....ll...",
        "......lbbbbb....lbbb..",
        "...lllbbbbbbllllbbbbb.",
        "..lbbbbbbbbbbbbbbbb.bb",
        ".lbbbbbbbbbbbbbbbb....",
        "lb.bbbbbbbbbbbbbbb....",
        "b..bbbbbbbbbbbbbb.....",
        "...bb..bb...bb..bb....",
        "...b...b....b....b....",
        "...b....b...b....b....",
        "..b.....b..b.....b....",
        "..b.....b..b.....b....",
        ".bb....bb.bb....bb....",
    ];
    for (r, row) in BRUCE.iter().enumerate() {
        for (k, c) in row.bytes().enumerate() {
            let colour = match c {
                b'b' => bronze[0],
                b'l' => bronze[1],
                _ => continue,
            };
            b.set(52 + k as i32, 108 + r as i32, colour);
        }
    }
    if wx.snow {
        for x in 55..72 {
            b.set(x, 117, snow);
        }
    }

    // ---- Broad Street: the pavement, the kerb, double yellow lines, the
    // road -- shining in the rain, slushy in snow
    for y in foot..foot + 7 {
        for x in 0..w {
            let joint = (y - foot) % 4 == 3 || (x + (y - foot) / 4 * 3) % 8 == 0;
            b.set(x, y, if wx.snow { if joint { b.c("SnowShade") } else { snow } } else if joint { g[1] } else { g[2] });
        }
    }
    for x in 0..w {
        b.set(x, foot + 7, g[0]);
    }
    let road = foot + 8;
    let asphalt = b.c("Asphalt");
    for y in road..h {
        for x in 0..w {
            if wx.snow || wx.slush {
                // snow, or slush, with the traffic's tracks through it
                let track = matches!((y - road) % 13, 3 | 4 | 9 | 10);
                let lies = if wx.snow { b.c("SnowShade") } else { b.c("EarthLight") };
                b.set(x, y, if track { b.c("AsphaltLight") } else { lies });
            } else if wx.wet {
                b.wet(x, y, asphalt, y - road);
            } else {
                // grit in the asphalt
                let speckle = ((x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77)) >> 27 == 0;
                b.set(x, y, if speckle { b.c("AsphaltLight") } else { asphalt });
            }
        }
    }
    if !wx.snow && !wx.slush {
        let yellow = b.c("PaintYellow");
        for x in 0..w {
            b.set(x, road + 1, yellow);
            b.set(x, road + 3, yellow);
        }
    }
    // the lamps along it
    let lamp = b.c("LampLit");
    let iron = b.c("Pole");
    for &x in &[28, 92, 176, 230] {
        for y in foot - 32..foot + 6 {
            b.set(x, y, iron);
        }
        b.rect(x - 1, foot + 4, 3, 2, iron);
        for y in foot - 36..foot - 32 {
            for dx in -1..=1 {
                b.light(x + dx, y, lamp);
            }
        }
        b.rect(x - 2, foot - 37, 5, 1, iron);
        b.l.lamps.push((x, foot - 34));
    }
    // people along the pavement, the buses and cars on the road
    b.lane(Ride::Walker, (foot + 6) as f32, 0.0, -6.0, w as f32 + 6.0, 9.0, true, 6);
    b.lane(Ride::Car, (road + 10) as f32, 0.0, -14.0, w as f32 + 14.0, -24.0, false, 2);
    b.lane(Ride::Coach, (h - 3) as f32, 0.0, -20.0, w as f32 + 20.0, 18.0, false, 1);
    b.lane(Ride::Car, (h - 3) as f32, 0.0, -14.0, w as f32 + 14.0, 26.0, false, 1);
    b.l.ground = (foot, h - 1);
    b.l.gulls = true;
}

/// Aberdeen after dark: the harbour.
pub(super) fn aberdeen_harbour(b: &mut Brush, wx: Wx) {
    let (w, h) = (b.l.w, b.l.h);
    let (quay, water) = (126, 138);
    let g = b.cs(["GraniteDark", "Granite", "GraniteLight"]);
    let slate = b.cs(["Slate", "SlateDark"]);
    let warm = b.c("WindowLit");
    let snow = b.c("Snow");
    let red = b.c("PaintRed");

    // ---- the city on its hill: roofs and lit windows, and the spires
    // floodlit -- Marischal College, the Town House, St Nicholas
    b.haze = 70;
    let mut x = -4;
    while x < w + 4 {
        let bw = b.int(8, 18);
        let top = b.int(94, 106);
        for xx in x..x + bw {
            let gable = top + ((xx - x - bw / 2).abs()) / 2;
            for y in gable..quay {
                b.set(xx, y, if y < top + 5 { if wx.snow && y == gable { snow } else { slate[0] } } else { g[0] });
            }
        }
        for _ in 0..b.int(1, 5) {
            let (wx_, wy) = (b.int(x + 1, x + bw - 1), b.int(top + 6, quay - 2));
            b.light(wx_, wy, warm);
        }
        b.set(x + bw / 2, top - 2, g[0]);
        b.set(x + bw / 2, top - 1, g[0]);
        x += bw;
    }
    // Marischal College, its pinnacles, and the Mitchell Tower -- nearer
    // than the roofs about them, floodlit, a few windows lit late; clear
    // of the readings, which take the top left
    b.haze = 35;
    let glass = b.c("Glass");
    let (mx0, mx1) = (100, 196);
    for y in 85..112 {
        for x in mx0..mx1 {
            let c = match (x - mx0) % 6 {
                0 => g[2],
                5 => g[0],
                _ => g[1],
            };
            b.flood(x, y, c);
        }
    }
    // lancets, two to a bay, on three floors
    for bay in (mx0..mx1).step_by(6) {
        for &(y0, y1) in &[(88, 94), (97, 103), (106, 111)] {
            for x in [bay + 2, bay + 3] {
                for y in y0..y1 {
                    if y == y0 && x == bay + 3 {
                        continue;
                    }
                    b.set(x, y, glass);
                }
                if b.chance(0.12) {
                    for y in y0 + 1..y1 {
                        b.light(x, y, warm);
                    }
                }
            }
        }
    }
    // its battlements, and the pinnacles bristling along them
    for x in mx0..mx1 {
        b.flood(x, 84, g[2]);
        if x % 3 == 0 {
            b.flood(x, 83, g[1]);
        }
    }
    for x in (mx0..mx1 + 1).step_by(6) {
        for y in 78..84 {
            b.flood(x, y, g[2]);
            b.flood(x + 1, y, g[0]);
        }
        for y in 70..78 {
            b.flood(x, y, g[2]);
            if y % 2 == 0 && y < 76 {
                b.flood(x - 1, y, g[1]);
                b.flood(x + 1, y, g[0]);
            }
        }
        b.set(x, 69, if wx.snow { snow } else { g[2] });
        if x + 3 < mx1 {
            b.flood(x + 3, 80, g[2]);
            b.flood(x + 3, 81, g[2]);
            b.flood(x + 3, 82, g[1]);
        }
    }
    // the Mitchell Tower: corner turrets and their pinnacles, a tall
    // window up its face, lit
    let (tx0, tx1) = (142, 152);
    for y in 48..84 {
        for x in tx0..tx1 {
            let c = if x == tx0 || x == tx0 + 1 { g[2] } else if x >= tx1 - 2 { g[0] } else { g[1] };
            b.flood(x, y, c);
        }
    }
    for y in 54..76 {
        b.set(tx0 + 4, y, glass);
        b.set(tx0 + 5, y, glass);
    }
    for y in 56..62 {
        b.light(tx0 + 4, y, warm);
        b.light(tx0 + 5, y, warm);
    }
    for x in tx0..tx1 {
        if x % 2 == 0 {
            b.flood(x, 47, g[1]);
        }
    }
    for &x in &[tx0, tx1 - 1] {
        for y in 36..48 {
            b.flood(x, y, g[2]);
            if y % 2 == 0 && y < 44 {
                b.flood(x + 1, y, g[0]);
            }
        }
        b.set(x, 35, if wx.snow { snow } else { g[2] });
    }
    // the Town House steeple, its clock lit
    let (sx, sc) = (200, 204);
    for y in 66..quay {
        for x in sx..sx + 8 {
            b.flood(x, y, if x == sx { g[2] } else if x == sx + 7 { g[0] } else { g[1] });
        }
    }
    for r in 0..24 {
        let half = (r / 5).min(3);
        for x in sc - half..=sc - 1 + half {
            b.flood(x, 42 + r, if x >= sc { g[0] } else { g[2] });
        }
    }
    let face = b.c("ClockFace");
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1), (-1, 0), (2, 0), (0, -1), (1, -1), (0, 2), (1, 2)] {
        b.light(sc - 1 + dx, 72 + dy, face);
    }
    // St Nicholas, its tower and spire
    let (nx, nc) = (224, 227);
    for y in 78..quay {
        for x in nx..nx + 7 {
            b.flood(x, y, if x == nx { g[2] } else if x == nx + 6 { g[0] } else { g[1] });
        }
    }
    for r in 0..26 {
        let half = (r / 6).min(3);
        for x in nc - half..=nc + half {
            b.flood(x, 52 + r, if x > nc { g[0] } else { g[2] });
        }
    }

    // ---- the quay: sheds, the cranes with their red lights, the lamps
    b.haze = 25;
    for y in 112..quay {
        for x in 0..w {
            let shed = (x / 26) % 2 == 0;
            let c = if y < 115 { slate[(shed) as usize] } else if (x % 26) == 0 { g[0] } else if shed { g[1] } else { g[0] };
            b.set(x, y, c);
        }
    }
    for x in (4..w).step_by(7) {
        if b.chance(0.5) {
            b.light(x, 119, warm);
            b.light(x + 1, 119, warm);
        }
    }
    let crane = b.cs(["PaintOrange", "BarnDark"]);
    for &cx in &[30, 72] {
        let top = 78;
        for y in top..quay {
            b.set(cx - 2, y, crane[1]);
            b.set(cx + 2, y, crane[1]);
            if (y - top) % 4 == 0 {
                b.line((cx - 2) as f32, y as f32, (cx + 2) as f32, (y + 4) as f32, crane[0]);
            }
        }
        b.rect(cx - 3, top - 4, 7, 4, crane[0]);
        b.light(cx + 1, top - 3, warm);
        for x in cx - 26..cx + 10 {
            b.set(x, top - 5, crane[0]);
            if x % 3 == 0 {
                b.set(x, top - 6, crane[1]);
            }
        }
        b.rect(cx + 6, top - 4, 4, 3, b.c("PaintBlack"));
        b.l.blinkers.push((cx - 26, top - 6, red));
        b.l.blinkers.push((cx, top - 6, red));
    }
    for y in quay..water {
        for x in 0..w {
            let joint = (y - quay) % 3 == 2 || (x + (y - quay) / 3 * 4) % 9 == 0;
            b.set(x, y, if y == quay && wx.snow { snow } else if joint { g[0] } else { g[1] });
        }
    }
    let lamp = b.c("LampLit");
    for x in (16..w).step_by(34) {
        for y in quay - 12..quay {
            b.set(x, y, b.c("Pole"));
        }
        b.light(x, quay - 13, lamp);
        b.light(x + 1, quay - 13, lamp);
        b.l.lamps.push((x, quay - 13));
    }

    // ---- the harbour water
    b.haze = 0;
    let (sea, deep) = (b.c("Water"), b.c("WaterDeep"));
    for y in water..h {
        for x in 0..w {
            b.wet(x, y, super::mix(sea, deep, (y - water) as f32 / (h - water) as f32), y - water);
        }
    }

    // ---- the supply ships at their berths, lit, their bridges at the bow
    for &(x0, x1, line, hull) in &[(4, 104, 156, "PaintOrange"), (126, 188, 150, "Barn")] {
        let hull = b.c(hull);
        let white = b.c("PaintWhite");
        let dark = b.c("PaintBlack");
        for x in x0..x1 {
            let sheer = if x > x1 - 8 { (x - (x1 - 8)) / 2 } else { 0 };
            for y in line - 11 + sheer..line {
                b.set(x, y, if y == line - 11 + sheer { white } else if y > line - 3 { dark } else { hull });
            }
            // the water under it mirrors it
            for y in line..h {
                b.wet(x, y, super::mix(sea, deep, (y - water) as f32 / (h - water) as f32), y - line);
            }
        }
        let (bx0, bx1) = (x1 - (x1 - x0) / 3, x1 - 8);
        for y in line - 32..line - 11 {
            for x in bx0..bx1 {
                b.set(x, y, white);
            }
        }
        for row in 0..3 {
            let y = line - 29 + row * 6;
            for x in (bx0 + 2..bx1 - 2).step_by(3) {
                b.light(x, y, warm);
                b.light(x + 1, y, warm);
            }
        }
        for y in line - 44..line - 32 {
            b.set((bx0 + bx1) / 2, y, white);
        }
        b.l.blinkers.push(((bx0 + bx1) / 2, line - 45, white));
        // its deck lights, and a crane on the after deck
        for x in (x0 + 4..bx0).step_by(9) {
            b.light(x, line - 12, lamp);
        }
        b.line((x0 + 12) as f32, (line - 11) as f32, (x0 + 26) as f32, (line - 24) as f32, b.c("PaintYellow"));
    }
    // the buoys, and a tug crossing the harbour
    b.l.blinkers.push((232, 164, b.c("PaintGreen")));
    b.l.blinkers.push((112, 170, red));
    b.lane(Ride::Tug, (h - 5) as f32, 0.0, -20.0, w as f32 + 20.0, 6.0, true, 1);
    b.l.ground = (water, h - 1);
}
