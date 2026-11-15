use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use doc_model::{CommentId, CommentInfo, GroupId, Place, Scope, VisibleGroup};
use gpui_kit::*;
use l10n::tr;

use crate::actions::*;
use crate::markdown::{self, BlockKind, LineStyle, Span};
use crate::drag::TextDrag;
use crate::model::DocModel;
use crate::prefs::{Alignment, ParagraphLook, Prefs, WheelSwitch};
use crate::sentence_keys;
use crate::text::TextIndex;
use crate::theme::{self, c, ca};

const CUT_EDIT_ALPHA: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Page,
    Cut(doc_model::CutId),
    Field,
    Comment(CommentId),
}

#[derive(Clone, Debug)]
pub enum ViewEvent {
    Edited,
    SelectionChanged,
    Cycled { group: GroupId, is_original: bool },
    Confirm,
    Cancel,
    /// ⌘⌫ in an empty field. Plain ⌫ doesn't emit, so a press too many removes nothing.
    DeleteLineEmpty,
    ContextMenu(Point<Pixels>),
    OpenGroup(GroupId),
    MarkHover(Option<doc_model::CutId>),
    TraceMenu(doc_model::CutId, Point<Pixels>),
    PointerMoved,
    /// ↑ (-1) or ↓ (+1) in a single-line field.
    Arrow(isize),
    History { redo: bool },
    /// A click on a trim's faded word: keep the cut starting at this char.
    KeepCut(usize),
    /// ⌘Z or ⇧⌘Z found the drawer's next step tied to a file that isn't on the page.
    HistoryElsewhere(u64),
    DragStarted,
    Tab,
    Cut(doc_model::CutId),
    PasteCut,
    /// ⌘Z or ⇧⌘Z on a step tied to another chapter.
    LinkedHistory { link: u64, redo: bool },
    CommentHover(Option<CommentId>),
    EditComment(CommentId),
    SceneBreakHint,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabPaint {
    pub marks: Vec<PaintMark>,
    /// (range, whether Walk through is on it).
    pub cuts: Vec<(Range<usize>, bool)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintMark {
    pub range: Range<usize>,
    /// A dotted underline under words; otherwise a wash over the sentence.
    pub word: bool,
    pub current: bool,
    pub note: Option<String>,
}

impl EventEmitter<ViewEvent> for TextView {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragUnit {
    Char,
    Word,
    Paragraph,
}

struct Wheel {
    group: Option<GroupId>,
    travel: Pixels,
    last: Instant,
    lifted: bool,
    /// Whether the gesture runs sideways, fixed by its first movement; `None` until it moves.
    sideways: Option<bool>,
}

/// Mouse wheels report no phases: a wheel quiet this long has ended its gesture.
const WHEEL_REST: Duration = Duration::from_millis(250);
const WHEEL_STEP: Pixels = px(48.);
pub const ZEN_DIM: f32 = 0.3;
const ZEN_FADE: Duration = Duration::from_millis(200);
const GLIDE: Duration = Duration::from_millis(150);
const CARET_SLIDE: Duration = Duration::from_millis(90);

/// `from` and `to` are relative to the content's top-left, so scrolling carries the caret.
#[derive(Clone, Copy, Debug)]
struct CaretSlide {
    from: Point<Pixels>,
    to: Point<Pixels>,
    since: Instant,
    /// A resize rewraps the text, so the caret snaps instead of sliding.
    column_width: Pixels,
}

impl CaretSlide {
    fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.since).as_secs_f32() / CARET_SLIDE.as_secs_f32()).min(1.)
    }

    fn at(&self, now: Instant) -> Point<Pixels> {
        let k = 1. - (1. - self.progress(now)).powi(3);
        point(self.from.x + (self.to.x - self.from.x) * k, self.from.y + (self.to.y - self.from.y) * k)
    }

    fn sliding(&self, now: Instant) -> bool {
        self.progress(now) < 1.
    }
}

#[derive(Clone, Copy, Debug)]
struct Row {
    start: usize,
    end: usize,
    /// x of `start` in the unwrapped layout.
    x0: Pixels,
    hyphen: bool,
}

struct ShapedLine {
    wrapped: WrappedLine,
    rows: Vec<Row>,
    /// (row, x within it, the shaped `-`).
    hyphens: Vec<(usize, Pixels, gpui_kit::ShapedLine)>,
}

impl LineBox {
    fn shift(&self, row: usize) -> Pixels {
        if row == 0 { self.x_shift } else { px(0.) }
    }
}

struct LineBox {
    line: usize,
    shaped: Rc<ShapedLine>,
    /// Relative to the content top.
    y: Pixels,
    height: Pixels,
    /// Negative when a heading's `# ` sits left of the column (first row only).
    x_shift: Pixels,
    kind: BlockKind,
    alpha: f32,
    gap: bool,
}

pub struct Layout {
    lines: Vec<LineBox>,
    /// Window position of the content top-left, scroll applied.
    origin: Point<Pixels>,
    column_width: Pixels,
    pub(crate) line_height: Pixels,
    font_size: Pixels,
    total_height: Pixels,
    pub(crate) bounds: Bounds<Pixels>,
    index: Rc<TextIndex>,
}

impl Layout {
    fn line_box(&self, line: usize) -> Option<&LineBox> {
        // Lines are laid out in order, so the box index equals the line index.
        self.lines.get(line).filter(|b| b.line == line)
    }

    fn row_of(shaped: &ShapedLine, byte: usize) -> usize {
        shaped
            .rows
            .iter()
            .rposition(|r| r.start <= byte)
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn x_shift(&self, line: usize) -> Option<Pixels> {
        self.line_box(line).map(|b| b.x_shift)
    }

    #[cfg(test)]
    pub fn font_size_of(&self, line: usize) -> Option<Pixels> {
        self.line_box(line).map(|b| b.shaped.wrapped.unwrapped_layout.font_size)
    }

    #[cfg(test)]
    pub fn text_bounds(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        self.rects(range, false)
    }

    #[cfg(test)]
    pub fn row_ends(&self, line: usize) -> Vec<(Pixels, bool)> {
        let Some(b) = self.line_box(line) else { return Vec::new() };
        let s = self.index.line_str(line);
        let ul = &b.shaped.wrapped.unwrapped_layout;
        b.shaped
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let end = row.start + s[row.start..row.end].trim_end().len();
                let mut x = self.origin.x + b.shift(i) + ul.x_for_index(end) - row.x0;
                if let Some((_, _, h)) = b.shaped.hyphens.iter().find(|(r, ..)| *r == i) {
                    x += h.width;
                }
                (x, row.hyphen)
            })
            .collect()
    }

    /// (content top-left in window coordinates, column width).
    pub fn column(&self) -> (Point<Pixels>, Pixels) {
        (self.origin, self.column_width)
    }

    #[cfg(any(test, feature = "shots"))]
    pub fn line_height(&self) -> Pixels {
        self.line_height
    }

    #[cfg(test)]
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    pub fn alpha_at(&self, c: usize) -> f32 {
        self.line_box(self.index.line_of(c)).map_or(1., |b| b.alpha)
    }

    /// Top-left of the caret slot for `c`, in window coordinates.
    pub fn point_for(&self, c: usize) -> Option<Point<Pixels>> {
        let idx = &self.index;
        let line = idx.line_of(c.min(idx.len()));
        let b = self.line_box(line)?;
        let byte = idx.byte_in_line(line, c);
        let r = Self::row_of(&b.shaped, byte);
        let row = b.shaped.rows[r];
        let x = b.shaped.wrapped.unwrapped_layout.x_for_index(byte) - row.x0;
        Some(point(
            self.origin.x + b.shift(r) + x,
            self.origin.y + b.y + self.line_height * r as f32,
        ))
    }

    pub fn index_for(&self, p: Point<Pixels>) -> usize {
        let idx = &self.index;
        if self.lines.is_empty() {
            return 0;
        }
        let y = p.y - self.origin.y;
        let li = match self
            .lines
            .binary_search_by(|b| (b.y).partial_cmp(&y).unwrap_or(std::cmp::Ordering::Less))
        {
            Ok(i) => i,
            Err(0) => return 0,
            Err(i) => i - 1,
        };
        let b = &self.lines[li];
        if y > b.y + b.height && li + 1 == self.lines.len() {
            return idx.len();
        }
        if b.gap {
            // In a paragraph break: upper half to the end above, lower half to the start below.
            let start = idx.lines[b.line].char_start;
            return if y < b.y + b.height / 2. { start - 1 } else { start + 1 };
        }
        let rows = &b.shaped.rows;
        let r = (((y - b.y) / self.line_height).floor().max(0.) as usize).min(rows.len() - 1);
        let row = rows[r];
        let x = p.x - self.origin.x - b.shift(r) + row.x0;
        let ul = &b.shaped.wrapped.unwrapped_layout;
        let mut byte = ul.closest_index_for_x(x).clamp(row.start, row.end);
        let s = idx.line_str(b.line);
        if r + 1 < rows.len() && byte == row.end && byte > row.start && !row.hyphen {
            // Past a wrapped row's end: before the wrap (past a hyphen, where the word broke).
            byte = s[..byte].char_indices().next_back().map_or(0, |(i, _)| i);
        }
        idx.lines[b.line].char_start + s[..byte.min(s.len())].chars().count()
    }

    pub(crate) fn rects(&self, range: Range<usize>, full_rows: bool) -> Vec<Bounds<Pixels>> {
        self.row_rects(range, full_rows, false)
    }

    /// Like `rects`, but each ends at the row's last character: no space at a wrap or the end.
    pub(crate) fn ink_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        self.row_rects(range, false, true)
    }

    fn row_rects(&self, range: Range<usize>, full_rows: bool, ink: bool) -> Vec<Bounds<Pixels>> {
        let idx = &self.index;
        let mut out = Vec::new();
        if range.is_empty() {
            return out;
        }
        let (l0, l1) = (idx.line_of(range.start), idx.line_of(range.end));
        for li in l0..=l1 {
            let Some(b) = self.line_box(li) else { continue };
            // A gap is no row: the line break before it shows it's selected.
            if b.gap {
                continue;
            }
            let info = idx.lines[li];
            let s = range.start.max(info.char_start);
            let e = range.end.min(info.char_start + info.char_len);
            let (bs, be) = (idx.byte_in_line(li, s), idx.byte_in_line(li, e));
            let ul = &b.shaped.wrapped.unwrapped_layout;
            let continues = range.end > info.char_start + info.char_len;
            let rows = &b.shaped.rows;
            let text = idx.line_str(li);
            for (ri, row) in rows.iter().enumerate() {
                let (rs, mut re) = (bs.max(row.start), be.min(row.end));
                if ink && rs < re {
                    re = rs + text.get(rs..re).map_or(re - rs, |t| t.trim_end().len());
                }
                let passes_end = if ri + 1 == rows.len() { continues } else { be > row.end };
                if rs > re || (rs == re && !(full_rows && passes_end)) {
                    continue;
                }
                let x0 = ul.x_for_index(rs) - row.x0;
                let mut x1 = ul.x_for_index(re) - row.x0;
                if full_rows && passes_end && ri + 1 == rows.len() {
                    // Show that the selection includes the line break.
                    x1 += px(6.);
                }
                let top = self.origin.y + b.y + self.line_height * ri as f32;
                out.push(Bounds::from_corners(
                    point(self.origin.x + b.shift(ri) + x0, top),
                    point(self.origin.x + b.shift(ri) + x1, top + self.line_height),
                ));
            }
        }
        out
    }

    /// The paragraph break whose gap holds window y `y`, and its height.
    pub(crate) fn gap_at(&self, y: Pixels) -> Option<(usize, Pixels)> {
        let y = y - self.origin.y;
        self.lines.iter().find(|b| b.gap && b.y <= y && y < b.y + b.height).map(|b| (b.line, b.height))
    }

    pub(crate) fn space_above(&self, c: usize) -> Pixels {
        let line = self.index.line_of(c.min(self.index.len()));
        match line.checked_sub(1).and_then(|l| self.line_box(l)) {
            Some(b) if b.gap => b.height,
            _ => self.line_height,
        }
    }

    #[cfg(test)]
    pub fn gap_height(&self, line: usize) -> Option<Pixels> {
        self.line_box(line).filter(|b| b.gap).map(|b| b.height)
    }

    fn vertical_extent(&self, range: Range<usize>) -> Option<(Pixels, Pixels)> {
        let idx = &self.index;
        let (a, z) = (idx.line_of(range.start), idx.line_of(range.end));
        let ba = self.line_box(a)?;
        let bz = self.line_box(z)?;
        Some((self.origin.y + ba.y, self.origin.y + bz.y + bz.height))
    }
}

pub struct TextView {
    pub mode: Mode,
    pub model: Entity<DocModel>,
    /// Shares this one's history: the cuts document for the page, the page for a cut's editor.
    pub partner: Option<Entity<DocModel>>,
    /// Edits a cut of the project's cuts document (`partner` is the page).
    pub drawer_side: bool,
    field: String,
    field_version: u64,
    pub focus: FocusHandle,
    sel: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    goal_x: Option<Pixels>,
    index: Rc<TextIndex>,
    index_version: u64,
    styles: Rc<Vec<LineStyle>>,
    gaps: Rc<Vec<bool>>,
    groups: Rc<Vec<VisibleGroup>>,
    ghosts: Rc<Vec<Range<usize>>>,
    /// Cuts shown as a mark: (id, where, whole paragraphs).
    pub(crate) marks: Rc<Vec<(doc_model::CutId, usize, bool)>>,
    pub hovered_mark: Option<doc_model::CutId>,
    pub comments: Rc<Vec<CommentInfo>>,
    pub hovered_comment: Option<CommentId>,
    pub editing_comment: Option<CommentId>,
    pub comment_ticks: bool,
    /// Where each comment's note (`false`) or tick (`true`) was last painted.
    pub comment_rects: CommentRects,
    /// The scroll `comment_rects` were painted at.
    pub comment_rects_scroll: Cell<Pixels>,
    pub layout: Option<Rc<Layout>>,
    scroll_y: Pixels,
    autoscroll: bool,
    /// (from, to, start).
    glide: Option<(Pixels, Pixels, Instant)>,
    pub hovered_group: Option<GroupId>,
    wheel: Option<Wheel>,
    hover_char: Option<usize>,
    hover_in_bounds: bool,
    over_dots: bool,
    chip_group: Option<GroupId>,
    /// The label's grace-period token after the pointer left its area.
    chip_left: Option<u64>,
    chip_leaves: u64,
    _chip_timer: Option<Task<()>>,
    chip_rect: Rc<Cell<Option<Bounds<Pixels>>>>,
    over_chip: bool,
    sel_buttons: Option<Range<usize>>,
    drag: Option<(DragUnit, Range<usize>)>,
    /// A press inside the selection at this char: moving drags it, releasing puts the caret here.
    press_in_sel: Option<usize>,
    over_sel: bool,
    pub carrying: Option<Range<usize>>,
    pub drop_mark: Option<Place>,
    last_typing: Option<Instant>,
    caret_on: bool,
    blink_epoch: Instant,
    caret_slide: Option<CaretSlide>,
    caret_jump: bool,
    wash: Option<(GroupId, Instant)>,
    pub focus_range: Option<Range<usize>>,
    pub spotlight: Option<GroupId>,
    centre_request: Option<Range<usize>>,
    zen: Option<Range<usize>>,
    zen_prev: Option<Range<usize>>,
    zen_since: Option<Instant>,
    pub highlights: Rc<Vec<Range<usize>>>,
    pub misspelled: Rc<Vec<Range<usize>>>,
    pub lab: Rc<LabPaint>,
    spell_cache: HashMap<u64, Rc<Vec<Range<usize>>>>,
    spell_generation: u64,
    pub current_highlight: Option<usize>,
    pub raised_ghost: Option<Range<usize>>,
    pub flash: Option<(Range<usize>, Instant)>,
    /// (range, start, from opacity, to opacity).
    fades: Vec<(Range<usize>, Instant, f32, f32)>,
    pub placeholder: SharedString,
    /// Faded words drawn after the text (what may follow `??`).
    pub hint_after: SharedString,
    pub quiet_when_empty: bool,
    a11y_value: SharedString,
    cache: RefCell<HashMap<u64, Rc<ShapedLine>>>,
    /// The hyphenation language, with the text length and first-line hash it was read from.
    text_lang: Option<(usize, u64, Option<prose::Lang>)>,
    style_cache: HashMap<u64, LineStyle>,
    /// ⌃⌘↑'s steps: shrink retraces them while the selection is still the last grow's.
    grow_steps: Vec<Range<usize>>,
    grown: Option<Range<usize>>,
    /// ⇧ went down alone at this moment and nothing else has happened since.
    shift_tap: Option<Instant>,
    _blink: Option<Task<()>>,
}

impl Focusable for TextView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextView {
    pub fn new(mode: Mode, model: Entity<DocModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |this, _, cx| {
            this.refresh_index(cx);
            cx.notify();
        })
        .detach();
        let mut v = TextView {
            mode,
            model,
            partner: None,
            drawer_side: false,
            field: String::new(),
            field_version: 0,
            focus: cx.focus_handle(),
            sel: 0..0,
            reversed: false,
            marked: None,
            goal_x: None,
            index: Rc::new(TextIndex::new(String::new())),
            index_version: 0,
            styles: Rc::new(Vec::new()),
            gaps: Rc::new(Vec::new()),
            groups: Rc::new(Vec::new()),
            ghosts: Rc::new(Vec::new()),
            marks: Rc::new(Vec::new()),
            hovered_mark: None,
            comments: Rc::new(Vec::new()),
            hovered_comment: None,
            editing_comment: None,
            comment_ticks: false,
            comment_rects: Rc::new(RefCell::new(Vec::new())),
            comment_rects_scroll: Cell::new(px(0.)),
            layout: None,
            scroll_y: px(0.),
            autoscroll: false,
            glide: None,
            hovered_group: None,
            wheel: None,
            hover_char: None,
            hover_in_bounds: false,
            over_dots: false,
            chip_group: None,
            chip_left: None,
            chip_leaves: 0,
            _chip_timer: None,
            chip_rect: Rc::new(Cell::new(None)),
            over_chip: false,
            sel_buttons: None,
            drag: None,
            press_in_sel: None,
            over_sel: false,
            carrying: None,
            drop_mark: None,
            last_typing: None,
            caret_on: true,
            blink_epoch: Instant::now(),
            caret_slide: None,
            caret_jump: false,
            wash: None,
            focus_range: None,
            spotlight: None,
            centre_request: None,
            zen: None,
            zen_prev: None,
            zen_since: None,
            highlights: Rc::new(Vec::new()),
            misspelled: Default::default(),
            lab: Default::default(),
            spell_cache: HashMap::new(),
            spell_generation: 0,
            current_highlight: None,
            raised_ghost: None,
            flash: None,
            fades: Vec::new(),
            placeholder: SharedString::default(),
            hint_after: SharedString::default(),
            quiet_when_empty: false,
            a11y_value: SharedString::default(),
            cache: RefCell::new(HashMap::new()),
            text_lang: None,
            style_cache: HashMap::new(),
            grow_steps: Vec::new(),
            grown: None,
            shift_tap: None,
            _blink: None,
        };
        v.refresh_index(cx);
        v.start_blink(cx);
        v
    }

    fn start_blink(&mut self, cx: &mut Context<Self>) {
        // Scripted runs keep the caret steady, so screenshots compare.
        if cfg!(feature = "shots") && std::env::var_os("DOSTOEVSKY_SCRIPT").is_some() {
            return;
        }
        self._blink = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(530))
                    .await;
                let alive = this.update(cx, |v, cx| {
                    if v.blink_epoch.elapsed() > Duration::from_millis(500) {
                        v.caret_on = !v.caret_on;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    fn slide_caret(&mut self, layout: &Layout, cx: &App) {
        let Some(p) = layout.point_for(self.caret()) else {
            self.caret_slide = None;
            return;
        };
        let to = p - layout.origin;
        let width = layout.column_width;
        let jump = std::mem::take(&mut self.caret_jump);
        let prev = self.caret_slide.filter(|s| s.column_width == width);
        if prev.is_some_and(|s| s.to == to) && !jump {
            return;
        }
        let now = Instant::now();
        let from = prev.map_or(to, |s| s.at(now));
        let (dx, dy) = ((to.x - from.x).abs(), (to.y - from.y).abs());
        let near = dy < layout.line_height * 0.5 || (dy < layout.line_height * 1.5 && dx < layout.font_size * 2.);
        let smooth = !jump
            && near
            && Prefs::get(cx).motion() != crate::prefs::Effects::Off
            // Scripted runs don't slide the caret, so screenshots compare.
            && !(cfg!(feature = "shots") && std::env::var_os("DOSTOEVSKY_SCRIPT").is_some());
        self.caret_slide = Some(CaretSlide { from: if smooth { from } else { to }, to, since: now, column_width: width });
    }

    #[cfg(test)]
    pub fn caret_slide(&self) -> Option<(Point<Pixels>, Point<Pixels>)> {
        let origin = self.layout.as_ref()?.origin;
        self.caret_slide.map(|s| (origin + s.from, origin + s.to))
    }

    fn pause_blink(&mut self) {
        self.caret_on = true;
        self.blink_epoch = Instant::now();
    }

    fn current_version(&self, cx: &App) -> u64 {
        match self.mode {
            Mode::Field => self.field_version,
            _ => self.model.read(cx).version,
        }
    }

    fn refresh_index(&mut self, cx: &mut Context<Self>) {
        let v = self.current_version(cx);
        if v == self.index_version && self.index_version != 0 {
            return;
        }
        let old = self.index.clone();
        let text = match self.mode {
            Mode::Page => self.model.read(cx).doc.text(),
            Mode::Cut(id) => self.model.read(cx).doc.cut_info(id).map(|c| c.text).unwrap_or_default(),
            Mode::Comment(id) => self.model.read(cx).doc.comment_text(id).unwrap_or_default(),
            Mode::Field => self.field.clone(),
        };
        if text != old.text || self.index_version == 0 {
            let new = TextIndex::new(text);
            let map = |c: usize| map_through_diff(&old, &new, c);
            self.sel = map(self.sel.start)..map(self.sel.end);
            self.marked = self.marked.take().map(|m| map(m.start)..map(m.end)).filter(|m| !m.is_empty());
            self.a11y_value = new.text.clone().into();
            self.index = Rc::new(new);
            self.styles = Rc::new(match self.mode {
                Mode::Page => markdown::style_lines_cached(
                    (0..self.index.lines.len()).map(|i| self.index.line_str(i)),
                    &mut self.style_cache,
                ),
                _ => Vec::new(),
            });
            self.gaps = Rc::new(if self.mode == Mode::Page { paragraph_gaps(&self.index, &self.styles) } else { Vec::new() });
            if self.sel.is_empty() {
                let c = self.off_gap(self.sel.start, true);
                self.sel = c..c;
            }
        }
        if self.mode == Mode::Page {
            let m = self.model.read(cx);
            self.groups = Rc::new(m.doc.visible_groups());
            self.ghosts = Rc::new(m.doc.ghost_ranges());
            // Marks show in writing as in editing.
            let marks = m.doc.traces().into_iter().filter(|t| !t.shown).map(|t| (t.id, t.range.start, t.paragraph)).collect();
            self.marks = Rc::new(marks);
            self.comments = Rc::new(if m.layer { m.doc.comments() } else { Vec::new() });
        }
        self.index_version = v;
    }

    pub fn index(&self) -> &Rc<TextIndex> {
        &self.index
    }

    #[cfg(test)]
    pub fn text(&self) -> &str {
        &self.index.text
    }


    pub fn selection(&self) -> Range<usize> {
        self.sel.clone()
    }

    pub fn caret(&self) -> usize {
        if self.reversed { self.sel.start } else { self.sel.end }
    }

    pub fn groups(&self) -> &[VisibleGroup] {
        &self.groups
    }

    pub fn set_field_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field = text.to_string();
        self.field_version += 1;
        self.refresh_index(cx);
        let n = self.index.len();
        self.sel = n..n;
        cx.notify();
    }

    pub fn field_text(&self) -> &str {
        &self.field
    }

    fn in_gap(&self, c: usize) -> bool {
        if c == 0 || c >= self.index.len() {
            return false;
        }
        let l = self.index.line_of(c);
        self.gaps.get(l).copied().unwrap_or(false) && self.index.lines[l].char_start == c
    }

    fn off_gap(&self, c: usize, forward: bool) -> usize {
        match (self.in_gap(c), forward) {
            (false, _) => c,
            (true, true) => c + 1,
            (true, false) => c - 1,
        }
    }

    pub fn select(&mut self, r: Range<usize>, cx: &mut Context<Self>) {
        let n = self.index.len();
        let r = r.start.min(n)..r.end.min(n);
        let r = if r.is_empty() { let c = self.off_gap(r.start, true); c..c } else { r };
        if r.start <= r.end {
            self.sel = r;
            self.reversed = false;
        } else {
            self.sel = r.end..r.start;
            self.reversed = true;
        }
        self.after_move(cx);
    }

    fn move_to(&mut self, c: usize, cx: &mut Context<Self>) {
        let c = c.min(self.index.len());
        let c = self.off_gap(c, c > self.caret());
        self.sel = c..c;
        self.reversed = false;
        self.after_move(cx);
    }

    fn select_to(&mut self, c: usize, cx: &mut Context<Self>) {
        let c = c.min(self.index.len());
        let c = self.off_gap(c, c > self.caret());
        let anchor = if self.reversed { self.sel.end } else { self.sel.start };
        if c < anchor {
            self.sel = c..anchor;
            self.reversed = true;
        } else {
            self.sel = anchor..c;
            self.reversed = false;
        }
        self.after_move(cx);
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.shift_tap = None;
        // Writing a comment's note is one undo step however the caret moves.
        if !matches!(self.mode, Mode::Field | Mode::Comment(_)) {
            self.model.update(cx, |m, _| m.doc.break_undo_group());
        }
        self.last_typing = None;
        self.autoscroll = true;
        self.pause_blink();
        cx.emit(ViewEvent::SelectionChanged);
        cx.notify();
    }

    pub fn replace(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) -> bool {
        if self.last_typing.is_some_and(|t| t.elapsed() > Duration::from_millis(1500)) {
            self.model.update(cx, |m, _| m.doc.break_undo_group());
        }
        let t0 = Instant::now();
        let ok = match self.mode {
            Mode::Page => self.model.update(cx, |m, cx| m.replace(cx, range.clone(), text)),
            Mode::Cut(id) => self.model.update(cx, |m, cx| m.edit(cx, |d| d.replace_in_cut(id, range.clone(), text)).is_ok()),
            Mode::Comment(id) => self.model.update(cx, |m, cx| m.edit(cx, |d| d.replace_in_comment(id, range.clone(), text)).is_ok()),
            Mode::Field => {
                let (a, b) = (self.index.char_to_byte(range.start), self.index.char_to_byte(range.end));
                self.field.replace_range(a..b, text);
                self.field_version += 1;
                true
            }
        };
        if !ok {
            return false;
        }
        // No diff remapping: we know where the caret goes.
        self.index_version = 0;
        let caret = range.start + text.chars().count();
        self.refresh_index(cx);
        let caret = self.off_gap(caret, true);
        if std::env::var_os("DOSTOEVSKY_PERF").is_some() {
            eprintln!("perf: replace+index {:?}", t0.elapsed());
        }
        self.sel = caret..caret;
        self.reversed = false;
        self.last_typing = Some(Instant::now());
        self.shift_tap = None;
        self.autoscroll = true;
        self.goal_x = None;
        self.pause_blink();
        cx.emit(ViewEvent::Edited);
        cx.notify();
        true
    }

    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        // Control characters a key can send (⇧⇥'s U+0019) are not text; a pasted tab is.
        let mut text: String = text.chars().filter(|&c| !c.is_control() || c == '\n' || c == '\t').collect();
        if text.is_empty() {
            return;
        }
        let range = self.marked.take().unwrap_or(self.sel.clone());
        if self.mode == Mode::Field {
            text = text.replace('\n', " ");
        }
        let mut range = range;
        if self.mode == Mode::Page && Prefs::get(cx).smart_punctuation && text.chars().count() == 1 {
            let before = range.start.checked_sub(1).and_then(|i| self.index.char_at(i));
            let before2 = range.start.checked_sub(2).and_then(|i| self.index.char_at(i));
            match text.as_str() {
                "\"" | "'" => {
                    let opening = before.is_none_or(|b| b.is_whitespace() || "([{“‘—-".contains(b));
                    text = match (text.as_str(), opening) {
                        ("\"", true) => "“",
                        ("\"", false) => "”",
                        ("'", true) => "‘",
                        _ => "’",
                    }
                    .into();
                }
                "-" if before == Some('-')
                    && before2.is_some_and(|b| b.is_alphanumeric() || b == ' ')
                    && range.is_empty() =>
                {
                    range = range.start - 1..range.end;
                    text = "—".into();
                }
                _ => {}
            }
        }
        self.replace(range, &text, cx);
    }

    fn delete_range(&mut self, r: Range<usize>, cx: &mut Context<Self>) {
        if !r.is_empty() {
            self.replace(r, "", cx);
        }
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.history(false, cx);
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.history(true, cx);
    }

    fn history(&mut self, redo: bool, cx: &mut Context<Self>) {
        if self.mode == Mode::Field {
            cx.emit(ViewEvent::History { redo });
            return;
        }
        if self.mode == Mode::Page {
            let d = &self.model.read(cx).doc;
            let tag = if redo { d.redo_tag() } else { d.undo_tag() };
            if let Some(link) = tag.and_then(|t| t.link).filter(|l| crate::cuts::is_cross(*l, cx)) {
                cx.emit(ViewEvent::LinkedHistory { link, redo });
                self.select_last_change(cx);
                return;
            }
        }
        if let Some(partner) = self.partner.clone() {
            let (page, drawer) = match self.mode {
                Mode::Cut(_) if self.drawer_side => (partner, self.model.clone()),
                _ => (self.model.clone(), partner),
            };
            if let crate::model::Joint::Elsewhere(link) = crate::model::joint_history(&page, &drawer, redo, cx) {
                cx.emit(ViewEvent::HistoryElsewhere(link));
            }
        } else if redo {
            self.model.update(cx, |m, cx| m.redo(cx));
        } else {
            self.model.update(cx, |m, cx| m.undo(cx));
        }
        self.select_last_change(cx);
    }

    fn select_last_change(&mut self, cx: &mut Context<Self>) {
        let old = self.index.clone();
        self.refresh_index(cx);
        let (p, _, new_end) = diff_span(&old.text, &self.index.text);
        let _ = p;
        let c = self.off_gap(self.index.byte_to_char(new_end), true);
        self.sel = c..c;
        self.reversed = false;
        self.autoscroll = true;
        cx.emit(ViewEvent::Edited);
        cx.notify();
    }

    /// Innermost visible group containing `c`, paragraph groups included.
    pub fn group_at(&self, c: usize) -> Option<&VisibleGroup> {
        self.groups
            .iter()
            .filter(|g| g.range.start <= c && c <= g.range.end && !g.range.is_empty())
            .max_by_key(|g| g.depth)
    }

    fn layer_on(&self, cx: &App) -> bool {
        self.mode == Mode::Page && self.model.read(cx).layer
    }

    /// The source lines that stay bright, or `None` when nothing is dimmed.
    pub fn zen_lines(&self, cx: &App) -> Option<Range<usize>> {
        if self.mode != Mode::Page || !Prefs::get(cx).zen_writing || self.layer_on(cx) || self.focus_range.is_some() {
            return None;
        }
        let idx = &self.index;
        let first = idx.line_of(self.sel.start);
        let mut last = idx.line_of(self.sel.end);
        // A selection ending at a line's start (a whole paragraph) doesn't reach into it.
        if last > first && idx.lines[last].char_start == self.sel.end {
            last -= 1;
        }
        // A paragraph broken over several source lines is still one paragraph.
        let para = |l: usize| self.styles.get(l).is_some_and(|s| s.kind == BlockKind::Paragraph);
        let (mut a, mut b) = (first, last);
        while a > 0 && para(a) && para(a - 1) {
            a -= 1;
        }
        while b + 1 < idx.lines.len() && para(b) && para(b + 1) {
            b += 1;
        }
        Some(a..b + 1)
    }

    pub fn typewriter(&self, cx: &App) -> bool {
        self.mode == Mode::Page && Prefs::get(cx).typewriter && !self.layer_on(cx) && self.focus_range.is_none()
    }

    pub fn gliding(&self) -> bool {
        self.glide.is_some()
    }

    pub fn zen_fading(&self) -> bool {
        self.zen_since.is_some()
    }

    pub fn cycle(&mut self, id: GroupId, delta: isize, cx: &mut Context<Self>) {
        let r = self.model.update(cx, |m, cx| m.cycle(cx, id, delta));
        if let Some(r) = r {
            self.after_cycle(id, r.is_original, cx);
        }
    }

    pub fn after_cycle(&mut self, id: GroupId, is_original: bool, cx: &mut Context<Self>) {
        // A selection of the whole group follows it; one inside collapses to its end; others shift.
        let old = self.groups.iter().find(|g| g.id == id).map(|g| g.range.clone());
        let sel = self.sel.clone();
        self.refresh_index(cx);
        let new = self.groups.iter().find(|g| g.id == id).map(|g| g.range.clone());
        if let (Some(o), Some(n)) = (old, new) {
            if sel == o && !sel.is_empty() {
                self.sel = n;
            } else if o.start <= sel.start && sel.end <= o.end {
                self.sel = n.end..n.end;
            }
        }
        self.wash = Some((id, Instant::now()));
        cx.emit(ViewEvent::Cycled { group: id, is_original });
        cx.notify();
    }

    fn cycle_hovered(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.hovered_group {
            Some(id) if self.layer_on(cx) => self.cycle(id, delta, cx),
            _ => {
                // The pointer left the group between keymap dispatch and now.
                if delta > 0 {
                    self.move_down(&MoveDown, window, cx)
                } else {
                    self.move_up(&MoveUp, window, cx)
                }
            }
        }
    }

    fn update_spelling(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let on = self.mode == Mode::Page
            && match Prefs::get(cx).spell_check {
                crate::prefs::SpellCheck::Always => true,
                crate::prefs::SpellCheck::Editing => self.layer_on(cx),
                crate::prefs::SpellCheck::Off => false,
            };
        let Some(layout) = self.layout.clone().filter(|_| on) else {
            if !self.misspelled.is_empty() {
                self.misspelled = Default::default();
            }
            return;
        };
        let generation = crate::spell::Spelling::generation(cx);
        if generation != self.spell_generation {
            self.spell_cache.clear();
            self.spell_generation = generation;
        }
        if self.spell_cache.len() > 4096 {
            self.spell_cache.clear();
        }
        let caret = self.caret();
        let typing = self.last_typing.is_some();
        let mut out = Vec::new();
        for b in &layout.lines {
            let top = layout.origin.y + b.y;
            if top + b.height < bounds.top() || top > bounds.bottom() {
                continue;
            }
            let text = self.index.line_str(b.line);
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                text.hash(&mut h);
                h.finish()
            };
            let words = match self.spell_cache.get(&key) {
                Some(w) => w.clone(),
                None => {
                    let w = Rc::new(crate::spell::Spelling::misspelled(cx, text));
                    self.spell_cache.insert(key, w.clone());
                    w
                }
            };
            let start = self.index.lines[b.line].char_start;
            for r in words.iter() {
                let r = start + r.start..start + r.end;
                if typing && r.start <= caret && caret <= r.end {
                    continue;
                }
                out.push(r);
            }
        }
        if *self.misspelled != out {
            self.misspelled = Rc::new(out);
        }
    }

    /// The label under a hovered group's dots: `version 3 of 5 ›` or `original · 1 of 5 ›`.
    pub fn hover_chip(&self, cx: &App) -> Option<(VisibleGroup, String)> {
        let id = self.chip_group?;
        if !self.layer_on(cx) || !Prefs::get(cx).hover_label || self.drag.is_some() || self.spotlight == Some(id) {
            return None;
        }
        let g = self.groups.iter().find(|g| g.id == id)?.clone();
        let label = if g.active == 0 {
            tr!("original · 1 of {total} ›", total = g.variant_count)
        } else {
            tr!("version {n} of {total} ›", n = g.active + 1, total = g.variant_count)
        };
        Some((g, label))
    }

    pub fn chip_bounds(&self, cx: &App) -> Option<Bounds<Pixels>> {
        self.hover_chip(cx)?;
        self.chip_rect.get()
    }

    fn chip_area(&self, id: GroupId, cx: &App) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let g = self.groups.iter().find(|g| g.id == id)?;
        let dots = dots_bounds(layout, g)?;
        Some(match self.chip_bounds(cx) {
            Some(chip) => dots.union(&chip),
            None => dots,
        })
    }

    /// The label stays 300 ms after the pointer leaves, so it can be reached; no other group takes it.
    fn update_chip(&mut self, hovered: Option<GroupId>, p: Point<Pixels>, cx: &mut Context<Self>) {
        let over_chip = self.chip_bounds(cx).is_some_and(|b| b.contains(&p));
        if over_chip != self.over_chip {
            self.over_chip = over_chip;
            cx.notify();
        }
        if let Some(id) = self.chip_group {
            let inside = hovered == Some(id) || self.chip_area(id, cx).is_some_and(|b| b.contains(&p));
            if inside {
                if self.chip_left.take().is_some() {
                    self._chip_timer = None;
                }
            } else if self.chip_left.is_none() {
                self.chip_leaves += 1;
                let token = self.chip_leaves;
                self.chip_left = Some(token);
                self._chip_timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_millis(300)).await;
                    let _ = this.update(cx, |v, cx| {
                        if v.chip_left == Some(token) {
                            v.chip_left = None;
                            v.chip_group = v.hovered_group;
                            v.over_chip = false;
                            cx.notify();
                        }
                    });
                }));
            }
            return;
        }
        if hovered.is_some() {
            self.chip_group = hovered;
            cx.notify();
        }
    }

    pub fn selection_buttons(&self, cx: &App) -> Option<Range<usize>> {
        let r = self.sel_buttons.clone()?;
        let show = r == self.sel
            && !r.is_empty()
            && self.layer_on(cx)
            && Prefs::get(cx).selection_buttons
            && self.drag.is_none()
            && self.press_in_sel.is_none()
            && self.carrying.is_none()
            && self.marked.is_none();
        show.then_some(r)
    }

    pub fn hide_selection_buttons(&mut self, cx: &mut Context<Self>) {
        if self.sel_buttons.take().is_some() {
            cx.notify();
        }
    }

    pub fn dots_at(&self, p: Point<Pixels>, cx: &App) -> Option<GroupId> {
        if !self.layer_on(cx) {
            return None;
        }
        let layout = self.layout.as_ref()?;
        // Painted last is on top.
        self.groups.iter().rev().find(|g| dots_bounds(layout, g).is_some_and(|b| b.contains(&p))).map(|g| g.id)
    }

    pub fn caret_group(&self) -> Option<GroupId> {
        let c = self.caret();
        self.group_at(c).map(|g| g.id)
    }

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.sel.is_empty() {
            let c = self.index.prev_grapheme(self.caret());
            self.move_to(c, cx);
        } else {
            self.move_to(self.sel.start, cx);
        }
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.sel.is_empty() {
            let c = self.index.next_grapheme(self.caret());
            self.move_to(c, cx);
        } else {
            self.move_to(self.sel.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        let c = self.index.prev_grapheme(self.caret());
        self.select_to(c, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        let c = self.index.next_grapheme(self.caret());
        self.select_to(c, cx);
    }

    fn vertical(&mut self, rows: i32) -> Option<usize> {
        let layout = self.layout.clone()?;
        let p = layout.point_for(self.caret())?;
        let x = *self.goal_x.get_or_insert(p.x);
        let mut y = p.y + layout.line_height * (rows as f32 + 0.5);
        // A paragraph break's gap is not a row: step over it.
        if let Some((_, h)) = layout.gap_at(y) {
            y += if rows > 0 { h } else { -h };
        }
        if y < layout.origin.y {
            return Some(0);
        }
        if y > layout.origin.y + layout.total_height {
            return Some(self.index.len());
        }
        Some(layout.index_for(point(x, y)))
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Field {
            cx.emit(ViewEvent::Arrow(-1));
            return;
        }
        if !self.sel.is_empty() {
            return self.move_to(self.sel.start, cx);
        }
        if let Some(c) = self.vertical(-1) {
            let g = self.goal_x;
            self.move_to(c, cx);
            self.goal_x = g;
        }
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Field {
            cx.emit(ViewEvent::Arrow(1));
            return;
        }
        if !self.sel.is_empty() {
            return self.move_to(self.sel.end, cx);
        }
        if let Some(c) = self.vertical(1) {
            let g = self.goal_x;
            self.move_to(c, cx);
            self.goal_x = g;
        }
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.vertical(-1) {
            let g = self.goal_x;
            self.select_to(c, cx);
            self.goal_x = g;
        }
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.vertical(1) {
            let g = self.goal_x;
            self.select_to(c, cx);
            self.goal_x = g;
        }
    }

    fn page_rows(&self) -> i32 {
        self.layout
            .as_ref()
            .map_or(20, |l| ((l.bounds.size.height / l.line_height) as i32 - 2).max(1))
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.vertical(-self.page_rows()) {
            self.move_to(c, cx);
        }
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.vertical(self.page_rows()) {
            self.move_to(c, cx);
        }
    }

    fn select_page_up(&mut self, _: &SelectPageUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.vertical(-self.page_rows()) {
            self.select_to(c, cx);
        }
    }

    fn select_page_down(&mut self, _: &SelectPageDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.vertical(self.page_rows()) {
            self.select_to(c, cx);
        }
    }

    fn paragraph_start_before(&self, c: usize) -> usize {
        let idx = &self.index;
        let mut l = idx.line_of(c);
        if c == idx.lines[l].char_start {
            while l > 0 {
                l -= 1;
                if idx.lines[l].char_len > 0 {
                    break;
                }
            }
        }
        idx.lines[l].char_start
    }

    fn paragraph_end_after(&self, c: usize) -> usize {
        let idx = &self.index;
        let mut l = idx.line_of(c);
        if c == idx.line_end(l) {
            while l + 1 < idx.lines.len() {
                l += 1;
                if idx.lines[l].char_len > 0 {
                    break;
                }
            }
        }
        idx.line_end(l)
    }

    fn move_paragraph_start(&mut self, _: &MoveParagraphStart, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.paragraph_start_before(self.caret());
        self.move_to(c, cx);
    }

    fn move_paragraph_end(&mut self, _: &MoveParagraphEnd, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.paragraph_end_after(self.caret());
        self.move_to(c, cx);
    }

    fn select_paragraph_start(&mut self, _: &SelectParagraphStart, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.paragraph_start_before(self.caret());
        self.select_to(c, cx);
    }

    fn select_paragraph_end(&mut self, _: &SelectParagraphEnd, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.paragraph_end_after(self.caret());
        self.select_to(c, cx);
    }

    fn move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.index.prev_word(self.caret());
        self.move_to(c, cx);
    }

    fn move_word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.index.next_word(self.caret());
        self.move_to(c, cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.index.prev_word(self.caret());
        self.select_to(c, cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.index.next_word(self.caret());
        self.select_to(c, cx);
    }

    fn prev_sentence(&mut self, _: &PrevSentence, _: &mut Window, cx: &mut Context<Self>) {
        let c = sentence_keys::prev_sentence(&self.index, self.sel.start, &self.ghosts);
        self.move_to(c, cx);
    }

    fn next_sentence(&mut self, _: &NextSentence, _: &mut Window, cx: &mut Context<Self>) {
        let c = sentence_keys::next_sentence(&self.index, self.sel.end, &self.ghosts);
        self.move_to(c, cx);
    }

    fn select_prev_sentence(&mut self, _: &SelectPrevSentence, _: &mut Window, cx: &mut Context<Self>) {
        let c = sentence_keys::prev_sentence(&self.index, self.caret(), &self.ghosts);
        self.select_to(c, cx);
    }

    fn select_next_sentence(&mut self, _: &SelectNextSentence, _: &mut Window, cx: &mut Context<Self>) {
        let c = sentence_keys::next_sentence(&self.index, self.caret(), &self.ghosts);
        self.select_to(c, cx);
    }

    fn unit_groups(&self, cx: &App) -> Vec<Range<usize>> {
        if self.mode == Mode::Page && !self.model.read(cx).layer {
            return Vec::new();
        }
        self.groups.iter().map(|g| g.range.clone()).collect()
    }

    pub fn grow_selection(&mut self, cx: &mut Context<Self>) {
        if self.grown.as_ref() != Some(&self.sel) {
            self.grow_steps.clear();
        }
        let groups = self.unit_groups(cx);
        let Some(next) = sentence_keys::grow(&self.index, &groups, &self.sel) else { return };
        let before = if self.sel.is_empty() { self.caret()..self.caret() } else { self.sel.clone() };
        self.select(next.clone(), cx);
        self.grow_steps.push(before);
        self.grown = Some(next);
    }

    fn grow_action(&mut self, _: &GrowSelection, _: &mut Window, cx: &mut Context<Self>) {
        self.grow_selection(cx);
    }

    fn shrink_action(&mut self, _: &ShrinkSelection, _: &mut Window, cx: &mut Context<Self>) {
        if self.grown.as_ref() != Some(&self.sel) {
            self.grow_steps.clear();
            self.grown = None;
            return;
        }
        let Some(prev) = self.grow_steps.pop() else { return };
        if prev.is_empty() {
            self.move_to(prev.start, cx);
        } else {
            self.select(prev.clone(), cx);
        }
        self.grown = if self.grow_steps.is_empty() { None } else { Some(prev) };
    }

    /// Tap ⇧ to select: ⇧ pressed and let go alone within 300 ms grows the selection.
    fn on_modifiers(&mut self, e: &ModifiersChangedEvent, _: &mut Window, cx: &mut Context<Self>) {
        let m = e.modifiers;
        let only_shift = m.shift && !m.control && !m.alt && !m.platform && !m.function;
        let none = !m.shift && !m.control && !m.alt && !m.platform && !m.function;
        if only_shift {
            if self.shift_tap.is_none() {
                self.shift_tap = Some(Instant::now());
            }
            return;
        }
        let tap = self.shift_tap.take();
        if none
            && tap.is_some_and(|t| t.elapsed() < Duration::from_millis(300))
            && self.mode == Mode::Page
            && Prefs::get(cx).tap_shift
        {
            self.grow_selection(cx);
        }
    }

    fn row_bounds(&self) -> Range<usize> {
        let c = self.caret();
        let line = self.index.line_of(c);
        let fallback = self.index.line_range(line);
        let Some(layout) = &self.layout else { return fallback };
        let Some(b) = layout.line_box(line) else { return fallback };
        let byte = self.index.byte_in_line(line, c);
        let r = Layout::row_of(&b.shaped, byte);
        let row = b.shaped.rows[r];
        let s = self.index.line_str(line);
        let start = self.index.lines[line].char_start;
        let mut end_b = row.end;
        if r + 1 < b.shaped.rows.len() {
            // Stop before the space the row wrapped at.
            end_b = s[..row.end].char_indices().next_back().map_or(0, |(i, _)| i);
        }
        start + s[..row.start].chars().count()..start + s[..end_b].chars().count()
    }

    fn move_line_start(&mut self, _: &MoveLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.row_bounds();
        self.move_to(r.start, cx);
    }

    fn move_line_end(&mut self, _: &MoveLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.row_bounds();
        self.move_to(r.end, cx);
    }

    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.row_bounds();
        self.select_to(r.start, cx);
    }

    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.row_bounds();
        self.select_to(r.end, cx);
    }

    fn move_doc_start(&mut self, _: &MoveDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn move_doc_end(&mut self, _: &MoveDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.index.len(), cx);
    }

    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.index.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let c = self.caret();
            if c == 0 {
                window.play_system_bell();
                return;
            }
            // Joining two paragraphs takes the blank line between them too.
            let p = self.paragraph_join_at(c).unwrap_or_else(|| self.index.prev_grapheme(c));
            self.delete_range(p..c, cx);
        } else {
            self.delete_range(self.sel.clone(), cx);
        }
    }

    /// Where ⌫ at `c` deletes from to join its paragraph to the one before (an empty one too).
    fn paragraph_join_at(&self, c: usize) -> Option<usize> {
        if self.mode != Mode::Page {
            return None;
        }
        let idx = &self.index;
        let l = idx.line_of(c);
        let kind = |l: usize| self.styles.get(l).map(|s| s.kind);
        let empty = |l: usize| idx.lines[l].char_len == 0 && kind(l) == Some(BlockKind::Blank);
        let joins = |l: usize| matches!(kind(l), Some(BlockKind::Paragraph | BlockKind::Heading(_))) || (empty(l) && self.gaps.get(l + 1) == Some(&true));
        (l >= 2
            && idx.lines[l].char_start == c
            && idx.lines[l - 1].char_len == 0
            && joins(l - 2)
            && (idx.lines[l].char_len == 0 || kind(l) == Some(BlockKind::Paragraph)))
            .then(|| idx.lines[l - 1].char_start - 1)
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let c = self.caret();
            // At a paragraph's end ⌦ joins the next one to it, as ⌫ does from its start.
            let n = if self.in_gap(c + 1) && self.paragraph_join_at(c + 2).is_some() { c + 2 } else { self.index.next_grapheme(c) };
            self.delete_range(c..n, cx);
        } else {
            self.delete_range(self.sel.clone(), cx);
        }
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let c = self.caret();
            self.delete_range(self.index.prev_word(c)..c, cx);
        } else {
            self.delete_range(self.sel.clone(), cx);
        }
    }

    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let c = self.caret();
            self.delete_range(c..self.index.next_word(c), cx);
        } else {
            self.delete_range(self.sel.clone(), cx);
        }
    }

    fn delete_line_left(&mut self, _: &DeleteLineLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Field && self.index.len() == 0 {
            cx.emit(ViewEvent::DeleteLineEmpty);
            return;
        }
        if self.sel.is_empty() {
            let c = self.caret();
            let s = self.row_bounds().start;
            self.delete_range(s..c, cx);
        } else {
            self.delete_range(self.sel.clone(), cx);
        }
    }

    fn delete_line_right(&mut self, _: &DeleteLineRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let c = self.caret();
            let e = self.row_bounds().end;
            self.delete_range(c..e, cx);
        } else {
            self.delete_range(self.sel.clone(), cx);
        }
    }

    /// At the paragraph's end, deletes the line break (as Cocoa does).
    fn delete_to_paragraph_end(&mut self, _: &DeleteToParagraphEnd, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.caret();
        let e = self.index.line_end(self.index.line_of(c));
        let e = if e == c { (c + 1).min(self.index.len()) } else { e };
        self.delete_range(c..e, cx);
    }

    /// At a line's end, swaps the last two characters.
    fn transpose(&mut self, _: &Transpose, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            return;
        }
        let idx = &self.index;
        let c = self.caret();
        let l = idx.line_of(c);
        let start = idx.lines[l].char_start;
        let c = if c == idx.line_end(l) && c > start { idx.prev_grapheme(c) } else { c };
        if c <= start {
            window.play_system_bell();
            return;
        }
        let a = idx.prev_grapheme(c);
        let b = idx.next_grapheme(c);
        let swapped = format!("{}{}", idx.slice(c..b), idx.slice(a..c));
        self.replace(a..b, &swapped, cx);
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.mode, Mode::Field | Mode::Comment(_)) {
            cx.emit(ViewEvent::Confirm);
            return;
        }
        // A paragraph break is a blank line, as Markdown needs; at a line's start a newline is enough.
        let mut text = "\n".to_string();
        let line = self.index.line_of(self.sel.start);
        if self.mode == Mode::Page && self.sel.is_empty() && self.sel.start == self.index.lines[line].char_start {
            // Markdown and every export drop a second empty paragraph: hint instead.
            let blank = |l: usize| self.index.lines[l].char_len == 0 && self.styles.get(l).is_some_and(|s| s.kind == BlockKind::Blank);
            let kind = self.styles.get(line).map(|s| s.kind);
            if (blank(line) && line >= 1 && blank(line - 1)) || (!blank(line) && line >= 2 && blank(line - 1) && blank(line - 2)) {
                cx.emit(ViewEvent::SceneBreakHint);
                return;
            }
            if kind == Some(BlockKind::Paragraph) && line >= 1 && self.gaps.get(line - 1) == Some(&true) {
                self.insert("\n\n", cx);
                return;
            }
        }
        if self.mode != Mode::Page || self.sel.start == self.index.lines[line].char_start {
            self.insert(&text, cx);
            return;
        }
        let kind = self.styles.get(line).map(|s| s.kind);
        if matches!(kind, Some(BlockKind::Paragraph | BlockKind::Heading(_))) {
            // At a line's end before a paragraph line, the ⇧Return break becomes half the blank line.
            let c = self.sel.start;
            let next = line + 1;
            if self.sel.is_empty()
                && c == self.index.line_end(line)
                && next < self.index.lines.len()
                && self.index.lines[next].char_len > 0
                && self.styles.get(next).map(|s| s.kind) == Some(BlockKind::Paragraph)
            {
                self.insert("\n", cx);
                self.move_to(c + 2, cx);
                return;
            }
            text.push('\n');
        }
        if self.sel.is_empty() {
            let s = self.index.line_str(line);
            let style = markdown::style_line(s);
            if matches!(style.kind, BlockKind::List | BlockKind::Quote) {
                let marker_end = style
                    .spans
                    .first()
                    .filter(|(_, k)| *k == Span::Marker)
                    .map_or(0, |(r, _)| r.end);
                let prefix = &s[..marker_end];
                if s.len() == marker_end {
                    let start = self.index.lines[line].char_start;
                    self.replace(start..self.caret(), "", cx);
                    return;
                }
                let next = increment_ordered(prefix);
                text.push_str(&next);
            }
        }
        self.insert(&text, cx);
    }

    fn line_break(&mut self, _: &LineBreak, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Field {
            cx.emit(ViewEvent::Confirm);
            return;
        }
        self.insert("\n", cx);
    }

    /// Inserts nothing on the page: a tab is drawn as a space and splits the word for spelling.
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Field {
            cx.emit(ViewEvent::Tab);
        }
    }

    /// Swallowed so ⇧⇥ never reaches the input method as text.
    fn backtab(&mut self, _: &Backtab, _: &mut Window, _: &mut Context<Self>) {}

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ViewEvent::Confirm);
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if cx.has_active_drag() {
            cx.stop_active_drag(window);
            self.carrying = None;
            self.drop_mark = None;
            cx.notify();
            return;
        }
        if self.marked.is_some() {
            self.marked = None;
            cx.notify();
            return;
        }
        if self.selection_buttons(cx).is_some() {
            self.hide_selection_buttons(cx);
            return;
        }
        cx.emit(ViewEvent::Cancel);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.sel = 0..self.index.len();
        self.reversed = false;
        self.after_move(cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            return;
        }
        let text = self.index.slice(self.sel.clone()).to_string();
        crate::clipboard::write(self, text, cx);
    }

    fn cut(&mut self, a: &Cut, w: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            return;
        }
        let _ = a;
        if self.mode == Mode::Page {
            let sel = self.sel.clone();
            match self.model.update(cx, |m, cx| m.edit(cx, |d| d.cut(sel, None))) {
                Ok((id, r)) => {
                    crate::clipboard::write_cut(&self.model, id, cx);
                    self.sel = r.end..r.end;
                    self.reversed = false;
                    self.fade_ghost(r, true, cx);
                    cx.emit(ViewEvent::Cut(id));
                    cx.notify();
                }
                // Already inside a cut: just copy.
                Err(_) => self.copy(&Copy, w, cx),
            }
            return;
        }
        self.copy(&Copy, w, cx);
        self.delete_range(self.sel.clone(), cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        crate::clipboard::paste(self, cx);
    }

    pub fn paste_fragment(&mut self, fragment: &doc_model::Fragment, cx: &mut Context<Self>) -> bool {
        if self.mode != Mode::Page {
            return false;
        }
        let range = self.sel.clone();
        if !range.is_empty() {
            self.replace(range.clone(), "", cx);
        }
        let at = range.start;
        let ok = self
            .model
            .update(cx, |m, cx| m.edit(cx, |d| d.paste(at, fragment)).is_ok());
        if ok {
            self.index_version = 0;
            self.refresh_index(cx);
            let c = at + fragment.plain_text().chars().count();
            self.sel = c..c;
            self.autoscroll = true;
            cx.emit(ViewEvent::Edited);
            cx.notify();
        }
        ok
    }

    pub fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.insert(text, cx);
    }

    fn undo_action(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.undo(cx);
    }

    fn redo_action(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.redo(cx);
    }

    fn wrap_selection(&mut self, marker: &str, cx: &mut Context<Self>) {
        let r = self.sel.clone();
        let m = marker.chars().count();
        if r.is_empty() {
            let s = format!("{marker}{marker}");
            self.replace(r.clone(), &s, cx);
            self.sel = r.start + m..r.start + m;
            return;
        }
        // Toggle off when the selection is already wrapped.
        let before = r.start.checked_sub(m).map(|s| self.index.slice(s..r.start).to_string());
        let after = (r.end + m <= self.index.len()).then(|| self.index.slice(r.end..r.end + m).to_string());
        if before.as_deref() == Some(marker) && after.as_deref() == Some(marker) {
            self.replace(r.end..r.end + m, "", cx);
            self.replace(r.start - m..r.start, "", cx);
            self.sel = r.start - m..r.end - m;
        } else {
            self.replace(r.end..r.end, marker, cx);
            self.replace(r.start..r.start, marker, cx);
            self.sel = r.start + m..r.end + m;
        }
        cx.notify();
    }

    fn bold(&mut self, _: &Bold, _: &mut Window, cx: &mut Context<Self>) {
        self.wrap_selection("**", cx);
    }

    fn italic(&mut self, _: &Italic, _: &mut Window, cx: &mut Context<Self>) {
        self.wrap_selection("*", cx);
    }

    fn show_character_palette(&mut self, _: &ShowCharacterPalette, window: &mut Window, _: &mut Context<Self>) {
        window.show_character_palette();
    }

    fn cycle_next(&mut self, _: &CycleNext, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_hovered(1, window, cx);
    }

    fn cycle_prev(&mut self, _: &CyclePrev, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_hovered(-1, window, cx);
    }

    fn cycle_at_caret_next(&mut self, _: &CycleAtCaretNext, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.caret_group() {
            self.cycle(id, 1, cx);
        }
    }

    fn cycle_at_caret_prev(&mut self, _: &CycleAtCaretPrev, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.caret_group() {
            self.cycle(id, -1, cx);
        }
    }

    fn back_to_original(&mut self, _: &BackToOriginal, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.hovered_group.filter(|_| self.layer_on(cx)).or(self.caret_group());
        if let Some(id) = id {
            let r = self.model.update(cx, |m, cx| m.set_active(cx, id, 0));
            if r.is_some() {
                self.after_cycle(id, true, cx);
            }
        }
    }

    fn next_group(&mut self, _: &NextGroup, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.caret();
        if let Some(g) = self.groups.iter().filter(|g| g.range.start > c).min_by_key(|g| g.range.start) {
            let s = g.range.start;
            self.move_to(s, cx);
        }
    }

    fn prev_group(&mut self, _: &PrevGroup, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.caret();
        let cur = self.group_at(c).map(|g| g.range.start);
        if let Some(g) = self
            .groups
            .iter()
            .filter(|g| g.range.start < c && Some(g.range.start) != cur)
            .max_by_key(|g| g.range.start)
        {
            let s = g.range.start;
            self.move_to(s, cx);
        }
    }

    fn over_selection(&self, layout: &Layout, p: Point<Pixels>, cx: &App) -> bool {
        self.mode == Mode::Page
            && !self.sel.is_empty()
            && self.model.read(cx).layer
            && layout.rects(self.sel.clone(), false).iter().any(|r| r.contains(&p))
    }

    fn text_drag(&self, cx: &App) -> TextDrag {
        let m = self.model.read(cx);
        let text = self.index.slice(self.sel.clone());
        TextDrag {
            range: self.sel.clone(),
            preview: crate::drag::preview(text),
            words: english::word_count(text),
            block: m.doc.is_whole_paragraphs(self.sel.clone()),
            version: m.version,
            grip: Point::default(),
        }
    }

    pub fn drop_place(&self, p: Point<Pixels>, block: bool) -> Option<Place> {
        let layout = self.layout.as_ref()?;
        let idx = &self.index;
        let c = layout.index_for(p);
        let line = idx.line_of(c);
        let blank = |i: usize| idx.line_str(i).trim().is_empty();
        let next_start = |i: usize| (i + 1..idx.lines.len()).find(|k| !blank(*k)).map_or(idx.len(), |k| idx.lines[k].char_start);
        if let Some((gap, _)) = layout.gap_at(p.y) {
            return Some(Place::Block(next_start(gap)));
        }
        if blank(line) {
            return Some(Place::Block(next_start(line)));
        }
        if block {
            let para = idx.paragraph_at(c);
            let top = layout.point_for(para.start)?.y;
            let bottom = layout.point_for(para.end)?.y + layout.line_height;
            if p.y <= (top + bottom) / 2. {
                return Some(Place::Block(para.start));
            }
            return Some(Place::Block(next_start(idx.line_of(para.end))));
        }
        let start = idx.lines[line].char_start;
        let chars: Vec<char> = idx.line_str(line).chars().collect();
        let k = c - start;
        let ws = |i: usize| chars[i].is_whitespace();
        (0..=chars.len())
            .filter(|&i| {
                let token_start = i < chars.len() && !ws(i) && (i == 0 || ws(i - 1));
                let token_end = i > 0 && !ws(i - 1) && (i == chars.len() || ws(i));
                token_start || token_end
            })
            .min_by_key(|&i| i.abs_diff(k))
            .map(|i| Place::Inline(start + i))
    }

    pub fn nudge_scroll(&mut self, dy: Pixels, cx: &mut Context<Self>) {
        self.glide = None;
        self.autoscroll = false;
        self.scroll_y += dy;
        self.clamp_scroll();
        cx.notify();
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(layout) = self.layout.clone() else { return };
        self.sel_buttons = None;
        if e.button == MouseButton::Left {
            if let Some(id) = self.dots_at(e.position, cx) {
                cx.emit(ViewEvent::OpenGroup(id));
                return;
            }
            if let Some(id) = self.chip_group.filter(|_| self.chip_bounds(cx).is_some_and(|b| b.contains(&e.position))) {
                cx.emit(ViewEvent::OpenGroup(id));
                return;
            }
        }
        let on_note = self.comment_rects.borrow().iter().find(|(_, b, tick)| (if *tick { b.dilate(px(5.)) } else { *b }).contains(&e.position)).map(|(id, _, _)| *id);
        if let Some(id) = on_note {
            if e.button == MouseButton::Left {
                cx.emit(ViewEvent::EditComment(id));
            } else {
                cx.emit(ViewEvent::ContextMenu(e.position));
            }
            return;
        }
        if let Some(id) = self.mark_at(e.position) {
            cx.emit(ViewEvent::TraceMenu(id, e.position));
            return;
        }
        let c = layout.index_for(e.position);
        if e.button == MouseButton::Left {
            if let Some((r, _)) = self.lab.cuts.iter().find(|(r, _)| r.start <= c && c < r.end) {
                cx.emit(ViewEvent::KeepCut(r.start));
                return;
            }
        }
        if e.button == MouseButton::Right {
            if !(self.sel.start <= c && c <= self.sel.end && !self.sel.is_empty()) {
                let w = self.index.word_at(c);
                self.select(w, cx);
            }
            cx.emit(ViewEvent::ContextMenu(e.position));
            return;
        }
        if e.click_count == 1 && !e.modifiers.shift && self.over_selection(&layout, e.position, cx) {
            // Maybe a drag: wait to see whether the pointer moves.
            self.press_in_sel = Some(c);
            self.goal_x = None;
            cx.notify();
            return;
        }
        let unit = match e.click_count {
            1 => DragUnit::Char,
            2 => DragUnit::Word,
            _ => DragUnit::Paragraph,
        };
        let in_gap = layout.gap_at(e.position.y).is_some();
        let r = match unit {
            DragUnit::Char => c..c,
            DragUnit::Word if in_gap => c..c,
            DragUnit::Word => self.index.word_at(c),
            DragUnit::Paragraph => self.index.paragraph_at(c),
        };
        if e.modifiers.shift && unit == DragUnit::Char {
            self.select_to(c, cx);
        } else {
            self.select(r.clone(), cx);
        }
        self.goal_x = None;
        self.caret_jump = true;
        self.drag = Some((unit, r));
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.press_in_sel.take() {
            if !cx.has_active_drag() {
                self.move_to(c, cx);
            }
        }
        if self.drag.take().is_some() && !self.sel.is_empty() && self.mode == Mode::Page {
            self.sel_buttons = Some(self.sel.clone());
            cx.notify();
        }
    }

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ViewEvent::PointerMoved);
        if self.press_in_sel.is_some() || cx.has_active_drag() {
            return;
        }
        let Some(layout) = self.layout.clone() else { return };
        if let Some((unit, anchor)) = self.drag.clone() {
            if e.pressed_button == Some(MouseButton::Left) {
                let c = layout.index_for(e.position);
                let r = match unit {
                    DragUnit::Char => c..c,
                    DragUnit::Word => self.index.word_at(c),
                    DragUnit::Paragraph => self.index.paragraph_at(c),
                };
                if r.start < anchor.start {
                    self.sel = r.start..anchor.end;
                    self.reversed = true;
                } else {
                    self.sel = anchor.start..r.end.max(anchor.end);
                    self.reversed = false;
                }
                self.autoscroll = true;
                cx.notify();
                return;
            }
            self.drag = None;
        }
        self.update_hover(e.position, cx);
    }

    pub fn update_hover(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.clone() else { return };
        let inside = layout.bounds.contains(&p);
        let mut hovered = None;
        let mut hover_char = None;
        if inside && self.mode == Mode::Page {
            let c = layout.index_for(p);
            let near = layout
                .point_for(c)
                .is_some_and(|q| (q.x - p.x).abs() < px(14.) && p.y >= q.y - px(2.) && p.y <= q.y + layout.line_height);
            if near {
                hover_char = Some(c);
                hovered = self
                    .groups
                    .iter()
                    .filter(|g| g.scope != Scope::Paragraph && g.range.start <= c && c <= g.range.end)
                    .max_by_key(|g| g.depth)
                    .map(|g| g.id);
            }
            if hovered.is_none() {
                // Paragraph groups: anywhere over their lines, the margin rule included.
                let col_left = layout.origin.x;
                if p.x > col_left - px(40.) && p.x < col_left + layout.column_width {
                    for g in self.groups.iter().filter(|g| g.scope == Scope::Paragraph) {
                        if let Some((top, bottom)) = layout.vertical_extent(g.range.clone()) {
                            if p.y >= top && p.y <= bottom {
                                hovered = Some(g.id);
                            }
                        }
                    }
                }
            }
        }
        if !self.model.read(cx).layer {
            hovered = None;
        }
        let over_dots = inside && self.dots_at(p, cx).is_some();
        if over_dots != self.over_dots {
            self.over_dots = over_dots;
            cx.notify();
        }
        let over_sel = inside && self.over_selection(&layout, p, cx);
        if over_sel != self.over_sel {
            self.over_sel = over_sel;
            cx.notify();
        }
        if hovered != self.hovered_group || hover_char != self.hover_char || inside != self.hover_in_bounds {
            self.hovered_group = hovered;
            self.hover_char = hover_char;
            self.hover_in_bounds = inside;
            cx.notify();
        }
        self.update_chip(hovered, p, cx);
        let comment = if inside && self.mode == Mode::Page { self.comment_at(p, hover_char) } else { None };
        if comment != self.hovered_comment {
            self.hovered_comment = comment;
            cx.emit(ViewEvent::CommentHover(comment));
            cx.notify();
        }
        let mark = if inside { self.mark_at(p) } else { None };
        if mark != self.hovered_mark {
            self.hovered_mark = mark;
            cx.emit(ViewEvent::MarkHover(mark));
            cx.notify();
        }
    }

    pub(crate) fn mark_rect(&self, at: usize, paragraph: bool, cx: &App) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let lh = layout.line_height;
        if paragraph {
            let len = layout.index.len();
            let y = if at >= len {
                let ends_blank = layout.index.text.ends_with('\n');
                layout.point_for(len).map(|p| p.y + if ends_blank { lh / 2. } else { lh * 1.5 })?
            } else {
                layout.point_for(at).map(|p| p.y - if at == 0 { px(6.) } else { layout.space_above(at) / 2. })?
            };
            return Some(Bounds::new(point(layout.origin.x - px(26.), y - px(0.75)), size(px(14.), px(1.5))));
        }
        let p = layout.point_for(at)?;
        let h = (metrics(self.mode, cx).font_size * 1.2).min(lh);
        Some(Bounds::new(point(p.x - px(0.75), p.y + (lh - h) / 2.), size(px(1.5), h)))
    }

    /// The note or tick under `p`, else the shortest comment holding `c`.
    pub fn comment_at(&self, p: Point<Pixels>, c: Option<usize>) -> Option<CommentId> {
        if let Some((id, _, _)) = self.comment_rects.borrow().iter().find(|(_, b, tick)| {
            let b = if *tick { b.dilate(px(5.)) } else { *b };
            b.contains(&p)
        }) {
            return Some(*id);
        }
        let c = c?;
        self.comments.iter().filter(|k| k.range.start <= c && c < k.range.end).min_by_key(|k| k.range.len()).map(|k| k.id)
    }

    /// Like `comment_at`, wherever the pointer is on the page (for its menu).
    pub fn comment_under(&self, p: Point<Pixels>) -> Option<CommentId> {
        let layout = self.layout.as_ref()?;
        self.comment_at(p, Some(layout.index_for(p)))
    }

    fn mark_at(&self, p: Point<Pixels>) -> Option<doc_model::CutId> {
        let layout = self.layout.as_ref()?;
        let lh = layout.line_height;
        self.marks.iter().find_map(|&(id, at, paragraph)| {
            let len = layout.index.len();
            let hit = if paragraph {
                let y = if at >= len {
                    layout.point_for(len).map(|q| q.y + lh)
                } else {
                    layout.point_for(at).map(|q| q.y - if at == 0 { px(6.) } else { layout.space_above(at) / 2. })
                };
                y.is_some_and(|y| (p.y - y).abs() <= px(6.) && p.x >= layout.origin.x - px(34.) && p.x <= layout.origin.x - px(6.))
            } else {
                layout.point_for(at).is_some_and(|q| (q.x - p.x).abs() <= px(4.) && p.y >= q.y && p.y <= q.y + lh)
            };
            hit.then_some(id)
        })
    }

    fn on_scroll(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Page {
            return;
        }
        let lh = self.layout.as_ref().map_or(px(40.), |l| l.line_height);
        let d = e.delta.pixel_delta(lh);
        if self.wheel_cycles(e, d, cx) {
            return;
        }
        self.glide = None;
        self.scroll_y -= d.y;
        self.clamp_scroll();
        self.autoscroll = false;
        if let Some(p) = Some(e.position) {
            self.update_hover(p, cx);
        }
        cx.notify();
    }

    /// A gesture that starts over a group cycles it; one that starts elsewhere scrolls, even as
    /// groups pass under the pointer. A sideways gesture (a trackpad swiped left or right)
    /// cycles whatever the wheel setting says. Returns whether the event was taken.
    fn wheel_cycles(&mut self, e: &ScrollWheelEvent, d: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let fresh = e.touch_phase == TouchPhase::Started
            || self.wheel.as_ref().is_none_or(|w| now.duration_since(w.last) > WHEEL_REST);
        if fresh {
            self.update_hover(e.position, cx);
            let group = self.hovered_group;
            self.wheel = Some(Wheel { group, travel: px(0.), last: now, lifted: false, sideways: None });
        }
        let wheel = Prefs::get(cx).wheel;
        let layer = self.layer_on(cx);
        let Some(w) = self.wheel.as_mut() else { return false };
        w.last = now;
        if matches!(e.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            w.lifted = true;
        }
        if w.sideways.is_none() && d != Point::default() {
            w.sideways = Some(d.x.abs() > d.y.abs());
        }
        let sideways = w.sideways == Some(true);
        let switches = sideways
            || match wheel {
                WheelSwitch::Always => true,
                WheelSwitch::WithOption => e.modifiers.alt,
                WheelSwitch::Off => false,
            };
        let Some(id) = w.group.filter(|_| layer && switches) else { return false };
        // Fingers moving left or up (natural scrolling) give a negative delta: the next version.
        let along = if sideways { d.x } else { d.y };
        if w.lifted || along == px(0.) {
            return true;
        }
        let step = match e.delta {
            ScrollDelta::Lines(_) => true,
            ScrollDelta::Pixels(_) => {
                w.travel += along;
                w.travel.abs() >= WHEEL_STEP
            }
        };
        if step {
            w.travel = px(0.);
            self.cycle(id, if along < px(0.) { 1 } else { -1 }, cx);
        }
        true
    }

    fn clamp_scroll(&mut self) {
        if let Some(l) = &self.layout {
            let max = (l.total_height - l.line_height * 2.).max(px(0.));
            self.scroll_y = self.scroll_y.clamp(px(0.), max);
        } else {
            self.scroll_y = self.scroll_y.max(px(0.));
        }
    }

    pub fn reveal(&mut self, c: usize, cx: &mut Context<Self>) {
        self.move_to(c, cx);
        self.autoscroll = true;
    }

    pub(crate) fn a11y_description(&self, cx: &App) -> SharedString {
        if self.mode != Mode::Page {
            return SharedString::default();
        }
        let c = self.caret();
        let mut parts = Vec::new();
        if let Some(g) = self.group_at(c) {
            let original = self
                .model
                .read(cx)
                .doc
                .group(g.id)
                .and_then(|i| i.variants.first().map(|v| v.text.chars().take(60).collect::<String>()))
                .unwrap_or_default();
            parts.push(tr!("alternative {n} of {total}, original: {original}", n = g.active + 1, total = g.variant_count, original = original));
        }
        let doc = &self.model.read(cx).doc;
        let in_trace = self.ghosts.iter().any(|r| r.start <= c && c < r.end) || self.marks.iter().any(|(_, at, _)| *at == c);
        if let Some(info) = doc.cut_at(c).filter(|_| in_trace).and_then(|id| doc.cut_info(id)) {
            let words: String = info.text.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
            parts.push(tr!("cut: {words}", words = words));
        }
        parts.join(", ").into()
    }

    pub fn wash_active(&self) -> bool {
        self.wash.is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(220))
    }

    /// The ghost under the pointer, or else the one find raised.
    pub fn hovered_ghost(&self) -> Option<Range<usize>> {
        let hovered = self.hover_char.and_then(|c| self.ghosts.iter().find(|g| g.start <= c && c < g.end).cloned());
        hovered.or_else(|| self.raised_ghost.clone())
    }

    /// The faded cut the selection is strictly inside, on the focused page.
    pub(crate) fn cut_being_edited(&self, window: &Window) -> Option<Range<usize>> {
        if self.mode != Mode::Page || !self.focus.is_focused(window) {
            return None;
        }
        self.ghosts.iter().find(|g| g.start < self.sel.start && self.sel.end < g.end).cloned()
    }

    #[cfg(test)]
    pub fn fades(&self) -> Vec<(Range<usize>, f32)> {
        self.fades.iter().map(|(r, _, _, to)| (r.clone(), *to)).collect()
    }

    pub fn fade_ghost(&mut self, range: Range<usize>, to_ghost: bool, cx: &mut Context<Self>) {
        let alpha = theme::ghost_alpha(Prefs::get(cx));
        let (from, to) = if to_ghost { (1.0, alpha) } else { (alpha, 1.0) };
        if Prefs::get(cx).motion() != crate::prefs::Effects::Off {
            self.fades.push((range, Instant::now(), from, to));
        }
        cx.notify();
    }
}

/// Which lines are a paragraph break's gap. In a run of empty lines after a block the first,
/// third… are breaks and the others empty paragraphs; a break needs a line after it.
pub(crate) fn paragraph_gaps(idx: &TextIndex, styles: &[LineStyle]) -> Vec<bool> {
    let n = idx.lines.len();
    let blank = |l: usize| idx.lines[l].char_len == 0 && styles.get(l).is_none_or(|s| s.kind == BlockKind::Blank);
    let mut out = vec![false; n];
    let mut run: Option<usize> = None;
    for (l, gap) in out.iter_mut().enumerate() {
        if blank(l) {
            if let Some(k) = run.as_mut() {
                *gap = *k % 2 == 0 && l + 1 < n;
                *k += 1;
            }
        } else {
            run = Some(0);
        }
    }
    out
}

/// Assumes one contiguous change; offsets inside it go to its end.
fn map_through_diff(old: &TextIndex, new: &TextIndex, c: usize) -> usize {
    let (prefix, old_end, new_end) = diff_span(&old.text, &new.text);
    let b = old.char_to_byte(c);
    if b <= prefix {
        new.byte_to_char(b)
    } else if b >= old_end {
        new.byte_to_char(b - old_end + new_end)
    } else {
        new.byte_to_char(new_end)
    }
}

/// (common prefix bytes, end of change in old, end of change in new).
fn diff_span(a: &str, b: &str) -> (usize, usize, usize) {
    let mut p = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(p) || !b.is_char_boundary(p) {
        p -= 1;
    }
    let max_s = (a.len() - p).min(b.len() - p);
    let mut s = a.bytes().rev().zip(b.bytes().rev()).take(max_s).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(a.len() - s) || !b.is_char_boundary(b.len() - s) {
        s -= 1;
    }
    (p, a.len() - s, b.len() - s)
}

fn increment_ordered(prefix: &str) -> String {
    let indent = prefix.len() - prefix.trim_start().len();
    let rest = &prefix[indent..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if let Ok(n) = digits.parse::<u64>() {
        format!("{}{}{}", &prefix[..indent], n + 1, &rest[digits.len()..])
    } else {
        prefix.to_string()
    }
}

impl EntityInputHandler for TextView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.index.utf16_to_char(range.start)..self.index.utf16_to_char(range.end);
        adjusted.replace(self.index.char_to_utf16(r.start)..self.index.char_to_utf16(r.end));
        Some(self.index.slice(r).to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.index.char_to_utf16(self.sel.start)..self.index.char_to_utf16(self.sel.end),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|m| self.index.char_to_utf16(m.start)..self.index.char_to_utf16(m.end))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(r) = range {
            let r = self.index.utf16_to_char(r.start)..self.index.utf16_to_char(r.end);
            self.marked = None;
            self.replace(r, text, cx);
        } else {
            self.insert(text, cx);
        }
        self.marked = None;
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        new_sel: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = range
            .map(|r| self.index.utf16_to_char(r.start)..self.index.utf16_to_char(r.end))
            .or(self.marked.clone())
            .unwrap_or(self.sel.clone());
        self.replace(r.clone(), text, cx);
        let n = text.chars().count();
        self.marked = (n > 0).then(|| r.start..r.start + n);
        if let Some(s) = new_sel {
            // `new_sel` is in UTF-16 units relative to the inserted text.
            let units: Vec<usize> = text.chars().scan(0, |acc, ch| { *acc += ch.len_utf16(); Some(*acc) }).collect();
            let to_char = |u: usize| units.iter().take_while(|x| **x <= u).count();
            self.sel = r.start + to_char(s.start)..r.start + to_char(s.end);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let s = self.index.utf16_to_char(range.start);
        let e = self.index.utf16_to_char(range.end);
        let a = layout.point_for(s)?;
        let b = layout.point_for(e).filter(|b| b.y == a.y).unwrap_or(a);
        Some(Bounds::from_corners(a, point(b.x.max(a.x + px(1.)), a.y + layout.line_height)))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.index.len_utf16())
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        Some(self.index.char_to_utf16(layout.index_for(p)))
    }
}

impl Render for TextView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut ctx = KeyContext::new_with_defaults();
        ctx.add("TextView");
        match self.mode {
            Mode::Page => {
                ctx.add("Page");
                if self.hovered_group.is_some() && self.model.read(cx).layer {
                    ctx.add("hovering_group");
                }
                if !self.sel.is_empty() && self.model.read(cx).layer && sentence_keys::whole_unit(&self.index, &self.sel).is_some() {
                    ctx.add("whole_unit");
                }
            }
            Mode::Cut(_) => ctx.add("Note"),
            Mode::Comment(_) => ctx.add("Comment"),
            Mode::Field => ctx.add("Field"),
        }
        let mut el = div()
            .id("text-view")
            .role(match self.mode {
                Mode::Field => Role::TextInput,
                _ => Role::MultilineTextInput,
            })
            .aria_label(match self.mode {
                Mode::Page => tr!("Page"),
                Mode::Cut(_) => tr!("Cut"),
                Mode::Comment(_) => tr!("Comment"),
                Mode::Field => tr!("Field"),
            })
            .aria_value(self.a11y_value.clone())
            .aria_placeholder(self.placeholder.clone())
            .aria_description(self.a11y_description(cx))
            .key_context(ctx)
            .track_focus(&self.focus)
            .cursor(if self.over_dots || self.over_chip {
                CursorStyle::PointingHand
            } else if self.over_sel || self.press_in_sel.is_some() {
                CursorStyle::Arrow
            } else {
                CursorStyle::IBeam
            })
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::move_word_left))
            .on_action(cx.listener(Self::move_word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::move_line_start))
            .on_action(cx.listener(Self::move_line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::move_doc_start))
            .on_action(cx.listener(Self::move_doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_page_up))
            .on_action(cx.listener(Self::select_page_down))
            .on_action(cx.listener(Self::move_paragraph_start))
            .on_action(cx.listener(Self::move_paragraph_end))
            .on_action(cx.listener(Self::select_paragraph_start))
            .on_action(cx.listener(Self::select_paragraph_end))
            .on_action(cx.listener(Self::prev_sentence))
            .on_action(cx.listener(Self::next_sentence))
            .on_action(cx.listener(Self::select_prev_sentence))
            .on_action(cx.listener(Self::select_next_sentence))
            .on_action(cx.listener(Self::grow_action))
            .on_action(cx.listener(Self::shrink_action))
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            .capture_key_down(cx.listener(|v, _: &KeyDownEvent, _, _| v.shift_tap = None))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_line_left))
            .on_action(cx.listener(Self::delete_line_right))
            .on_action(cx.listener(Self::delete_to_paragraph_end))
            .on_action(cx.listener(Self::transpose))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::line_break))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::backtab))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo_action))
            .on_action(cx.listener(Self::redo_action))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll));
        if self.mode == Mode::Page {
            el = el
                .size_full()
                .on_action(cx.listener(Self::bold))
                .on_action(cx.listener(Self::italic))
                .on_action(cx.listener(Self::cycle_next))
                .on_action(cx.listener(Self::cycle_prev))
                .on_action(cx.listener(Self::cycle_at_caret_next))
                .on_action(cx.listener(Self::cycle_at_caret_prev))
                .on_action(cx.listener(Self::back_to_original))
                .on_action(cx.listener(Self::next_group))
                .on_action(cx.listener(Self::prev_group));
        } else {
            el = el.w_full();
        }
        if self.mode != Mode::Page {
            return el.child(PageElement { view: cx.entity() });
        }
        if !cx.has_active_drag() && (self.carrying.is_some() || self.drop_mark.is_some()) {
            // The drag ended somewhere that didn't take it (or Esc).
            self.carrying = None;
            self.drop_mark = None;
        }
        // GPUI drags start from `on_drag`: the empty click handler makes the grip track presses,
        // and it offers a drag only after a press in the selection, so other presses still select.
        let mut grip = div().id("page-grip").size_full().on_click(|_, _, _| {});
        if self.press_in_sel.is_some() {
            let page = cx.entity();
            grip = grip.on_drag(self.text_drag(cx), move |d: &TextDrag, at, _, cx| {
                page.update(cx, |p, cx| {
                    p.press_in_sel = None;
                    p.carrying = Some(d.range.clone());
                    cx.emit(ViewEvent::DragStarted);
                    cx.notify();
                });
                let mut d = d.clone();
                d.grip = at;
                cx.new(|_| d)
            });
        }
        el.child(grip.child(PageElement { view: cx.entity() }))
    }
}

struct PageElement {
    view: Entity<TextView>,
}

impl IntoElement for PageElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub(crate) fn heading_size(font_size: Pixels, line_height: Pixels, level: u8) -> Pixels {
    let step = match level {
        1 => 1.7,
        2 => 1.4,
        _ => 1.2,
    };
    (font_size * step * theme::hand_font().scale()).min(line_height * 0.95)
}

const COLUMN_SAMPLE: &str = "At last I have come back, and found that they had not been expecting me at all.";

/// An average letter of [`COLUMN_SAMPLE`]: counting "m"s in a proportional face set 110-130 a line.
pub(crate) fn average_letter(advance: impl Fn(char) -> Option<Pixels>) -> Option<Pixels> {
    let widths: Vec<Pixels> = COLUMN_SAMPLE.chars().map(&advance).collect::<Option<_>>()?;
    let (lo, hi) = widths.iter().fold((widths[0], widths[0]), |(lo, hi), &w| (lo.min(w), hi.max(w)));
    // A mono's letter exactly, so its page can't move by a rounding.
    Some(if lo == hi { lo } else { widths.iter().fold(px(0.), |a, &b| a + b) / widths.len() as f32 })
}

/// (comment, painted bounds, tick rather than note).
pub type CommentRects = Rc<RefCell<Vec<(CommentId, Bounds<Pixels>, bool)>>>;

pub const COMMENT_PAD: Pixels = px(5.);

#[derive(Clone)]
struct Metrics {
    family: SharedString,
    font_size: Pixels,
    line_height: Pixels,
    column_chars: u32,
    ghost_alpha: f32,
    ghost_hover_alpha: f32,
}

fn metrics(mode: Mode, cx: &App) -> Metrics {
    let p = Prefs::get(cx);
    match mode {
        Mode::Page => Metrics {
            family: p.font_family.clone().into(),
            font_size: px(p.font_size),
            line_height: px((p.font_size * p.line_spacing).round()),
            column_chars: p.column_chars,
            ghost_alpha: theme::ghost_alpha(p),
            ghost_hover_alpha: theme::ghost_hover_alpha(p),
        },
        Mode::Cut(_) => Metrics {
            family: theme::UI.into(),
            font_size: px(theme::NOTE_PX),
            line_height: px(theme::NOTE_LINE),
            column_chars: 0,
            ghost_alpha: theme::ghost_alpha(p),
            ghost_hover_alpha: theme::ghost_hover_alpha(p),
        },
        Mode::Comment(_) => Metrics {
            family: theme::UI.into(),
            font_size: px(theme::NOTE_PX),
            line_height: px(theme::NOTE_LINE),
            column_chars: 0,
            ghost_alpha: theme::ghost_alpha(p),
            ghost_hover_alpha: theme::ghost_hover_alpha(p),
        },
        Mode::Field => Metrics {
            family: theme::UI.into(),
            font_size: px(13.),
            line_height: px(22.),
            column_chars: 0,
            ghost_alpha: theme::ghost_alpha(p),
            ghost_hover_alpha: theme::ghost_hover_alpha(p),
        },
    }
}

impl TextView {
    fn text_lang(&mut self) -> Option<prose::Lang> {
        use std::hash::{Hash, Hasher};
        let n = self.index.len();
        let first = {
            let mut h = std::hash::DefaultHasher::new();
            self.index.lines.first().map(|_| self.index.line_str(0)).unwrap_or("").hash(&mut h);
            h.finish()
        };
        if let Some((at, f, lang)) = self.text_lang {
            let again = if lang.is_some() { 2000 } else { 100 };
            if f == first && at.abs_diff(n) < again {
                return lang;
            }
        }
        let mut sample = String::new();
        for li in 0..self.index.lines.len() {
            sample.push_str(self.index.line_str(li));
            sample.push('\n');
            if sample.len() > 20_000 {
                break;
            }
        }
        let lang = prose::detect(&sample);
        self.text_lang = Some((n, first, lang));
        lang
    }

    fn compute_layout(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) -> Layout {
        let m = metrics(self.mode, cx);
        let ts = window.text_system().clone();
        // Markdown is source text: `-->` and `!=` must look like what they are.
        let regular = Font { features: FontFeatures::disable_ligatures(), ..font(m.family.clone()) };
        let regular_id = ts.resolve_font(&regular);
        let char_w = average_letter(|c| ts.advance(regular_id, m.font_size, c).ok().map(|s| s.width)).unwrap_or(m.font_size * 0.6);
        let (column_width, left) = if self.mode == Mode::Page {
            let w = (char_w * m.column_chars as f32).min(bounds.size.width - px(80.)).max(px(120.));
            // Centred in the window, not in what the panels leave, so opening one doesn't move text.
            let centred = ((window.viewport_size().width - w) / 2.).floor();
            let (lo, hi) = (bounds.left() + px(40.), bounds.right() - px(40.) - w);
            (w, if lo <= hi { centred.clamp(lo, hi) } else { bounds.left() + ((bounds.size.width - w) / 2.).floor() })
        } else {
            (bounds.size.width.max(px(20.)), bounds.left())
        };
        let caret_line = self.index.line_of(self.caret());
        let focused_page = self.mode == Mode::Page;
        let (justify, hyphenate) = {
            let p = Prefs::get(cx);
            (self.mode == Mode::Page && p.alignment == Alignment::Justified, self.mode == Mode::Page && p.hyphenation)
        };
        let indented = self.mode == Mode::Page && Prefs::get(cx).paragraphs == ParagraphLook::Indented;
        let text_lang = if hyphenate || indented { self.text_lang() } else { None };
        let lang = text_lang.filter(|_| hyphenate);
        // Russian books indent the first paragraph too; English ones start it flush.
        let indent_first = text_lang == Some(prose::Lang::Ru);
        let indent_px = (m.font_size * 1.5).round();
        let gaps = self.gaps.clone();
        let hyphen_w = ts.advance(ts.resolve_font(&regular), m.font_size, '-').map(|s| s.width).unwrap_or(char_w);
        // The word being typed isn't broken: it would jump between rows at every letter.
        let typed_word = self.last_typing.is_some().then(|| {
            let s = self.index.line_str(caret_line);
            let cb = self.index.byte_in_line(caret_line, self.caret());
            let start = s[..cb].char_indices().rev().take_while(|(_, c)| c.is_alphabetic()).last().map_or(cb, |(i, _)| i);
            let end = s[cb..].char_indices().take_while(|(_, c)| c.is_alphabetic()).last().map_or(cb, |(i, c)| cb + i + c.len_utf8());
            start..end
        });
        let hand_headings = self.mode == Mode::Page && Prefs::get(cx).hand_headings;
        let hand = font(theme::hand());
        let margin = left - bounds.left();
        let idx = self.index.clone();
        let styles = self.styles.clone();
        let ghosts = self.ghosts.clone();
        let hover_ghost = self.hovered_ghost();
        let in_cut = self.cut_being_edited(window);
        let now = Instant::now();
        self.fades.retain(|(_, t, _, _)| now.duration_since(*t) < Duration::from_millis(350));
        let fades = self.fades.clone();
        let zen = self.zen_lines(cx);
        if zen != self.zen {
            self.zen_prev = std::mem::replace(&mut self.zen, zen);
            self.zen_since = (Prefs::get(cx).motion() != crate::prefs::Effects::Off).then_some(now);
        }
        let zen_k = match self.zen_since {
            Some(t) if now.duration_since(t) < ZEN_FADE => {
                let k = now.duration_since(t).as_secs_f32() / ZEN_FADE.as_secs_f32();
                1. - (1. - k) * (1. - k)
            }
            _ => {
                self.zen_since = None;
                1.
            }
        };
        let zen_alpha = |lit: &Option<Range<usize>>, li: usize| match lit {
            Some(r) if !r.contains(&li) => ZEN_DIM,
            _ => 1.,
        };
        let kind_of = |l: usize| {
            styles.get(l).map(|s| s.kind).unwrap_or(if idx.line_str(l).trim().is_empty() { BlockKind::Blank } else { BlockKind::Paragraph })
        };
        let is_gap = |l: usize| gaps.get(l).copied().unwrap_or(false);
        let empty_para = |l: usize| idx.lines[l].char_len == 0 && kind_of(l) == BlockKind::Blank && !is_gap(l) && l > 0;
        let gap_height = |l: usize| {
            let body = |k: usize| kind_of(k) == BlockKind::Paragraph || empty_para(k);
            if l > 0 && body(l - 1) && l + 1 < idx.lines.len() && body(l + 1) {
                if indented { px(0.) } else { (m.line_height * (2. / 3.)).round() }
            } else {
                m.line_height
            }
        };
        let indent_of = |l: usize| {
            if !indented || !(kind_of(l) == BlockKind::Paragraph || empty_para(l)) {
                return px(0.);
            }
            let starts = l == 0 || (idx.lines[l - 1].char_len == 0 && kind_of(l - 1) == BlockKind::Blank);
            if !starts {
                return px(0.);
            }
            if indent_first {
                return indent_px;
            }
            let mut k = l;
            while k > 0 {
                k -= 1;
                if empty_para(k) {
                    return indent_px;
                }
                if !(idx.lines[k].char_len == 0 && kind_of(k) == BlockKind::Blank) {
                    return if kind_of(k) == BlockKind::Paragraph { indent_px } else { px(0.) };
                }
            }
            px(0.)
        };
        let mut lines = Vec::with_capacity(idx.lines.len());
        let mut y = px(0.);
        let mut used = HashMap::new();
        let mut cache = self.cache.borrow_mut();
        for li in 0..idx.lines.len() {
            let s = idx.line_str(li);
            let info = idx.lines[li];
            let style = styles.get(li).cloned().unwrap_or(LineStyle {
                kind: if s.trim().is_empty() { BlockKind::Blank } else { BlockKind::Paragraph },
                spans: Vec::new(),
                hidden_prefix: 0,
            });
            let hide = style.hidden_prefix > 0 && !(focused_page && caret_line == li);
            let level = match style.kind {
                BlockKind::Heading(l) if hand_headings => Some(l),
                _ => None,
            };
            let line_size = level.map_or(m.font_size, |l| heading_size(m.font_size, m.line_height, l));
            let prefix_w = s
                .chars()
                .take(style.hidden_prefix)
                .map(|c| ts.advance(regular_id, line_size, c).map_or(char_w * (line_size / m.font_size), |a| a.width))
                .fold(px(0.), |a, b| a + b);
            // Shown, the `# ` hangs left of the column so the title doesn't move.
            let hang = style.hidden_prefix > 0 && self.mode == Mode::Page && prefix_w <= margin;
            let (from, to) = (zen_alpha(&self.zen_prev, li), zen_alpha(&self.zen, li));
            let line_alpha = if zen_k < 1. { from + (to - from) * zen_k } else { to };
            let line_range = info.char_start..info.char_start + info.char_len;
            // Attribute boundaries, in bytes within the line.
            let mut cuts = vec![0, s.len()];
            for (r, _) in &style.spans {
                cuts.push(r.start);
                cuts.push(r.end);
            }
            let to_b = |c: usize| idx.byte_in_line(li, c.clamp(line_range.start, line_range.end));
            let ghost_cuts = |r: &Range<usize>, cuts: &mut Vec<usize>| {
                if r.start < line_range.end && r.end > line_range.start {
                    cuts.push(to_b(r.start));
                    cuts.push(to_b(r.end));
                }
            };
            for g in ghosts.iter() {
                ghost_cuts(g, &mut cuts);
            }
            for (r, ..) in &fades {
                ghost_cuts(r, &mut cuts);
            }
            if let Some(m) = &self.marked {
                ghost_cuts(m, &mut cuts);
            }
            if let Some(f) = &self.focus_range {
                ghost_cuts(f, &mut cuts);
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut runs = Vec::new();
            let mut key = std::hash::DefaultHasher::new();
            s.hash(&mut key);
            for w in cuts.windows(2) {
                let (a, b) = (w[0], w[1]);
                if a >= b {
                    continue;
                }
                let span = style
                    .spans
                    .iter()
                    .rev()
                    .find(|(r, _)| r.start <= a && b <= r.end)
                    .map(|(_, k)| *k);
                let mut weight = FontWeight::NORMAL;
                let mut fstyle = FontStyle::Normal;
                let mut color = c(match self.mode {
                    Mode::Cut(_) => theme::text_2(),
                    Mode::Comment(_) => theme::comment(),
                    _ => theme::text(),
                });
                if matches!(style.kind, BlockKind::Heading(_)) && level.is_none() {
                    weight = FontWeight::BOLD;
                }
                if style.kind == BlockKind::Quote {
                    color = c(theme::text_2());
                }
                let mut underline = None;
                match span {
                    Some(Span::Marker) => color = c(theme::muted()),
                    Some(Span::Strong) => weight = FontWeight::BOLD,
                    Some(Span::Emphasis) => fstyle = FontStyle::Italic,
                    Some(Span::StrongEmphasis) => {
                        weight = FontWeight::BOLD;
                        fstyle = FontStyle::Italic;
                    }
                    Some(Span::Code) => color = c(theme::text_2()),
                    Some(Span::Link) => {
                        underline = Some(UnderlineStyle { thickness: px(1.), color: Some(ca(theme::muted(), 0.8)), wavy: false })
                    }
                    None => {}
                }
                if style.kind == BlockKind::Rule {
                    color = c(theme::faint());
                }
                let ca_ = info.char_start + s[..a].chars().count();
                if hide && b <= style.hidden_prefix {
                    color.a = 0.;
                }
                let in_r = |r: &Range<usize>| r.start <= ca_ && ca_ < r.end;
                let mut ghost_factor = 1.;
                if let Some(g) = ghosts.iter().find(|g| in_r(g)) {
                    let hovered = hover_ghost.as_ref() == Some(g);
                    ghost_factor = if in_cut.as_ref() == Some(g) {
                        CUT_EDIT_ALPHA
                    } else if hovered {
                        m.ghost_hover_alpha
                    } else {
                        m.ghost_alpha
                    };
                }
                if let Some((_, t, from, to)) = fades.iter().rev().find(|(r, ..)| in_r(r)) {
                    let k = (now.duration_since(*t).as_secs_f32() / 0.35).clamp(0., 1.);
                    let k = 1. - (1. - k) * (1. - k);
                    ghost_factor = from + (to - from) * k;
                }
                color.a *= ghost_factor * line_alpha;
                if let Some(u) = underline.as_mut().and_then(|u| u.color.as_mut()) {
                    u.a *= line_alpha;
                }
                if let Some(f) = &self.focus_range {
                    if !in_r(f) {
                        color.a *= 0.38;
                    }
                }
                if self.marked.as_ref().is_some_and(in_r) {
                    underline = Some(UnderlineStyle { thickness: px(1.), color: Some(color), wavy: false });
                }
                let f = if level.is_some() && !matches!(span, Some(Span::Marker | Span::Code)) {
                    hand.clone()
                } else {
                    Font { weight, style: fstyle, ..regular.clone() }
                };
                (b - a, weight.0.to_bits(), fstyle == FontStyle::Italic, color.a.to_bits(), color.l.to_bits(), color.h.to_bits(), color.s.to_bits(), underline.is_some()).hash(&mut key);
                runs.push(TextRun { len: b - a, font: f, color, background_color: None, underline, strikethrough: None });
            }
            let shift_px = if hide || hang { prefix_w } else { px(0.) };
            let wrap = column_width;
            let indent = indent_of(li);
            (f32::from(wrap).to_bits(), f32::from(line_size).to_bits(), f32::from(indent).to_bits(), m.family.as_ref(), level.map(|_| theme::hand())).hash(&mut key);
            let set = matches!(style.kind, BlockKind::Paragraph | BlockKind::Quote);
            let justify_line = justify && set;
            let line_lang = lang.filter(|_| set);
            let keep = typed_word.clone().filter(|_| line_lang.is_some() && li == caret_line);
            (justify_line, line_lang.map(|l| l.code()), keep.clone()).hash(&mut key);
            let k = key.finish();
            let shaped = if let Some(sh) = cache.get(&k) {
                sh.clone()
            } else {
                // The shaper gives a tab no width: a space (one byte, so offsets hold) shows the split.
                let text: SharedString = s.replace('\t', " ").into();
                let mut wrapped = ts
                    .shape_text(text, line_size, &runs, Some(wrap), None)
                    .ok()
                    .and_then(|mut v| v.pop())
                    .unwrap_or_default();
                let breaks = |w: &str| line_lang.map_or_else(|| Rc::from(Vec::new()), |l| crate::hyphen::breaks(w, l));
                let hyphens = line_lang.map(|_| crate::wrap::Hyphens { breaks: &breaks, width: hyphen_w, keep });
                let hyphenated = crate::wrap::rewrap(&mut wrapped, wrap, hyphens.as_ref(), indent);
                if justify_line {
                    let from = if style.kind == BlockKind::Quote { s.len() - s.trim_start_matches(['>', ' ']).len() } else { 0 };
                    crate::wrap::justify(&mut wrapped, wrap, &hyphenated, hyphen_w, from, indent);
                }
                let ul = &wrapped.unwrapped_layout;
                let mut starts = vec![0];
                for b in wrapped.wrap_boundaries() {
                    starts.push(ul.runs[b.run_ix].glyphs[b.glyph_ix].index);
                }
                let rows: Vec<Row> = starts
                    .iter()
                    .enumerate()
                    .map(|(i, st)| Row {
                        start: *st,
                        end: starts.get(i + 1).copied().unwrap_or(s.len()),
                        x0: ul.x_for_index(*st),
                        hyphen: hyphenated.get(i) == Some(&true),
                    })
                    .collect();
                let hyphens = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.hyphen)
                    .filter_map(|(i, r)| {
                        let mut at = 0;
                        let run = runs.iter().find(|run| {
                            at += run.len;
                            at >= r.end
                        })?;
                        let run = TextRun { len: 1, underline: None, ..run.clone() };
                        let line = ts.shape_line("-".into(), line_size, &[run], None);
                        Some((i, ul.x_for_index(r.end) - r.x0, line))
                    })
                    .collect();
                Rc::new(ShapedLine { wrapped, rows, hyphens })
            };
            used.insert(k, shaped.clone());
            let rows = shaped.rows.len().max(1);
            let gap = is_gap(li);
            let height = if gap { gap_height(li) } else { m.line_height * rows as f32 };
            let x_shift = if indent > px(0.) { indent } else { -shift_px };
            lines.push(LineBox { line: li, shaped, y, height, x_shift, kind: style.kind, alpha: line_alpha, gap });
            y += height;
        }
        *cache = used;
        let top_pad = match self.mode {
            Mode::Page => px(196.),
            Mode::Comment(_) => COMMENT_PAD,
            _ => px(0.),
        };
        Layout {
            lines,
            origin: point(left, bounds.top() + top_pad),
            column_width,
            line_height: m.line_height,
            font_size: m.font_size,
            total_height: y,
            bounds,
            index: idx,
        }
    }
}

impl Element for PageElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mode = self.view.read(cx).mode;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if mode == Mode::Page {
            style.size.height = relative(1.).into();
            return (window.request_layout(style, [], cx), ());
        }
        let view = self.view.clone();
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(w) => w,
                _ => px(300.),
            });
            let bounds = Bounds::new(point(px(0.), px(0.)), size(width, px(10000.)));
            let h = view.update(cx, |v, cx| {
                v.refresh_index_app(cx);
                let l = v.compute_layout(bounds, window, cx);
                let pad = if matches!(v.mode, Mode::Comment(_)) { COMMENT_PAD * 2. } else { px(0.) };
                l.total_height.max(l.line_height) + pad
            });
            size(width, h)
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.view.update(cx, |v, cx| {
            let t0 = Instant::now();
            v.refresh_index_app(cx);
            let t1 = Instant::now();
            let mut layout = v.compute_layout(bounds, window, cx);
            if std::env::var_os("DOSTOEVSKY_PERF").is_some() && v.mode == Mode::Page {
                eprintln!("perf: index {:?} layout {:?}", t1 - t0, t1.elapsed());
            }
            if v.mode == Mode::Page {
                let (h, lh) = (bounds.size.height, layout.line_height);
                let centred = |top: Pixels| top - (h - lh) / 2.;
                let mut max = (layout.total_height + px(120.) - h * 0.5).max(px(0.));
                if Prefs::get(cx).typewriter {
                    // From Prefs, not the layer, so toggling the layer doesn't move the page.
                    max = max.max(centred(layout.origin.y - bounds.top() + layout.total_height - lh));
                }
                if v.autoscroll && v.drag.is_none() {
                    if let Some(p) = layout.point_for(v.caret()) {
                        let top = p.y - bounds.top();
                        let margin = lh * 2.;
                        let typewriter = v.typewriter(cx);
                        let target = if typewriter {
                            centred(top)
                        } else if top - v.scroll_y < margin {
                            top - margin
                        } else if top - v.scroll_y + lh + margin > h {
                            top + lh + margin - h
                        } else {
                            v.scroll_y
                        };
                        let target = target.clamp(px(0.), max);
                        let glide = typewriter
                            && Prefs::get(cx).motion() != crate::prefs::Effects::Off
                            && (target - v.scroll_y).abs() <= h;
                        // Typing along a line keeps the glide already under way.
                        if v.glide.is_none_or(|(_, to, _)| to != target) {
                            if glide && (target - v.scroll_y).abs() > px(0.5) {
                                v.glide = Some((v.scroll_y, target, Instant::now()));
                            } else {
                                v.glide = None;
                                v.scroll_y = target;
                            }
                        }
                    }
                }
                v.autoscroll = false;
                if let Some(r) = v.centre_request.take() {
                    if let Some((top, bottom)) = layout.vertical_extent(r) {
                        let (top, bottom) = (top - bounds.top(), bottom - bounds.top());
                        let target = if bottom - top > h * 0.6 { centred(top) } else { (top + bottom - h) / 2. };
                        let target = target.clamp(px(0.), max);
                        let glide = Prefs::get(cx).motion() != crate::prefs::Effects::Off && (target - v.scroll_y).abs() <= h * 2.;
                        if glide && (target - v.scroll_y).abs() > px(0.5) {
                            v.glide = Some((v.scroll_y, target, Instant::now()));
                        } else {
                            v.glide = None;
                            v.scroll_y = target;
                        }
                    }
                }
                if let Some((from, to, t)) = v.glide {
                    let k = t.elapsed().as_secs_f32() / GLIDE.as_secs_f32();
                    if k >= 1. {
                        v.scroll_y = to;
                        v.glide = None;
                    } else {
                        v.scroll_y = from + (to - from) * (1. - (1. - k) * (1. - k));
                    }
                }
                v.scroll_y = v.scroll_y.clamp(px(0.), max);
                layout.origin.y -= v.scroll_y;
            }
            v.slide_caret(&layout, cx);
            v.layout = Some(Rc::new(layout));
            v.update_spelling(bounds, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = self.view.read(cx);
        let Some(layout) = view.layout.clone() else { return };
        let focus = view.focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        let focused = focus.is_focused(window);
        let lh = layout.line_height;
        let mode = view.mode;
        let sel = view.sel.clone();
        let layer = view.layer_on(cx);
        let groups = view.groups.clone();
        let hovered = view.hovered_group;
        let wash = view.wash;
        let spotlight = view.spotlight;
        let flash = view.flash.clone();
        let caret = view.caret();
        let caret_on = view.caret_on;
        let now = Instant::now();
        let caret_slide = view.caret_slide;
        let placeholder = view.placeholder.clone();
        let hint_after = view.hint_after.clone();
        let quiet_when_empty = view.quiet_when_empty;
        let animating = view.wash_active()
            || !view.fades.is_empty()
            || view.zen_fading()
            || view.gliding()
            || caret_slide.is_some_and(|s| focused && sel.is_empty() && s.sliding(now))
            || flash.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(900));
        let m = metrics(mode, cx);
        let ghost_alpha = m.ghost_alpha;
        let ghosts = view.ghosts.clone();
        let highlights = view.highlights.clone();
        let current_highlight = view.current_highlight;
        let chip = view.hover_chip(cx);
        let chip_rect = view.chip_rect.clone();
        let misspelled = view.misspelled.clone();
        let lab = view.lab.clone();
        let carrying = view.carrying.clone();
        let drop_mark = view.drop_mark;
        let in_cut = view.cut_being_edited(window);
        let hovered_mark = view.hovered_mark;
        let comments = view.comments.clone();
        let comment_lit = {
            let (hovered, writing) = (view.hovered_comment, view.editing_comment);
            let caret = sel.is_empty().then_some(sel.start);
            comments
                .iter()
                .filter(|k| Some(k.id) == hovered || Some(k.id) == writing || caret.is_some_and(|c| k.range.start <= c && c <= k.range.end && !k.range.is_empty()))
                .map(|k| k.id)
                .collect::<Vec<CommentId>>()
        };
        let editing_comment = view.editing_comment;
        let comment_ticks = view.comment_ticks || Prefs::get(cx).comment_look == crate::prefs::CommentLook::Ticks;
        let comment_rects = view.comment_rects.clone();
        comment_rects.borrow_mut().clear();
        view.comment_rects_scroll.set(view.scroll_y);
        let marks: Vec<(doc_model::CutId, Bounds<Pixels>)> = view.marks.iter().filter_map(|&(id, at, p)| Some((id, view.mark_rect(at, p, cx)?))).collect();

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let visible = |b: &LineBox| {
                let top = layout.origin.y + b.y;
                top + b.height >= bounds.top() - lh && top <= bounds.bottom() + lh
            };

            if layer {
                for g in groups.iter().filter(|g| g.scope == Scope::Paragraph) {
                    let washed = wash.is_some_and(|(id, t)| id == g.id && t.elapsed() < Duration::from_millis(400));
                    if hovered == Some(g.id) || washed {
                        if let Some((top, bottom)) = layout.vertical_extent(g.range.clone()) {
                            let alpha = if washed { 1. } else { 0.6 };
                            window.paint_quad(
                                fill(
                                    Bounds::from_corners(
                                        point(layout.origin.x - px(14.), top - px(4.)),
                                        point(layout.origin.x + layout.column_width + px(14.), bottom + px(4.)),
                                    ),
                                    ca(theme::para_hl(), alpha),
                                )
                                .corner_radii(px(6.)),
                            );
                        }
                    }
                }
            }
            if let Some(g) = spotlight.and_then(|id| groups.iter().find(|g| g.id == id)) {
                if g.scope == Scope::Paragraph {
                    if let Some((top, bottom)) = layout.vertical_extent(g.range.clone()) {
                        window.paint_quad(
                            fill(
                                Bounds::from_corners(
                                    point(layout.origin.x - px(14.), top - px(4.)),
                                    point(layout.origin.x + layout.column_width + px(14.), bottom + px(4.)),
                                ),
                                ca(theme::para_hl(), 0.8),
                            )
                            .corner_radii(px(6.)),
                        );
                    }
                } else {
                    for r in layout.rects(g.range.clone(), false) {
                        window.paint_quad(fill(r, ca(theme::accent(), 0.16)).corner_radii(px(3.)));
                    }
                }
            }
            if let Some((r, t)) = &flash {
                let k = 1. - (t.elapsed().as_secs_f32() / 0.9).clamp(0., 1.);
                if let Some((top, bottom)) = layout.vertical_extent(r.clone()) {
                    window.paint_quad(
                        fill(
                            Bounds::from_corners(
                                point(layout.origin.x - px(14.), top - px(4.)),
                                point(layout.origin.x + layout.column_width + px(14.), bottom + px(4.)),
                            ),
                            ca(theme::accent(), 0.18 * k),
                        )
                        .corner_radii(px(6.)),
                    );
                }
            }
            if let Some((id, t)) = wash {
                let k = t.elapsed().as_secs_f32() / 0.22;
                if k < 1. {
                    if let Some(g) = groups.iter().find(|g| g.id == id && g.scope != Scope::Paragraph) {
                        for r in layout.rects(g.range.clone(), false) {
                            window.paint_quad(fill(r, ca(theme::accent(), 0.22 * (1. - k))).corner_radii(px(3.)));
                        }
                    }
                }
            }

            for mk in lab.marks.iter().filter(|m| !m.word) {
                let alpha = if mk.current { 0.2 } else { 0.11 };
                for r in layout.rects(mk.range.clone(), false) {
                    window.paint_quad(fill(r, ca(theme::mark(), alpha)).corner_radii(px(3.)));
                }
            }

            for k in comments.iter() {
                let alpha = if comment_lit.contains(&k.id) { 0.24 } else { 0.10 };
                for r in layout.rects(k.range.clone(), false) {
                    window.paint_quad(fill(r, ca(theme::comment(), alpha)).corner_radii(px(2.)));
                }
            }

            for (i, h) in highlights.iter().enumerate() {
                let current = current_highlight == Some(i);
                for r in layout.rects(h.clone(), false) {
                    let color = if current { ca(theme::accent(), 0.45) } else { ca(theme::accent(), 0.18) };
                    window.paint_quad(fill(r, color).corner_radii(px(2.)));
                }
            }

            if !sel.is_empty() {
                let color = if focused { ca(theme::accent(), 0.32) } else { ca(theme::accent(), 0.16) };
                for r in layout.rects(sel.clone(), true) {
                    window.paint_quad(fill(r, color));
                }
            }

            for b in layout.lines.iter().filter(|b| visible(b)) {
                let origin = point(layout.origin.x + b.x_shift, layout.origin.y + b.y);
                if b.kind == BlockKind::Rule && mode == Mode::Page {
                    let y = origin.y + lh / 2.;
                    window.paint_quad(fill(
                        Bounds::new(point(layout.origin.x, y), size(layout.column_width, px(1.))),
                        ca(theme::line(), b.alpha),
                    ));
                    if caret_line_is(&layout, caret, b.line) {
                        let _ = b.shaped.wrapped.paint(origin, lh, TextAlign::Left, None, window, cx);
                    }
                    continue;
                }
                if b.x_shift != px(0.) && b.shaped.rows.len() > 1 {
                    // Only the first row is shifted: paint it, then the rest from the column.
                    let first = Bounds::from_corners(point(px(-1e5), px(-1e5)), point(px(1e5), origin.y + lh));
                    let rest = Bounds::from_corners(point(px(-1e5), origin.y + lh), point(px(1e5), px(1e7)));
                    window.with_content_mask(Some(ContentMask { bounds: first }), |window| {
                        let _ = b.shaped.wrapped.paint(origin, lh, TextAlign::Left, None, window, cx);
                    });
                    window.with_content_mask(Some(ContentMask { bounds: rest }), |window| {
                        let _ = b.shaped.wrapped.paint(point(layout.origin.x, origin.y), lh, TextAlign::Left, None, window, cx);
                    });
                    for (r, x, hyphen) in &b.shaped.hyphens {
                        let at = point(layout.origin.x + b.shift(*r) + *x, origin.y + lh * *r as f32);
                        let _ = hyphen.paint(at, lh, TextAlign::Left, None, window, cx);
                    }
                    continue;
                }
                let _ = b.shaped.wrapped.paint(origin, lh, TextAlign::Left, None, window, cx);
                for (r, x, hyphen) in &b.shaped.hyphens {
                    let _ = hyphen.paint(point(origin.x + *x, origin.y + lh * *r as f32), lh, TextAlign::Left, None, window, cx);
                }
            }

            if layout.index.len() == 0 && !placeholder.is_empty() {
                let run = TextRun {
                    len: placeholder.len(),
                    font: font(m.family.clone()),
                    color: c(theme::faint()),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window.text_system().shape_line(placeholder.clone(), m.font_size, &[run], None);
                let _ = line.paint(layout.origin, lh, TextAlign::Left, None, window, cx);
            } else if layout.index.len() > 0 && !hint_after.is_empty() {
                if let Some(p) = layout.point_for(layout.index.len()) {
                    let run = TextRun {
                        len: hint_after.len(),
                        font: font(m.family.clone()),
                        color: c(theme::faint()),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let line = window.text_system().shape_line(hint_after.clone(), m.font_size, &[run], None);
                    let _ = line.paint(point(p.x + px(2.), p.y), lh, TextAlign::Left, None, window, cx);
                }
            } else if layout.index.len() == 0 && mode == Mode::Page && !quiet_when_empty {
                let run = |text: &str, weight: FontWeight| TextRun {
                    len: text.len(),
                    font: Font { weight, ..font(m.family.clone()) },
                    color: c(theme::placeholder()),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let title = tr!("Title");
                let line = window.text_system().shape_line(title.into(), m.font_size, &[run(title, FontWeight::BOLD)], None);
                let _ = line.paint(layout.origin, lh, TextAlign::Left, None, window, cx);
                let hint = tr!("Start writing. Select anything and press ⇧⌘A to keep other versions of it.");
                let at = point(layout.origin.x, layout.origin.y + lh * 2.);
                if let Ok(lines) = window.text_system().shape_text(hint.into(), m.font_size, &[run(hint, FontWeight::NORMAL)], Some(layout.column_width), None) {
                    for l in lines {
                        let _ = l.paint(at, lh, TextAlign::Left, None, window, cx);
                    }
                }
            }

            if layer {
                for g in groups.iter() {
                    // A version without the words shows as a cut (faded text or a mark): only
                    // the group's dots say it is one of the group's versions.
                    if g.cut {
                        if let Some((start, horizontal)) = dots_start(&layout, g) {
                            paint_dots(start, horizontal, g, 1., window);
                        }
                        continue;
                    }
                    let ghosted = g.ghosted || ghosts.iter().any(|r| r.start <= g.range.start && g.range.end <= r.end);
                    let alpha = if ghosted { ghost_alpha.max(0.08) * 1.5 } else { 1. };
                    if g.scope == Scope::Paragraph {
                        paint_paragraph_rule(&layout, g, alpha, hovered == Some(g.id), window);
                    } else {
                        paint_underline(&layout, g, alpha, hovered == Some(g.id), window);
                    }
                }
            }

            for (r, current) in lab.cuts.iter() {
                for rect in layout.rects(r.clone(), false) {
                    let mid = rect.top() + lh / 2. + px(1.);
                    if *current {
                        window.paint_quad(fill(rect.dilate(px(2.)), ca(theme::mark(), 0.16)).corner_radii(px(3.)));
                        window.paint_quad(fill(Bounds::new(point(rect.left(), mid), size(rect.size.width, px(1.2))), ca(theme::mark(), 0.9)));
                    } else {
                        window.paint_quad(fill(rect, ca(theme::bg(), 0.68)));
                        window.paint_quad(fill(Bounds::new(point(rect.left(), mid), size(rect.size.width, px(1.))), ca(theme::text(), 0.38)));
                    }
                }
            }

            // The current mark also gets Walk through's wash, so the bar's quote and the page agree.
            for mk in lab.marks.iter().filter(|m| m.word) {
                let alpha = if mk.current { 1. } else { 0.75 };
                for rect in layout.rects(mk.range.clone(), false) {
                    if mk.current {
                        window.paint_quad(fill(rect.dilate(px(2.)), ca(theme::mark(), 0.16)).corner_radii(px(3.)));
                    }
                    let y = rect.top() + lh / 2. + layout.font_size * 0.62 + px(1.5);
                    let mut x = rect.left() + px(0.5);
                    while x + px(2.) <= rect.right() {
                        window.paint_quad(fill(Bounds::new(point(x, y), size(px(2.), px(2.))), ca(theme::mark(), alpha)).corner_radii(px(1.)));
                        x += px(4.);
                    }
                }
            }

            // REVIEW's notes and comments' notes share one stack in text order, so they never overlap.
            // With no room right of the column REVIEW's notes go and comments become ticks.
            let note_x = layout.origin.x + layout.column_width + px(30.);
            let room = bounds.right() - note_x - px(16.);
            let mut review: Vec<(Pixels, &PaintMark)> = Vec::new();
            for mk in lab.marks.iter().filter(|m| m.note.is_some()) {
                let Some(first) = layout.rects(mk.range.clone(), false).into_iter().next() else { continue };
                review.push((first.top(), mk));
            }
            let mut notes: Vec<(Pixels, &CommentInfo)> = comments
                .iter()
                .filter_map(|k| {
                    let top = layout.rects(k.range.clone(), false).into_iter().next().map(|r| r.top()).or_else(|| layout.point_for(k.range.start).map(|p| p.y))?;
                    Some((top, k))
                })
                .collect();
            notes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let in_margin = room >= px(110.) && !comment_ticks;
            // REVIEW's notes keep their order; comments go in by where they are.
            let mut stack: Vec<(Pixels, Option<&PaintMark>, Option<&CommentInfo>)> = Vec::new();
            let mut ki = 0;
            for (top, mk) in &review {
                while in_margin && ki < notes.len() && notes[ki].0 < *top {
                    stack.push((notes[ki].0, None, Some(notes[ki].1)));
                    ki += 1;
                }
                stack.push((*top, Some(*mk), None));
            }
            if in_margin {
                stack.extend(notes[ki..].iter().map(|(t, k)| (*t, None, Some(*k))));
            }
            if room >= px(110.) {
                let w = room.min(px(210.));
                let mut last_bottom = px(f32::MIN);
                for (first_top, mk, k) in stack {
                    if let Some(k) = k {
                        // The note being written is a field the window draws here; its room stays.
                        let lit = comment_lit.contains(&k.id);
                        let writing = editing_comment == Some(k.id);
                        let line_h = px(theme::NOTE_LINE);
                        let color = ca(theme::comment(), if lit { 1. } else { 0.78 });
                        let text: SharedString = if k.text.is_empty() { " ".into() } else { k.text.clone().into() };
                        let run = TextRun { len: text.len(), font: font(theme::UI), color, background_color: None, underline: None, strikethrough: None };
                        let Ok(mut lines) = window.text_system().shape_text(text, px(theme::NOTE_PX), &[run], Some(w), None) else { continue };
                        lines.iter_mut().for_each(|l| {
                            crate::wrap::rewrap(l, w, None, px(0.));
                        });
                        let top = first_top.max(last_bottom + px(6.));
                        let n = lines.iter().map(|l| l.wrap_boundaries().len() + 1).sum::<usize>().max(1);
                        window.paint_quad(fill(Bounds::new(point(note_x - px(9.), top + px(1.)), size(px(2.), line_h * n as f32 - px(2.))), ca(theme::comment(), if lit { 0.9 } else { 0.42 })));
                        let mut y = top;
                        for l in lines {
                            let rows = l.wrap_boundaries().len() + 1;
                            if !writing {
                                let _ = l.paint(point(note_x, y), line_h, TextAlign::Left, None, window, cx);
                            }
                            y += line_h * rows as f32;
                        }
                        let y = y.max(top + line_h);
                        comment_rects.borrow_mut().push((k.id, Bounds::from_corners(point(note_x - px(10.), top), point(note_x + w, y)), false));
                        last_bottom = y;
                        continue;
                    }
                    let Some(mk) = mk else { continue };
                    let first_top_px = first_top;
                    let note = mk.note.clone().unwrap_or_default();
                    let color = if mk.current { c(theme::mark()) } else { ca(theme::mark(), 0.62) };
                    let run = TextRun { len: note.len(), font: font(theme::UI), color, background_color: None, underline: None, strikethrough: None };
                    let Ok(mut lines) = window.text_system().shape_text(note.into(), px(11.5), &[run], Some(w), None) else { continue };
                    lines.iter_mut().for_each(|l| {
                        crate::wrap::rewrap(l, w, None, px(0.));
                    });
                    let line_h = px(16.);
                    let top = first_top_px.max(last_bottom + px(6.));
                    let n = lines.iter().map(|l| l.wrap_boundaries().len() + 1).sum::<usize>();
                    window.paint_quad(fill(Bounds::new(point(note_x - px(9.), top + px(1.)), size(px(2.), line_h * n as f32 - px(2.))), ca(theme::mark(), if mk.current { 0.8 } else { 0.4 })));
                    let mut y = top;
                    for l in lines {
                        let rows = l.wrap_boundaries().len() + 1;
                        let _ = l.paint(point(note_x, y), line_h, TextAlign::Left, None, window, cx);
                        y += line_h * rows as f32;
                    }
                    last_bottom = y;
                }
            }
            if !in_margin {
                let x0 = layout.origin.x + layout.column_width + px(14.);
                let h = (lh * 0.66).min(px(20.));
                let mut prev: Option<(Pixels, Pixels)> = None;
                for (top, k) in &notes {
                    let x = match prev {
                        Some((t, x)) if (t - *top).abs() < px(1.) => x + px(6.),
                        _ => x0,
                    };
                    prev = Some((*top, x));
                    let lit = comment_lit.contains(&k.id);
                    let b = Bounds::new(point(x, *top + (lh - h) / 2.), size(px(2.), h));
                    window.paint_quad(fill(b, ca(theme::comment(), if lit { 1. } else { 0.6 })).corner_radii(px(1.)));
                    comment_rects.borrow_mut().push((k.id, b, true));
                }
            }

            for r in misspelled.iter() {
                let alpha = 0.85 * layout.alpha_at(r.start);
                for rect in layout.rects(r.clone(), false) {
                    let y = rect.top() + lh / 2. + layout.font_size * 0.62 + px(1.5);
                    let mut x = rect.left() + px(0.5);
                    while x + px(1.5) <= rect.right() {
                        window.paint_quad(fill(Bounds::new(point(x, y), size(px(1.6), px(1.6))), ca(theme::danger(), alpha)).corner_radii(px(0.8)));
                        x += px(3.);
                    }
                }
            }

            if let Some(r) = &in_cut {
                for rect in layout.rects(r.clone(), false) {
                    window.paint_quad(fill(rect, ca(theme::accent(), 0.06)).corner_radii(px(2.)));
                }
            }
            for (id, rect) in &marks {
                let alpha = if hovered_mark == Some(*id) { 0.95 } else { 0.55 };
                window.paint_quad(fill(*rect, ca(theme::accent(), alpha)).corner_radii(px(0.75)));
            }

            if let Some(r) = &carrying {
                for rect in layout.rects(r.clone(), false) {
                    window.paint_quad(fill(rect, ca(theme::bg(), 0.6)));
                    window.paint_quad(outline(rect.dilate(px(1.)), ca(theme::accent(), 0.7), BorderStyle::Dashed).corner_radii(px(2.)));
                }
            }
            match drop_mark {
                Some(Place::Inline(at)) => {
                    if let Some(p) = layout.point_for(at) {
                        let h = (m.font_size * 1.6).min(lh);
                        let r = Bounds::new(point(p.x - px(1.5), p.y + (lh - h) / 2.), size(px(3.), h));
                        window.paint_quad(fill(r.dilate(px(2.)), ca(theme::accent(), 0.25)).corner_radii(px(3.)));
                        window.paint_quad(fill(r, c(theme::accent())).corner_radii(px(1.5)));
                    }
                }
                Some(Place::Block(at)) => {
                    let len = layout.index.len();
                    let y = if at >= len {
                        let ends_blank = layout.index.text.ends_with('\n');
                        layout.point_for(len).map(|p| p.y + if ends_blank { lh / 2. } else { lh * 1.5 })
                    } else {
                        layout.point_for(at).map(|p| p.y - if at == 0 { px(6.) } else { lh / 2. })
                    };
                    if let Some(y) = y {
                        let x = layout.origin.x;
                        window.paint_quad(fill(Bounds::new(point(x, y - px(1.5)), size(layout.column_width, px(3.))), c(theme::accent())).corner_radii(px(1.5)));
                        paint_dot(point(x - px(6.), y), px(4.), false, c(theme::accent()), window);
                    }
                }
                None => {}
            }

            chip_rect.set(chip.as_ref().and_then(|chip| paint_chip(&layout, chip, window, cx)));

            if focused && sel.is_empty() && (caret_on || mode == Mode::Field) {
                let p = match caret_slide {
                    Some(s) if s.sliding(now) => Some(layout.origin + s.at(now)),
                    _ => layout.point_for(caret),
                };
                if let Some(p) = p {
                    let h = (m.font_size * 1.45).min(lh);
                    window.paint_quad(fill(
                        Bounds::new(point(p.x - px(1.), p.y + (lh - h) / 2.), size(px(2.), h)),
                        c(theme::accent()),
                    ));
                }
            }
        });

        if animating {
            window.request_animation_frame();
        }
    }
}

fn caret_line_is(layout: &Layout, caret: usize, line: usize) -> bool {
    layout.index.line_of(caret) == line
}

impl TextView {
    /// Re-reads the model now, for callers that just edited it and need fresh groups or offsets.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_index(cx);
    }

    pub fn centre_on(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.centre_request = Some(range);
        cx.notify();
    }

    pub fn scroll_offset(&self) -> Pixels {
        self.scroll_y
    }

    pub fn document_swapped(&mut self, sel: Range<usize>, scroll: Pixels, cx: &mut Context<Self>) {
        self.index_version = 0;
        self.sel = 0..0;
        self.reversed = false;
        self.marked = None;
        self.goal_x = None;
        self.hovered_group = None;
        self.hovered_mark = None;
        self.wheel = None;
        self.hover_char = None;
        self.drag = None;
        self.wash = None;
        self.glide = None;
        self.zen = None;
        self.zen_prev = None;
        self.zen_since = None;
        self.flash = None;
        self.fades.clear();
        self.focus_range = None;
        self.spotlight = None;
        self.centre_request = None;
        self.highlights = Default::default();
        self.current_highlight = None;
        self.misspelled = Default::default();
        self.layout = None;
        self.refresh_index(cx);
        let n = self.index.len();
        self.sel = sel.start.min(n)..sel.end.min(n);
        self.scroll_y = scroll;
        self.autoscroll = false;
        self.pause_blink();
        cx.notify();
    }

    fn refresh_index_app(&mut self, cx: &mut Context<Self>) {
        self.refresh_index(cx);
    }
}

/// Deterministic, so underlines never shimmer between frames.
fn jitter(seed: u64, i: u64) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ i.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 31;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 29;
    ((x % 1000) as f32 / 1000.) - 0.5
}

fn wavy_h(x0: Pixels, x1: Pixels, y: Pixels, seed: u64, width: Pixels) -> Option<Path<Pixels>> {
    if x1 - x0 < px(2.) {
        return None;
    }
    let mut pb = PathBuilder::stroke(width);
    let step = px(18.);
    pb.move_to(point(x0, y + px(jitter(seed, 0) * 0.4)));
    let mut x = x0;
    let mut i = 1u64;
    while x < x1 {
        let nx = (x + step).min(x1);
        let ny = y + px(jitter(seed, i) * 0.5);
        let ctrl = point((x + nx) / 2., y + px(jitter(seed, i + 1000) * 0.9));
        pb.curve_to(point(nx, ny), ctrl);
        x = nx;
        i += 1;
    }
    pb.build().ok()
}

fn wavy_v(x: Pixels, y0: Pixels, y1: Pixels, seed: u64) -> Option<Path<Pixels>> {
    if y1 - y0 < px(2.) {
        return None;
    }
    let mut pb = PathBuilder::stroke(px(1.3));
    let step = px(22.);
    pb.move_to(point(x + px(jitter(seed, 0) * 0.4), y0));
    let mut y = y0;
    let mut i = 1u64;
    while y < y1 {
        let ny = (y + step).min(y1);
        let ctrl = point(x + px(jitter(seed, i + 500) * 0.9), (y + ny) / 2.);
        pb.curve_to(point(x + px(jitter(seed, i) * 0.4), ny), ctrl);
        y = ny;
        i += 1;
    }
    pb.build().ok()
}

fn paint_dot(center: Point<Pixels>, r: Pixels, filled: bool, color: Hsla, window: &mut Window) {
    let b = Bounds::new(point(center.x - r, center.y - r), size(r * 2., r * 2.));
    if filled {
        window.paint_quad(fill(b, color).corner_radii(r));
    } else {
        window.paint_quad(
            quad(b, r, transparent_black(), px(0.9), color, BorderStyle::Solid),
        );
    }
}

fn paint_dots(start: Point<Pixels>, horizontal: bool, g: &VisibleGroup, alpha: f32, window: &mut Window) {
    let n = g.variant_count.min(9);
    let on = theme::accent();
    for i in 0..n {
        let d = DOT_PITCH * i as f32;
        let p = if horizontal { point(start.x + d, start.y) } else { point(start.x, start.y + d) };
        if i == g.active {
            paint_dot(p, DOT_R, true, ca(on, alpha), window);
        } else {
            paint_dot(p, DOT_R, false, ca(theme::muted(), 0.85 * alpha), window);
        }
    }
    if g.variant_count > 9 {
        let p = if horizontal { point(start.x + DOT_PITCH * 9., start.y) } else { point(start.x, start.y + DOT_PITCH * 9.) };
        window.paint_quad(fill(Bounds::new(point(p.x - px(1.5), p.y - px(0.4)), size(px(3.), px(0.8))), ca(theme::muted(), alpha)));
        window.paint_quad(fill(Bounds::new(point(p.x - px(0.4), p.y - px(1.5)), size(px(0.8), px(3.))), ca(theme::muted(), alpha)));
    }
}

const DOT_PITCH: Pixels = px(4.5);

fn paint_underline(layout: &Layout, g: &VisibleGroup, alpha: f32, hovered: bool, window: &mut Window) {
    let rects = layout.ink_rects(g.range.clone());
    let base = underline_offset(layout, g);
    let color = if hovered { ca(theme::accent(), 0.9 * alpha) } else { ca(theme::pen(), alpha) };
    let last_end = sentence_dots(layout, g).and_then(|(_, line_end)| line_end);
    for (i, r) in rects.iter().enumerate() {
        let y = r.top() + layout.line_height / 2. + base;
        let right = if i + 1 == rects.len() { last_end.unwrap_or(r.right()) } else { r.right() };
        if let Some(path) = wavy_h(r.left(), right, y, g.id.0, px(1.15)) {
            window.paint_path(path, color);
        }
    }
    if let Some((start, horizontal)) = dots_start(layout, g) {
        paint_dots(start, horizontal, g, alpha, window);
    }
}

/// The first dot's centre and, when the dots sit on the underline, where the line stops.
fn sentence_dots(layout: &Layout, g: &VisibleGroup) -> Option<(Point<Pixels>, Option<Pixels>)> {
    if g.scope != Scope::Sentence {
        return None;
    }
    let last = *layout.ink_rects(g.range.clone()).last()?;
    let y = last.top() + layout.line_height / 2. + underline_offset(layout, g);
    let n = g.variant_count.min(9) + usize::from(g.variant_count > 9);
    let x = last.right() - DOT_R - DOT_PITCH * n.saturating_sub(1) as f32;
    let line_end = x - DOT_R - DOT_GAP;
    Some(if line_end - last.left() >= MIN_LINE { (point(x, y), Some(line_end)) } else { (point(x, y + px(4.5)), None) })
}

const DOT_R: Pixels = px(1.4);
const DOT_GAP: Pixels = px(5.);
const MIN_LINE: Pixels = px(12.);

/// The first dot's centre, and whether the dots run across.
pub(crate) fn dots_start(layout: &Layout, g: &VisibleGroup) -> Option<(Point<Pixels>, bool)> {
    let n = g.variant_count.min(9) as f32;
    let span = DOT_PITCH * (n - 1.);
    match g.scope {
        Scope::Paragraph => {
            let (top, bottom) = layout.vertical_extent(g.range.clone())?;
            Some((point(paragraph_rule_x(layout, g) - px(8.), (top + bottom) / 2. - span / 2.), false))
        }
        Scope::Sentence => sentence_dots(layout, g).map(|(p, _)| (p, true)),
        // Nothing showing (a version without the words, as a mark): the dots sit under the spot.
        Scope::Word if g.range.is_empty() => {
            let p = layout.point_for(g.range.start)?;
            Some((point(p.x - span / 2., p.y + layout.line_height / 2. + underline_offset(layout, g) + px(4.5)), true))
        }
        Scope::Word => {
            let last = *layout.ink_rects(g.range.clone()).last()?;
            let y = last.top() + layout.line_height / 2. + underline_offset(layout, g);
            Some((point((last.left() + last.right()) / 2. - span / 2., y + px(4.5)), true))
        }
    }
}

pub(crate) fn dots_bounds(layout: &Layout, g: &VisibleGroup) -> Option<Bounds<Pixels>> {
    let (start, horizontal) = dots_start(layout, g)?;
    let n = g.variant_count.min(9) + usize::from(g.variant_count > 9);
    let span = DOT_PITCH * n.saturating_sub(1) as f32;
    let pad = px(5.);
    let (w, h) = if horizontal { (span, px(0.)) } else { (px(0.), span) };
    Some(Bounds::new(point(start.x - pad, start.y - pad), size(w + pad * 2., h + pad * 2.)))
}

fn underline_offset(layout: &Layout, g: &VisibleGroup) -> Pixels {
    layout.font_size * 0.62 + px(3.) * g.depth as f32
}

fn paragraph_rule_x(layout: &Layout, g: &VisibleGroup) -> Pixels {
    layout.origin.x - px(22.) - px(6.) * g.depth as f32
}

fn paint_paragraph_rule(layout: &Layout, g: &VisibleGroup, alpha: f32, hovered: bool, window: &mut Window) {
    let Some((top, bottom)) = layout.vertical_extent(g.range.clone()) else { return };
    let x = paragraph_rule_x(layout, g);
    let a = if hovered { 1. } else { 0.75 };
    if let Some(path) = wavy_v(x, top + px(6.), bottom - px(6.), g.id.0) {
        window.paint_path(path, ca(theme::pen(), a * alpha));
    }
    if let Some((start, vertical)) = dots_start(layout, g) {
        paint_dots(start, vertical, g, alpha, window);
    }
}

/// Drawn touching the dots, so the pointer can go text → dots → label without leaving.
fn paint_chip(layout: &Layout, chip: &(VisibleGroup, String), window: &mut Window, cx: &mut App) -> Option<Bounds<Pixels>> {
    let (g, label) = chip;
    let font_size = px(10.5);
    let run = |len: usize, hex: u32| TextRun {
        len,
        font: font(theme::UI),
        color: c(hex),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let arrow = label.rfind('›').unwrap_or(label.len());
    let mut runs = Vec::new();
    if let Some(o) = label.find(" · ").filter(|o| g.active == 0 && *o > 0 && *o <= arrow) {
        runs.extend([run(o, theme::original()), run(arrow - o, theme::text_2())]);
    } else {
        runs.push(run(arrow, theme::text_2()));
    }
    if arrow < label.len() {
        runs.push(run(label.len() - arrow, theme::accent()));
    }
    let line = window.text_system().shape_line(label.clone().into(), font_size, &runs, None);
    let (w, h) = (line.width + px(16.), px(20.));
    let dots = dots_bounds(layout, g)?;
    let left = (dots.center().x - w / 2.).min(layout.bounds.right() - w - px(4.)).max(layout.bounds.left() + px(4.));
    let below = dots.bottom() - px(2.);
    let top = if below + h > layout.bounds.bottom() - px(4.) { dots.top() - h + px(2.) } else { below };
    let b = Bounds::new(point(left, top), size(w, h));
    window.paint_quad(quad(b, px(5.), c(theme::bg_raised()), px(1.), c(theme::line()), BorderStyle::Solid));
    let _ = line.paint(point(b.left() + px(8.), b.top()), h, TextAlign::Left, None, window, cx);
    Some(b)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the gpui glob would shadow `#[test]`.
    use super::{increment_ordered, map_through_diff};
    use crate::text::TextIndex;

    #[test]
    fn diff_mapping() {
        let a = TextIndex::new("a friction here".into());
        let b = TextIndex::new("an eraser here".into());
        assert_eq!(map_through_diff(&a, &b, 0), 0);
        assert_eq!(map_through_diff(&a, &b, 15), 14);
        assert_eq!(map_through_diff(&a, &b, 11), 10);
        assert_eq!(map_through_diff(&a, &b, 5), 9);
    }

    #[test]
    fn ordered_lists_continue() {
        assert_eq!(increment_ordered("  9. "), "  10. ");
        assert_eq!(increment_ordered("- "), "- ");
    }
}
