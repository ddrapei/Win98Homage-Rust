# Win98 Widgets

Four Windows 98 style desktop widgets — Date/Time Properties, Resource Meter, Sticky Note
and Weather — as one small Rust program.

| | |
|--------------------------|---------|
| private memory           | ~2.5 MB |
| working set              | 2–3 MB idle |
| run-time dependencies    | none |
| program                  | one 700 KB .exe |

Measured with all four widgets showing.

## Build and run

```text
cargo build --release        ->  target\release\win98-widgets.exe
```

Any Rust toolchain will do (the GNU one needs no Visual Studio). There are no run-time
crates; the only build-time one is `png`, used to decode the weather icons.

Run the .exe. Nothing is installed: it can live anywhere. Only one copy runs at a time.

## The widgets are the skins

Each widget's layout is a Rainmeter-format `.ini` file in [`skins/`](skins). `build.rs`
compiles the four of them into static tables — every meter's position, shapes, colours,
text options and mouse actions — so **the `.ini` files are the single source of truth for
the layout**. Change a colour or move a meter there, rebuild, and the widget follows.
Nothing parses an `.ini` while the program runs, and Rainmeter itself is not involved: the
logic behind each widget is plain Rust (`datetime.rs`, `resources.rs`, `notes.rs`,
`weather.rs`).

Text is laid out by DirectWrite with the same calls Rainmeter makes for a String meter
(`AccurateText=1`, `AntiAlias=0`), and rasterised with the same aliased glyph renderer
Direct2D would have used. Shapes are filled by a small software canvas.

Checked against Rainmeter 4.5.28 drawing the same layouts: with the same clock time,
usage log, note and readings fed to both, each widget's render was compared with a
capture of Rainmeter's — **0 differing pixels in all four** at a tolerance of 10/255 (the
capture path itself filters slightly); at 4/255 the only differences left are the
antialiased edges of the clock hands, by at most 10 levels. `--dump` (see `src/dump.rs`)
is the harness.

Like Rainmeter 4.5, the program is deliberately not DPI aware, so on a scaled display
Windows stretches the widgets as a whole.

## Using them

They are desktop widgets, not windows: no taskbar button, not in Alt-Tab, and they never
take the keyboard (except the note's edit box, while you type in it).

| | |
|--|--|
| drag anywhere | move a widget; it snaps to screen edges and to the others (hold Ctrl or Shift not to) |
| right-click | menu: the widget's own options, Position, Snap, Lock, Close, the list of widgets, Start with Windows, settings, Exit |
| tray icon | the same menu — this is how a closed widget is brought back |
| `X` | closes that widget (it stops costing anything); `?` says what it does |

In the widgets themselves: tabs, month list, calendar selection, scroll to browse months,
the 12/24-hour field, checkboxes, the scrollbar, the paper swatch, and
click-the-paper-to-edit with save-as-you-type and a `.bak`.

**Position** (right-click → Position): *On desktop* (default) keeps the widgets behind
every window; *Normal*; *Always on top*.

## Where things are kept

| | |
|--|--|
| `%APPDATA%\Win98Widgets\settings.ini` | positions, which widgets are shown, and each widget's knobs |
| `%APPDATA%\Win98Widgets\Notes.txt` | the note (`NoteFile=` in settings.ini to keep it elsewhere) |
| `%LOCALAPPDATA%\Win98DateTime\Usage.log`, or `%LOCALAPPDATA%\Win98DateTime.log` | the usage log |
| `%LOCALAPPDATA%\Win98Widgets\weather.txt` | the last forecast |

A `settings.ini` beside the .exe is used instead, if there is one (a portable copy).

## How they behave

- **Locked or not** comes from Windows' own session notifications. Old log entries are
  still run through a 30-second filter for momentary false "unlocked" readings.
- **The sign-in time** is read in UTC, so "date the session from sign-in" holds in every
  time zone.
- **The second hand** moves on the second: the tick is re-aimed at each second's start.
- **Weather** shows the last forecast immediately after a restart, and each fetch runs in
  a short-lived child process, so the networking libraries are never resident.
- **Nothing is drawn while the workstation is locked**; the usage log keeps running.
- The note is wrapped by a fixed table of character widths, so it wraps in the same place
  on every machine.
- `?` opens a small About box.

## Layout of the source

```text
skins/       the four layouts (.ini) and the weather icons
build.rs     those .ini files compiled into static tables
skin         the tables, and drawing and hit-testing them
canvas       the software canvas they are drawn on
text         DirectWrite text, laid out as Rainmeter lays it out
datetime     Date/Time Properties   (usage: the on/unlocked journal)
resources    Resource Meter
notes        Sticky Note            (editor: the box you type in)
weather      Weather                (and the child that fetches it)
app          windows, mouse, menus, tray, the once-a-second tick
settings     settings.ini
clock        time and time zones
sys          the Win32 declarations all of it uses
dump         off-screen rendering, for checking against Rainmeter
```
