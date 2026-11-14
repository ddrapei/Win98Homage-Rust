# Win98 Widgets

Six Windows 98 style desktop widgets — Date/Time Properties, Resource Meter, Sticky Note,
Weather, Rain Window and Disk Space — as one small Rust program. There is one of each, and as many
more Sticky Notes as you ask for.

| | |
|--------------------------|---------|
| private memory           | ~2.5 MB |
| working set              | 2–3 MB idle |
| run-time dependencies    | none |
| program                  | one 700 KB .exe |

Measured with the first four widgets showing, before Rain Window and Disk Space were added.

## Build and run

```text
cargo build --release        ->  target\release\win98-widgets.exe
```

crates; the only build-time one is `png`, used to decode the weather icons.

Run the .exe. Nothing is installed: it can live anywhere. Only one copy runs at a time.

## The widgets are the skins

Each widget's layout is an `.ini` file in [`skins/`](skins): a `[Variables]` section for the
palette and the layout figures, then one section per meter — a shape, a line of text or an
image — in the order they are painted. `build.rs` compiles the six of them into static
tables — every meter's position, shapes, colours, text options and mouse actions — so
**the `.ini` files are the single source of truth for the layout**. Change a colour or move
a meter there, rebuild, and the widget follows. Nothing parses an `.ini` while the program
runs: the logic behind each widget is plain Rust in `src/widgets/` (`datetime.rs`,
`resources.rs`, `notes.rs`, `weather.rs`, `rain.rs`, `disk.rs`), and a mouse action in a skin, such as
`LeftMouseUpAction=Select(7)`, names the command that logic answers to.

Text is laid out by DirectWrite and rasterised with aliased glyphs. Shapes are filled by a
small software canvas.

`--dump <dir> [--state <file>]` draws each widget off-screen, in a state given by the
file (which can ask for more than one note), and writes it out as a BMP — for comparing
before and after a change (see `src/tools/dump.rs`).

The program is deliberately not DPI aware, so on a scaled display Windows stretches the
widgets as a whole.

## Using them

They are desktop widgets, not windows: no taskbar button, not in Alt-Tab, and they never
take the keyboard (except the note and the weather's city field, while you type in
them).

| | |
|--|--|
| drag anywhere | move a widget; it snaps to screen edges and to the others (hold Ctrl or Shift not to). The note's text is the exception: dragging there selects |
| right-click | menu: the widget's own options, Position, Snap, Lock, Close, the list of widgets (with *Sticky Note +* and *−* for the notes), Start with Windows, settings, Exit |
| tray icon | the same menu — this is how a closed widget is brought back |
| `X` | closes that widget (it stops costing anything); `?` says what it does |

In the widgets themselves: tabs, month list, calendar selection, scroll to browse months,
the 12/24-hour field, checkboxes, the scrollbar, the paper swatch and the ruled-paper
button, typing on the note with save-as-you-type and a `.bak`, and the weather's city
field.

**Typing on the note.** Click the text and type: the caret goes where you clicked, and
the note looks the same as it did before you clicked — same line breaks, checkboxes still
in the margin, done items still struck through. Drag or Shift+arrows to select,
double-click for a word, Ctrl+C/X/V, Ctrl+Z/Y to undo and redo. Type `[ ]` at the start
of a line to make a checkbox; Enter on a checkbox line starts another (on an empty one it
ends the list), and Backspace at the start of one removes the checkbox. A click anywhere
else, or Escape, finishes; the note is saved a second after you stop typing and again
when you finish.

**The note's paper.** The swatch button in the note's title bar moves on to the next
paper colour each time it is clicked — canary, white, pink, green, blue, orange, lilac,
and round to canary again — and the note remembers which. The colours are the `Paper1`,
`Paper2`, … variables in `skins/Win98Notes.ini`; another line there is another colour.

**More than one note.** The other widgets are ticked on and off in the Widgets list; the
notes are counted there instead. *Sticky Note +* puts one more note on the screen and
*Sticky Note −* takes one off — the note the menu was opened on, or else the last one.
A note taken off, or closed with its `X`, keeps its text, and `+` brings those back first,
where they were, before it makes a new one; a new one goes beside the note the menu was
opened on (or the last one), in the nearest place where there is room. Each note has its
own text, its own paper and ruling and its own place; none has a title — the bar is the
icon and the buttons. Right-click a note after the first → *Delete this note* removes it
for good: its text file goes to the Recycle Bin, after a question if there is anything
written on it. The first note is the one that is always there: it can be closed but not
deleted.

**The weather's city.** Click the city field and type a place: `Lviv`, or `Paris, US` /
`Portland, Maine` to say which one. Enter (or a click anywhere else) looks it up and pins
the forecast to it; Escape leaves things as they were; an empty field goes back to the
place your public IP address gives. The pin is kept in `settings.ini` as `ManualLat`,
`ManualLon` and `ManualLocation`, which can also be written by hand.

**Rain Window** does nothing: it is rain the colour of rust falling past a small dark
window, the size of the Sticky Note, to be looked at. Right-click it for how hard it
rains — *Light*, *Steady*, *Heavy* — and to *Pause* it; both are kept in `settings.ini`
as `Rate` and `Paused`. Its colours are the `Rain*` variables in `skins/Win98Rain.ini`.

**Disk Space** shows how full one drive is: the space used and free on it, its capacity,
and the two as the pie from a drive's Properties page — the free space is the pale slice.
The arrow buttons beside the drive's name step to the drive before and the one after, as
does the wheel over the name; right-click has every drive by its letter. The choice is
kept in `settings.ini` as `Drive`; with none made it is the drive Windows is on. The
pie's colours are the `Pie*` variables in `skins/Win98Disk.ini`.

**Position** (right-click → Position): *On desktop* (default) keeps the widgets behind
every window; *Normal*; *Always on top*.

## Where things are kept

| | |
|--|--|
| `%APPDATA%\Win98Widgets\settings.ini` | positions, which widgets are shown, and each widget's knobs |
| `%APPDATA%\Win98Widgets\Notes.txt` | the note (`NoteFile=` in settings.ini to keep it elsewhere) |
| `%APPDATA%\Win98Widgets\Notes2.txt`, `Notes3.txt`, … | the notes after the first, each with a `[Notes2]`, `[Notes3]`, … section of its own in settings.ini |
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
- **Rain Window is the one widget drawn more than once a second**: 25 frames a second, of
  its pane only, from a timer that runs only while there is rain to show — not while it
  is paused or closed, nor while the workstation is locked. A frame takes about a fifth
  of a millisecond to draw.
- **Disk Space** asks a drive on this machine for its figures every five seconds, and
  repaints only when a figure it shows has changed. A network drive, a disc or a card can
  take many seconds to answer, so those are asked every half minute from a thread that
  lasts as long as the one question: no widget waits for a drive.
- The note is wrapped by the widths DirectWrite gives for the font it is drawn in, and
  drawn by one piece of code whether or not it is being typed on, so nothing on the sheet
  moves when you click in.
- `?` opens a small About box.

## Layout of the source

```text
skins/       the six layouts (.ini), the weather icons and the tray icon
build.rs     those .ini files compiled into static tables
src/
  main.rs      the way in, and the list of modules
  app.rs       the host: windows (one per widget, and per note), mouse, menus,
               tray, the once-a-second tick, the frames in between
  tools/       for working on the program
    dump         off-screen rendering, for checking a change
  widgets/     what a widget is, and the six there are
    widget       the Widget trait: all the host knows about a widget
    datetime     Date/Time Properties
    resources    Resource Meter
    notes        Sticky Note (also: which sections of settings.ini are notes)
    weather      Weather (and the child that fetches it)
    rain         Rain Window (the one that is animated)
    disk         Disk Space
  input/       typing
    keys         the window that holds the keyboard for a widget
    editor       a text and what the keys do to it: the note's, the city field's
  storage/     what is kept on disk
    settings     settings.ini
    usage        the on/unlocked journal the Date/Time graph is drawn from
  graphics/    drawing
    skin         the compiled tables, and drawing and hit-testing them
    canvas       the software canvas they are drawn on
    text         DirectWrite text, rasterised into the canvas
  platform/    Windows
    sys          the Win32 declarations all of it uses
    clock        time and time zones
```

Each folder uses only the ones below it in that list. The modules keep their short names
in code (`crate::canvas`, `crate::notes`): `main.rs` re-exports each from its folder, so
moving a file between folders changes `main.rs` and nothing else.
