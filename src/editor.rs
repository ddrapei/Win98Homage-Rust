//! The note as something typed on: its text, the caret and the selection in
//! it, what each key does to them, undo -- and the rows the text wraps to.
//!
//! The text is the note file, character for character, with `\n` between
//! lines.  A line that opens with `[ ]` or `[x]` is a checklist item: those
//! brackets (and the space after them) are in the text but never on the
//! paper, where a checkbox in the margin stands for them.  So the caret is
//! kept out of them, and the keys that would otherwise meet them know what to
//! do instead: Backspace at the head of an item takes its checkbox away,
//! Enter on an item starts another, and typing `[ ]` at the start of a line
//! makes one.
//!
//! Nothing here knows about Windows, the file or the screen.  Widths come
//! from whoever calls `layout`, which is what lets the same rows be drawn
//! whether or not anyone is typing -- see `notes`.

/// The spaces a tab is as wide as.
pub const TAB: usize = 4;
/// Steps of undo kept.
const UNDO_MAX: usize = 200;

pub fn blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

// ---------------------------------------------------------------------------
//  Lines and rows
// ---------------------------------------------------------------------------

/// The `[ ]` or `[x]` a checklist item's line opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    /// How much of the line is not shown: its indent, the brackets, and one
    /// space after them if there is one.
    pub hidden: usize,
    /// Where the character between the brackets is.
    pub mark: usize,
    pub done: bool,
}

pub fn marker(line: &[char]) -> Option<Marker> {
    let lead = line.iter().take_while(|&&c| blank(c)).count();
    match line.get(lead..lead + 3)? {
        ['[', m @ (' ' | 'x' | 'X'), ']'] => {
            let space = usize::from(line.get(lead + 3) == Some(&' '));
            Some(Marker { hidden: lead + 3 + space, mark: lead + 1, done: *m != ' ' })
        }
        _ => None,
    }
}

/// One row of the paper: a run of the text, and where a caret stands in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The characters it shows are `start..end` of the text.
    pub start: usize,
    pub end: usize,
    /// Px from the left of the text column to each caret position,
    /// `start..=end`: one more entry than the row has characters.
    pub xs: Vec<f32>,
    /// The first row of its line -- the one a checkbox hangs off -- and the
    /// last, whose end is the end of the line.
    pub head: bool,
    pub last: bool,
    /// Its line is a checklist item, and a ticked one.
    pub todo: bool,
    pub done: bool,
    /// Where its line starts in the text, hidden marker and all.
    pub line: usize,
}

/// Where a line of these characters and advances breaks into rows of `room`
/// px, as ranges of it.  Rows after the first are `hang` px narrower.
///
/// Words are kept whole and blanks never start a row: at the end of one they
/// run on past its edge unseen, as they do in any edit box.  A word that no
/// row could hold fills the row it starts on and is cut where it stops
/// fitting, rather than leave that row half empty.
fn wrap(chars: &[char], adv: &[f32], room: f32, hang: f32) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let (mut start, mut width) = (0, 0.0f32);
    // the latest place this row could end: just after a blank
    let mut gap: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        if blank(chars[i]) {
            width += adv[i];
            i += 1;
            gap = Some(i);
            continue;
        }
        let avail = room - if rows.is_empty() { 0.0 } else { hang };
        if width + adv[i] > avail && i > start {
            let whole = |from: usize| {
                let end = chars[from..].iter().position(|&c| blank(c)).map_or(chars.len(), |n| from + n);
                adv[from..end].iter().sum::<f32>() <= room - hang
            };
            let cut = gap.filter(|&g| g > start && whole(g)).unwrap_or(i);
            rows.push((start, cut));
            start = cut;
            width = adv[cut..i].iter().sum();
            gap = None;
            continue; // this character again, against the new row
        }
        width += adv[i];
        i += 1;
    }
    rows.push((start, chars.len()));
    rows
}

/// Every line of `text` wrapped to `room` px, in one flat list.  `measure`
/// gives the advance of each character of a line's shown part.
///
/// A line's indent is repeated on the rows it wraps to, unless that would
/// leave them less than `min_room` to wrap in.
pub fn layout(text: &[char], room: f32, min_room: f32, measure: &mut dyn FnMut(&[char]) -> Vec<f32>) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut at = 0;
    for line in text.split(|&c| c == '\n') {
        let mark = marker(line);
        let hidden = mark.map_or(0, |m| m.hidden);
        let shown = &line[hidden..];
        let mut adv = measure(shown);
        adv.resize(shown.len(), 0.0);

        // An item's indent is hidden with its brackets: the box is in the margin.
        let indent = if mark.is_some() { 0 } else { shown.iter().take_while(|&&c| blank(c)).count() };
        // whole pixels, so the rows under the first start where its words do
        let mut hang = adv[..indent].iter().sum::<f32>().round();
        if room - hang < min_room {
            hang = 0.0;
        }

        let pieces = wrap(shown, &adv, room, hang);
        let count = pieces.len();
        for (n, (a, b)) in pieces.into_iter().enumerate() {
            let mut x = if n == 0 { 0.0 } else { hang };
            let mut xs = Vec::with_capacity(b - a + 1);
            xs.push(x);
            for w in &adv[a..b] {
                x += w;
                xs.push(x);
            }
            rows.push(Row {
                start: at + hidden + a,
                end: at + hidden + b,
                xs,
                head: n == 0,
                last: n + 1 == count,
                todo: mark.is_some(),
                done: mark.is_some_and(|m| m.done),
                line: at,
            });
        }
        at += line.len() + 1;
    }
    rows
}

// ---------------------------------------------------------------------------
//  The text, the caret, and what the keys do
// ---------------------------------------------------------------------------

/// What the last change was, for undo: typing that carries straight on from
/// more typing is one step, not one a letter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    None,
    Typing,
    Erasing,
}

struct State {
    text: Vec<char>,
    caret: usize,
    anchor: usize,
}

pub struct Doc {
    text: Vec<char>,
    caret: usize,
    /// The other end of the selection; the caret itself when there is none.
    anchor: usize,
    /// The caret is at the end of a wrapped row, not at the head of the next
    /// -- the same place in the text, a different place on the paper.
    eol: bool,
    /// The x the Up and Down keys are steering for, while they are held to.
    goal: Option<f32>,
    undo: Vec<State>,
    redo: Vec<State>,
    run: Run,
    /// A checkbox has just been typed: the space that follows `[ ]` out of
    /// habit is already there.
    swallow: bool,
    /// Counts the changes to the text.
    rev: u64,
}

impl Doc {
    pub fn new() -> Doc {
        Doc {
            text: Vec::new(),
            caret: 0,
            anchor: 0,
            eol: false,
            goal: None,
            undo: Vec::new(),
            redo: Vec::new(),
            run: Run::None,
            swallow: false,
            rev: 0,
        }
    }

    /// Start again from `text`: the file has been read.
    pub fn load(&mut self, text: Vec<char>) {
        self.text = text;
        self.caret = self.caret.min(self.text.len());
        self.anchor = self.caret;
        self.undo.clear();
        self.redo.clear();
        self.run = Run::None;
        self.swallow = false;
        self.rev += 1;
        self.settle();
    }

    pub fn text(&self) -> &[char] {
        &self.text
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selection, as start..end; empty when there is none.
    pub fn range(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn selection(&self) -> Option<String> {
        let (a, b) = self.range();
        (a != b).then(|| self.text[a..b].iter().collect())
    }

    pub fn deselect(&mut self) {
        self.anchor = self.caret;
    }

    // ---- finding places ---------------------------------------------------

    fn line_start(&self, p: usize) -> usize {
        self.text[..p].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self, p: usize) -> usize {
        self.text[p..].iter().position(|&c| c == '\n').map_or(self.text.len(), |i| p + i)
    }

    /// Where the shown part of the line starting at `line` begins.
    fn text_start(&self, line: usize) -> usize {
        line + marker(&self.text[line..self.line_end(line)]).map_or(0, |m| m.hidden)
    }

    /// The nearest place to `p` a caret may stand: not inside a marker.
    /// Coming from the right, that is the end of the line above.
    fn snap(&self, p: usize, back: bool) -> usize {
        let line = self.line_start(p);
        let shown = self.text_start(line);
        if p >= shown {
            p
        } else if back && line > 0 {
            line - 1
        } else {
            shown
        }
    }

    fn word_start(&self, p: usize, floor: usize) -> usize {
        let mut i = p;
        while i > floor && blank(self.text[i - 1]) {
            i -= 1;
        }
        while i > floor && !blank(self.text[i - 1]) {
            i -= 1;
        }
        i
    }

    /// The start of the word after the one at `p`, as Ctrl-Right has it.
    fn word_next(&self, p: usize, ceil: usize) -> usize {
        let mut i = p;
        while i < ceil && !blank(self.text[i]) {
            i += 1;
        }
        while i < ceil && blank(self.text[i]) {
            i += 1;
        }
        i
    }

    /// The row the caret is on and its x there.
    pub fn locate(&self, rows: &[Row]) -> (usize, f32) {
        if rows.is_empty() {
            return (0, 0.0);
        }
        let mut r = rows.partition_point(|row| row.start <= self.caret).saturating_sub(1);
        if self.eol && r > 0 && !rows[r].head && rows[r].start == self.caret {
            r -= 1;
        }
        let row = &rows[r];
        (r, row.xs[self.caret.saturating_sub(row.start).min(row.xs.len() - 1)])
    }

    /// The caret position on row `r` nearest to `x`, and whether that is the
    /// end of a wrapped row.
    pub fn hit(rows: &[Row], r: usize, x: f32) -> (usize, bool) {
        let row = &rows[r];
        let mut best = 0;
        for (i, at) in row.xs.iter().enumerate() {
            if (at - x).abs() < (row.xs[best] - x).abs() {
                best = i;
            }
        }
        let pos = row.start + best;
        (pos, pos == row.end && !row.last && best > 0)
    }

    // ---- moving -----------------------------------------------------------

    /// Put the caret at `p`, taking the selection along or dropping it.
    fn go(&mut self, p: usize, extend: bool) {
        self.caret = p;
        if !extend {
            self.anchor = p;
        }
        self.eol = false;
        self.goal = None;
        self.run = Run::None;
        self.swallow = false;
    }

    /// A click at `pos`; with Shift, the selection reaches to it.
    pub fn click(&mut self, pos: usize, eol: bool, extend: bool) {
        self.go(self.snap(pos, false), extend);
        self.eol = eol;
    }

    /// A double-click: the word there.
    pub fn select_word(&mut self, pos: usize) {
        let p = self.snap(pos, false);
        let (floor, ceil) = (self.text_start(self.line_start(p)), self.line_end(p));
        let in_word = |i: usize| i >= floor && i < ceil && !blank(self.text[i]);
        let at = if in_word(p) {
            p
        } else if p > floor && in_word(p - 1) {
            p - 1
        } else {
            return self.go(p, false);
        };
        let (mut a, mut b) = (at, at);
        while a > floor && !blank(self.text[a - 1]) {
            a -= 1;
        }
        while b < ceil && !blank(self.text[b]) {
            b += 1;
        }
        self.go(b, false);
        self.anchor = a;
    }

    pub fn select_all(&mut self) {
        self.go(self.text.len(), false);
        self.anchor = self.snap(0, false);
    }

    pub fn left(&mut self, word: bool, extend: bool) {
        let (a, b) = self.range();
        if a != b && !extend {
            return self.go(a, false);
        }
        let p = self.caret;
        let line = self.line_start(p);
        let shown = self.text_start(line);
        let to = if p <= shown {
            if line == 0 { p } else { line - 1 }
        } else if word {
            self.word_start(p, shown)
        } else {
            p - 1
        };
        self.go(to, extend);
    }

    pub fn right(&mut self, word: bool, extend: bool) {
        let (a, b) = self.range();
        if a != b && !extend {
            return self.go(b, false);
        }
        let p = self.caret;
        let end = self.line_end(p);
        let to = if p >= end {
            if end == self.text.len() { p } else { self.text_start(end + 1) }
        } else if word {
            self.word_next(p, end)
        } else {
            p + 1
        };
        self.go(to, extend);
    }

    /// Up or down `by` rows, holding to the x the first of them left from.
    pub fn vertical(&mut self, rows: &[Row], by: isize, extend: bool) {
        if rows.is_empty() {
            return;
        }
        let (r, x) = self.locate(rows);
        let goal = self.goal.unwrap_or(x);
        let to = (r as isize + by).clamp(0, rows.len() as isize - 1) as usize;
        if to != r {
            let (pos, eol) = Doc::hit(rows, to, goal);
            self.go(pos, extend);
            self.eol = eol;
        }
        self.goal = Some(goal);
    }

    /// Home: the start of the row, or with `whole` of the note.
    pub fn home(&mut self, rows: &[Row], whole: bool, extend: bool) {
        let to = if whole || rows.is_empty() { self.snap(0, false) } else { rows[self.locate(rows).0].start };
        self.go(to, extend);
    }

    /// End: the end of the row, or with `whole` of the note.
    pub fn end(&mut self, rows: &[Row], whole: bool, extend: bool) {
        if whole || rows.is_empty() {
            return self.go(self.text.len(), extend);
        }
        let row = &rows[self.locate(rows).0];
        self.go(row.end, extend);
        self.eol = !row.last;
    }

    /// The selection follows the pointer to `pos`.
    pub fn drag(&mut self, pos: usize, eol: bool) {
        let anchor = self.anchor;
        self.go(self.snap(pos, false), true);
        self.anchor = anchor;
        self.eol = eol;
    }

    // ---- changing ---------------------------------------------------------

    /// Remember things as they are, before a change -- unless the change
    /// carries on a run of the same kind.
    fn save(&mut self, run: Run) {
        if run != Run::None && run == self.run {
            return;
        }
        self.undo.push(State { text: self.text.clone(), caret: self.caret, anchor: self.anchor });
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.run = run;
    }

    fn replace(&mut self, a: usize, b: usize, with: &[char]) {
        self.text.splice(a..b, with.iter().copied());
        self.caret = a + with.len();
        self.anchor = self.caret;
        self.rev += 1;
    }

    /// After a change: a line may have become an item, or stopped being one.
    fn settle(&mut self) {
        self.caret = self.snap(self.caret, false);
        self.anchor = self.snap(self.anchor, false);
        self.eol = false;
        self.goal = None;
    }

    fn erase_selection(&mut self) -> bool {
        let (a, b) = self.range();
        if a == b {
            return false;
        }
        self.save(Run::None);
        self.replace(a, b, &[]);
        self.settle();
        true
    }

    /// One typed character, over the selection if there is one.
    pub fn type_char(&mut self, c: char) {
        if core::mem::take(&mut self.swallow) && c == ' ' && self.caret == self.anchor {
            return;
        }
        // a blank starts a new step: undo goes back a word at a time
        if blank(c) || self.caret != self.anchor {
            self.run = Run::None;
        }
        self.save(Run::Typing);
        let (a, b) = self.range();
        self.replace(a, b, &[c]);

        // "[ ]" finished at the head of a line: it is a checkbox from here on,
        // and the caret goes to where its words will.
        if c == ']' {
            let line = self.line_start(self.caret);
            if let Some(m) = marker(&self.text[line..self.line_end(line)]) {
                if self.caret == line + m.mark + 2 {
                    if self.text.get(self.caret) != Some(&' ') {
                        self.text.insert(self.caret, ' ');
                    }
                    self.caret += 1;
                    self.anchor = self.caret;
                    self.swallow = true;
                }
            }
        }
        self.settle();
    }

    /// Text put in whole -- a paste -- over the selection if there is one.
    pub fn insert(&mut self, s: &str) {
        let mut chars = Vec::with_capacity(s.len());
        let mut rest = s.chars().peekable();
        while let Some(c) = rest.next() {
            match c {
                '\r' => {
                    if rest.peek() == Some(&'\n') {
                        rest.next();
                    }
                    chars.push('\n');
                }
                '\0' => {}
                c => chars.push(c),
            }
        }
        if chars.is_empty() && self.caret == self.anchor {
            return;
        }
        self.swallow = false;
        self.save(Run::None);
        let (a, b) = self.range();
        self.replace(a, b, &chars);
        self.settle();
    }

    pub fn enter(&mut self) {
        self.swallow = false;
        self.save(Run::None);
        let (a, b) = self.range();
        self.replace(a, b, &[]);
        let (line, end) = (self.line_start(self.caret), self.line_end(self.caret));
        match marker(&self.text[line..end]) {
            // An item with nothing in it: Enter ends the list, not adds to it.
            Some(m) if line + m.hidden == end => self.replace(line, end, &[]),
            Some(m) => {
                let mut fresh: Vec<char> = self.text[line..line + m.mark - 1].to_vec();
                fresh.extend(['[', ' ', ']', ' ']);
                if self.caret == line + m.hidden {
                    // at the head of an item: the new one goes above it, and
                    // this one keeps its tick
                    fresh.push('\n');
                    let at = self.caret + fresh.len();
                    self.replace(line, line, &fresh);
                    self.caret = at;
                    self.anchor = at;
                } else {
                    fresh.insert(0, '\n');
                    self.replace(self.caret, self.caret, &fresh);
                }
            }
            None => self.replace(self.caret, self.caret, &['\n']),
        }
        self.settle();
    }

    /// Backspace; with `word`, back to the start of the word.
    pub fn backspace(&mut self, word: bool) {
        self.swallow = false;
        if self.erase_selection() {
            return;
        }
        let p = self.caret;
        let line = self.line_start(p);
        let shown = self.text_start(line);
        let (from, run) = if p == shown && shown > line {
            (line, Run::None) // the checkbox
        } else if p == line {
            if line == 0 {
                return;
            }
            (line - 1, Run::None) // the line break
        } else if word {
            (self.word_start(p, shown), Run::None)
        } else {
            (p - 1, Run::Erasing)
        };
        self.save(run);
        self.replace(from, p, &[]);
        self.settle();
    }

    /// Delete; with `word`, up to the start of the next word.
    pub fn delete(&mut self, word: bool) {
        self.swallow = false;
        if self.erase_selection() {
            return;
        }
        let p = self.caret;
        let end = self.line_end(p);
        let (to, run) = if p == end {
            if end == self.text.len() {
                return;
            }
            // the line below joins this one, and leaves its checkbox behind
            (self.text_start(end + 1), Run::None)
        } else if word {
            (self.word_next(p, end), Run::None)
        } else {
            (p + 1, Run::Erasing)
        };
        self.save(run);
        self.replace(p, to, &[]);
        self.settle();
    }

    /// Tick or untick the item whose line starts at `line`.
    pub fn toggle(&mut self, line: usize) -> bool {
        let Some(m) = marker(&self.text[line..self.line_end(line)]) else { return false };
        self.save(Run::None);
        self.text[line + m.mark] = if m.done { ' ' } else { 'x' };
        self.rev += 1;
        true
    }

    fn swap(&mut self, to: State) -> State {
        let was = State { text: core::mem::replace(&mut self.text, to.text), caret: self.caret, anchor: self.anchor };
        self.caret = to.caret;
        self.anchor = to.anchor;
        self.run = Run::None;
        self.swallow = false;
        self.eol = false;
        self.goal = None;
        self.rev += 1;
        was
    }

    pub fn undo(&mut self) -> bool {
        let Some(to) = self.undo.pop() else { return false };
        let was = self.swap(to);
        self.redo.push(was);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(to) = self.redo.pop() else { return false };
        let was = self.swap(to);
        self.undo.push(was);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn doc(s: &str) -> Doc {
        let mut d = Doc::new();
        d.load(chars(s));
        d
    }

    fn text(d: &Doc) -> String {
        d.text().iter().collect()
    }

    /// Every character 6 px wide.
    fn six(line: &[char]) -> Vec<f32> {
        vec![6.0; line.len()]
    }

    fn shown(text: &str, room: f32) -> Vec<String> {
        let t = chars(text);
        layout(&t, room, 24.0, &mut six).iter().map(|r| t[r.start..r.end].iter().collect()).collect()
    }

    #[test]
    fn a_marker_is_brackets_at_the_head_of_a_line() {
        assert_eq!(marker(&chars("[ ] milk")), Some(Marker { hidden: 4, mark: 1, done: false }));
        assert_eq!(marker(&chars("  [x]eggs")), Some(Marker { hidden: 5, mark: 3, done: true }));
        assert_eq!(marker(&chars("milk [ ]")), None);
        assert_eq!(marker(&chars("[y] no")), None);
    }

    #[test]
    fn words_wrap_whole_and_blanks_hang() {
        // ten characters to a row
        assert_eq!(shown("one two three four", 60.0), ["one two ", "three four"]);
        assert_eq!(shown("", 60.0), [""]);
        assert_eq!(shown("a\n\nb", 60.0), ["a", "", "b"]);
    }

    #[test]
    fn a_word_too_long_for_any_row_fills_the_one_it_starts_on() {
        assert_eq!(shown("see abcdefghijklmnop", 60.0), ["see abcdef", "ghijklmnop"]);
    }

    #[test]
    fn an_indent_is_repeated_and_an_item_hides_its_brackets() {
        let t = chars("  one two three\n[x] milk");
        let rows = layout(&t, 60.0, 24.0, &mut six);
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].xs[0], rows[1].xs[0]), (0.0, 12.0));
        assert!(rows[2].todo && rows[2].done && rows[2].head);
        assert_eq!(t[rows[2].start..rows[2].end].iter().collect::<String>(), "milk");
        assert_eq!(rows[2].line, 16);
    }

    #[test]
    fn typing_brackets_makes_a_checkbox() {
        let mut d = doc("");
        for c in "[ ] milk".chars() {
            d.type_char(c);
        }
        assert_eq!(text(&d), "[ ] milk");
        let mut d = doc("");
        for c in "[x]eggs".chars() {
            d.type_char(c);
        }
        assert_eq!(text(&d), "[x] eggs");
        assert!(d.undo());
        assert_eq!(text(&d), "");
    }

    #[test]
    fn enter_on_an_item_starts_another_and_on_an_empty_one_ends_the_list() {
        let mut d = doc("[x] milk");
        d.end(&[], true, false);
        d.enter();
        assert_eq!(text(&d), "[x] milk\n[ ] ");
        assert_eq!(d.caret(), 13);
        d.enter();
        assert_eq!(text(&d), "[x] milk\n");
        // at the head of an item the new one goes above
        let mut d = doc("[x] milk");
        d.enter();
        assert_eq!(text(&d), "[ ] \n[x] milk");
        assert_eq!(d.caret(), 9);
    }

    #[test]
    fn backspace_at_the_head_of_an_item_takes_the_checkbox() {
        let mut d = doc("one\n[ ] two");
        d.click(8, false, false);
        d.backspace(false);
        assert_eq!(text(&d), "one\ntwo");
        d.backspace(false);
        assert_eq!(text(&d), "onetwo");
        assert_eq!(d.caret(), 3);
    }

    #[test]
    fn delete_at_the_end_of_a_line_joins_the_item_below_without_its_brackets() {
        let mut d = doc("one\n[ ] two");
        d.click(3, false, false);
        d.delete(false);
        assert_eq!(text(&d), "onetwo");
    }

    #[test]
    fn the_caret_steps_over_a_marker() {
        let mut d = doc("one\n[ ] two");
        d.click(3, false, false);
        d.right(false, false);
        assert_eq!(d.caret(), 8);
        d.left(false, false);
        assert_eq!(d.caret(), 3);
        // a click cannot land in it either
        d.click(5, false, false);
        assert_eq!(d.caret(), 8);
    }

    #[test]
    fn up_and_down_hold_their_column() {
        let t = "abcdefgh\nab\nabcdefgh";
        let mut d = doc(t);
        let rows = layout(d.text(), 600.0, 24.0, &mut six);
        d.click(6, false, false);
        d.vertical(&rows, 1, false);
        assert_eq!(d.caret(), 11);
        d.vertical(&rows, 1, false);
        assert_eq!(d.caret(), 18);
    }

    #[test]
    fn end_on_a_wrapped_row_stays_on_that_row() {
        let mut d = doc("one two three four");
        let rows = layout(d.text(), 60.0, 24.0, &mut six);
        d.end(&rows, false, false);
        assert_eq!(d.caret(), 8);
        assert_eq!(d.locate(&rows).0, 0);
        d.home(&rows, false, false);
        assert_eq!(d.caret(), 0);
    }

    #[test]
    fn a_double_click_takes_the_word_and_typing_replaces_it() {
        let mut d = doc("buy oat milk");
        d.select_word(5);
        assert_eq!(d.selection().as_deref(), Some("oat"));
        d.type_char('x');
        assert_eq!(text(&d), "buy x milk");
        assert!(d.undo());
        assert_eq!(text(&d), "buy oat milk");
        assert!(d.redo());
        assert_eq!(text(&d), "buy x milk");
    }

    #[test]
    fn a_paste_keeps_its_lines_whatever_ends_them() {
        let mut d = doc("");
        d.insert("a\r\nb\rc\n[ ] d");
        assert_eq!(text(&d), "a\nb\nc\n[ ] d");
        assert!(d.toggle(6));
        assert_eq!(text(&d), "a\nb\nc\n[x] d");
    }
}
