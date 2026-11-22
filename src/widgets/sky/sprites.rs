//! What goes by in a place: buses and cabs, people, boats, ducks, a tractor,
//! a cable car -- each a little picture in whole pixels, a row a string, a
//! character a pixel.  Each character stands for one of the skin's colours
//! by day, and some -- windows, headlights -- for a light after dark.  `.`
//! is nothing.  They face right; going left they are drawn the other way
//! round.  Some have two frames, taken in turn as they go: legs walking,
//! wheels turning.

/// What rides a lane.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ride {
    /// A London double-decker, red.
    Bus,
    /// An Aberdeen double-decker, purple.
    Coach,
    /// A black cab.
    Cab,
    Car,
    /// Someone walking -- under an umbrella when it rains.
    Walker,
    /// A river boat, its cabin lit after dark.
    Boat,
    /// A harbour tug.
    Tug,
    Duck,
    Hen,
    Tractor,
    /// A cable car, hanging from its cable.
    Gondola,
    /// Tumbleweed, rolling with the wind.
    Weed,
}

pub struct Sprite {
    /// Its frames, each a row a string.
    pub frames: &'static [&'static [&'static str]],
    /// Each character: its colour by day, and its light after dark, if it
    /// has one.
    pub keys: &'static [(u8, &'static str, &'static str)],
}

impl Ride {
    pub const ALL: [Ride; 12] = [
        Ride::Bus,
        Ride::Coach,
        Ride::Cab,
        Ride::Car,
        Ride::Walker,
        Ride::Boat,
        Ride::Tug,
        Ride::Duck,
        Ride::Hen,
        Ride::Tractor,
        Ride::Gondola,
        Ride::Weed,
    ];

    pub fn index(self) -> usize {
        Ride::ALL.iter().position(|&r| r == self).unwrap_or(0)
    }

    /// How many frames a second it steps through its frames, going at its
    /// own pace.
    pub fn steps(self) -> f32 {
        match self {
            Ride::Walker => 4.0,
            Ride::Hen | Ride::Duck => 2.0,
            Ride::Weed => 6.0,
            _ => 0.0,
        }
    }

    pub fn sprite(self, wet: bool) -> &'static Sprite {
        match self {
            Ride::Bus => &BUS,
            Ride::Coach => &COACH,
            Ride::Cab => &CAB,
            Ride::Car => &CAR,
            Ride::Walker if wet => &BROLLY,
            Ride::Walker => &WALKER,
            Ride::Boat => &BOAT,
            Ride::Tug => &TUG,
            Ride::Duck => &DUCK,
            Ride::Hen => &HEN,
            Ride::Tractor => &TRACTOR,
            Ride::Gondola => &GONDOLA,
            Ride::Weed => &WEED,
        }
    }
}

const BUS_ROWS: &[&str] = &[
    "..RRRRRRRRRRRRRR.",
    ".RRRRRRRRRRRRRRRR",
    ".RGGRGGRGGRGGRGGR",
    ".RRRRRRRRRRRRRRRR",
    ".RGGRGGRGGRGGRRGR",
    ".RRRRRRRRRRRRRRRH",
    ".rrrrrrrrrrrrrrrr",
    "..KK.........KK..",
];

static BUS: Sprite = Sprite {
    frames: &[BUS_ROWS],
    keys: &[
        (b'R', "PaintRed", ""),
        (b'r', "PaintRedDark", ""),
        (b'G', "Glass", "WindowLit"),
        (b'H', "PaintWhite", "LampLit"),
        (b'K', "Wire", ""),
    ],
};

static COACH: Sprite = Sprite {
    frames: &[BUS_ROWS],
    keys: &[
        (b'R', "PaintPurple", ""),
        (b'r', "PaintWhite", ""),
        (b'G', "Glass", "WindowLit"),
        (b'H', "PaintWhite", "LampLit"),
        (b'K', "Wire", ""),
    ],
};

static CAB: Sprite = Sprite {
    frames: &[&[
        "...BBBB...",
        "..BGGBGB..",
        "BBBBBBBBBH",
        "bBBBBBBBBB",
        ".KK....KK.",
    ]],
    keys: &[
        (b'B', "PaintBlack", ""),
        (b'b', "PaintRed", "PaintRed"),
        (b'G', "Glass", "WindowLit"),
        (b'H', "PaintWhite", "LampLit"),
        (b'K', "Wire", ""),
    ],
};

static CAR: Sprite = Sprite {
    frames: &[&[
        "..CCCCC...",
        ".CGGCGGC..",
        "CCCCCCCCCH",
        "cCCCCCCCCC",
        ".KK....KK.",
    ]],
    keys: &[
        (b'C', "PaintBlue", ""),
        (b'c', "PaintRed", "PaintRed"),
        (b'G', "Glass", "WindowLit"),
        (b'H', "PaintWhite", "LampLit"),
        (b'K', "Wire", ""),
    ],
};

static WALKER: Sprite = Sprite {
    frames: &[
        &[".s.", "ccc", ".c.", ".c.", "l.l"],
        &[".s.", "ccc", ".c.", ".c.", ".l."],
    ],
    keys: &[(b's', "Sand", ""), (b'c', "PaintBlue", ""), (b'l', "Wire", "")],
};

static BROLLY: Sprite = Sprite {
    frames: &[
        &[".UUU.", "UUUUU", "..p..", ".cc..", ".cc..", ".l.l."],
        &[".UUU.", "UUUUU", "..p..", ".cc..", ".cc..", "..l.."],
    ],
    keys: &[(b'U', "PaintRed", ""), (b'p', "Wire", ""), (b'c', "PaintBlack", ""), (b'l', "Wire", "")],
};

static BOAT: Sprite = Sprite {
    frames: &[&[
        "........WWWWWWWWW.....",
        ".....WWWGGWGGWGGWW....",
        "...WWWWWWWWWWWWWWWWW..",
        "BBBBBBBBBBBBBBBBBBBBBH",
        ".BBBBBBBBBBBBBBBBBBBB.",
    ]],
    keys: &[
        (b'W', "PaintWhite", ""),
        (b'G', "Glass", "WindowLit"),
        (b'B', "PaintBlue", ""),
        (b'H', "PaintWhite", "LampLit"),
    ],
};

static TUG: Sprite = Sprite {
    frames: &[&[
        "......M.....",
        ".....WWW....",
        "....WGWGW...",
        "OOOOOOOOOOOH",
        ".OOOOOOOOOK.",
    ]],
    keys: &[
        (b'M', "PaintWhite", "LampLit"),
        (b'W', "PaintWhite", ""),
        (b'G', "Glass", "WindowLit"),
        (b'O', "PaintOrange", ""),
        (b'K', "Wire", ""),
        (b'H', "PaintWhite", "LampLit"),
    ],
};

static DUCK: Sprite = Sprite {
    frames: &[&["..g.", "bbbo", ".bb."], &["..g.", "bbbo", "bbb."]],
    keys: &[(b'g', "Leaf", ""), (b'b', "PaintBrown", ""), (b'o', "PaintOrange", "")],
};

static HEN: Sprite = Sprite {
    frames: &[&[".rw.", "www.", ".ww.", ".o.."], &["....", ".rw.", "wwww", ".o.."]],
    keys: &[(b'r', "PaintRed", ""), (b'w', "PaintWhite", ""), (b'o', "PaintOrange", "")],
};

static TRACTOR: Sprite = Sprite {
    frames: &[&[
        "....GG....",
        "....GgG...",
        "..GGGGG...",
        "GGGGGGGGGH",
        "KKK...KK..",
        "KKK...KK..",
    ]],
    keys: &[
        (b'G', "BridgeGreen", ""),
        (b'g', "Glass", "WindowLit"),
        (b'H', "PaintWhite", "LampLit"),
        (b'K', "Wire", ""),
    ],
};

static GONDOLA: Sprite = Sprite {
    frames: &[&["..p..", "..p..", "RRRRR", "RGGGR", "RRRRR"]],
    keys: &[(b'p', "Wire", ""), (b'R', "PaintRed", ""), (b'G', "Glass", "WindowLit")],
};

static WEED: Sprite = Sprite {
    frames: &[&[".tt.", "t.tt", "tt.t", ".tt."], &[".tt.", "tt.t", "t.tt", ".tt."]],
    keys: &[(b't', "WheatDark", "")],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_is_a_rectangle_and_every_character_has_a_colour() {
        for ride in Ride::ALL {
            for wet in [false, true] {
                let s = ride.sprite(wet);
                let w = s.frames[0][0].len();
                for frame in s.frames {
                    assert_eq!(frame.len(), s.frames[0].len(), "{ride:?}: frames of different heights");
                    for row in *frame {
                        assert_eq!(row.len(), w, "{ride:?}: a row of another width");
                        for c in row.bytes().filter(|&c| c != b'.') {
                            assert!(s.keys.iter().any(|k| k.0 == c), "{ride:?}: no colour for {}", c as char);
                        }
                    }
                }
            }
            assert_eq!(Ride::ALL[ride.index()], ride);
        }
    }
}
