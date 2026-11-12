//! The Resource Meter: CPU and physical memory as two segmented Win98
//! progress bars.
//!
//! The bars are drawn backwards, as in the skin: every block is in the .ini,
//! always, and what moves is one field-coloured rectangle per bar that covers
//! the tail the reading has not earned.  The cover moves in whole blocks and
//! the label in whole percents, so a steady load repaints nothing.

use crate::canvas::{Canvas, Rect};
use crate::skin::{self, Hooks, Meter, Skin};
use crate::sys;
use crate::text::Text;
use crate::widget::{Ctx, Dirty, Outcome, Request, Widget};

static SKIN: &Skin = &skin::resources::SKIN;

/// Width of one block, the field between two, and the field kept visible
/// between the run of blocks and the trough's bezel: the static blocks in the
/// .ini are laid out from the same three figures.
const BLOCK: i32 = 8;
const GAP: i32 = 2;
const MARGIN: i32 = 2;

struct Bar {
    y: i32,
    /// Blocks shown, of `slots`.
    blocks: i32,
    percent: String,
}

pub struct Resources {
    x: i32,
    w: i32,
    h: i32,
    slots: i32,
    field: u32,
    cpu: Bar,
    ram: Bar,
    /// Idle and total processor time at the last tick, in 100 ns.
    last: Option<(u64, u64)>,
    /// For the test harness: fixed readings instead of the machine's.
    fixed: Option<(f64, f64)>,
}

impl Resources {
    pub fn new() -> Resources {
        let w = SKIN.int("BarW", 318);
        Resources {
            x: SKIN.int("BarX", 31),
            w,
            h: SKIN.int("BarH", 16),
            // the trailing block needs no gap after it
            slots: ((w + GAP) / (BLOCK + GAP)).max(1),
            field: SKIN.colour("FieldBg"),
            // Everything covered until the first reading: without one the
            // bars read empty, not full.
            cpu: Bar { y: SKIN.int("CpuY", 45), blocks: 0, percent: String::new() },
            ram: Bar { y: SKIN.int("RamY", 87), blocks: 0, percent: String::new() },
            last: None,
            fixed: None,
        }
    }

    pub fn set_fixed(&mut self, cpu: f64, ram: f64) {
        self.fixed = Some((cpu, ram));
    }

    /// CPU load: the share of the last interval the processors
    /// were not idle.  The first reading has nothing to compare with and is 0.
    fn cpu_load(&mut self) -> f64 {
        let (mut idle, mut kernel, mut user) = (sys::FILETIME::default(), sys::FILETIME::default(), sys::FILETIME::default());
        if unsafe { sys::GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return 0.0;
        }
        let ticks = |ft: sys::FILETIME| (ft.hi as u64) << 32 | ft.lo as u64;
        // kernel time includes idle time
        let (idle, total) = (ticks(idle), ticks(kernel) + ticks(user));
        let load = match self.last {
            Some((last_idle, last_total)) if total > last_total => {
                1.0 - (idle.saturating_sub(last_idle)) as f64 / (total - last_total) as f64
            }
            _ => 0.0,
        };
        self.last = Some((idle, total));
        load.clamp(0.0, 1.0)
    }

    /// Memory load: memory in use over memory fitted.
    fn ram_load() -> f64 {
        let mut status = sys::MEMORYSTATUSEX { dwLength: size_of::<sys::MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { sys::GlobalMemoryStatusEx(&mut status) } == 0 || status.ullTotalPhys == 0 {
            return 0.0;
        }
        (status.ullTotalPhys - status.ullAvailPhys) as f64 / status.ullTotalPhys as f64
    }

    fn set(bar: &mut Bar, load: f64, slots: i32) -> bool {
        let blocks = (load * slots as f64 + 0.5).floor() as i32;
        let percent = format!("{} %", (load * 100.0 + 0.5).floor() as i32);
        let changed = blocks != bar.blocks || percent != bar.percent;
        bar.blocks = blocks;
        bar.percent = percent;
        changed
    }

    /// The label above a bar and the trough under it.
    fn gauge_rect(&self, bar: &Bar) -> Rect {
        let pad = SKIN.int("Pad", 12);
        Rect::new(0, pad + bar.y - 20, SKIN.w, pad + bar.y + self.h + 2)
    }

    fn cover(&self, bar: &Bar, x: i32, y: i32, canvas: &mut Canvas) {
        if bar.blocks >= self.slots {
            return;
        }
        // Nothing kept: cover the margin too.  Otherwise from the gap after
        // the last kept block to the far edge of the field.
        let from = if bar.blocks > 0 { self.x + bar.blocks * (BLOCK + GAP) - GAP } else { self.x - MARGIN };
        let to = self.x + self.w + MARGIN;
        canvas.fill(x + from, y + bar.y, to - from, self.h, self.field);
    }
}

impl Hooks for Resources {
    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "CpuPct" => &self.cpu.percent,
            "RamPct" => &self.ram.percent,
            _ => "",
        });
    }

    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        match meter.name {
            "MeterCpuCover" => self.cover(&self.cpu, x, y, canvas),
            "MeterRamCover" => self.cover(&self.ram, x, y, canvas),
            _ => {}
        }
    }
}

impl Widget for Resources {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Resource Meter\n\n\
         CPU usage\tthe share of the last second the processors were busy\n\
         RAM usage\tphysical memory in use\n\n\
         Each bar is 32 blocks; a block is about three percent."
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> Dirty {
        let (cpu, ram) = match self.fixed {
            Some(f) => f,
            None => (self.cpu_load(), Resources::ram_load()),
        };
        let mut dirty = Dirty::None;
        if Resources::set(&mut self.cpu, cpu, self.slots) {
            dirty = dirty.and(Dirty::Part(self.gauge_rect(&self.cpu)));
        }
        if Resources::set(&mut self.ram, ram, self.slots) {
            dirty = dirty.and(Dirty::Part(self.gauge_rect(&self.ram)));
        }
        dirty
    }

    fn command(&mut self, cmd: &str, _arg: i32, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        match cmd {
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }
}
