//! Document model for alternatives and cuts: groups own their variants, so switching never
//! moves anything; edits use display-char offsets with plain-string splice semantics.

pub mod snapshot;
mod tree;

use std::collections::HashSet;
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::Arc;

use snapshot::{DocTree, SnapshotError, TreeNote};
use tree::{Container, Cut, Node, Payload, Variant};
pub use tree::{Batch, CommentId, CutId, GroupId, Look, Scope, Source, VariantId};

/// A place in the final text (chars) that had other versions, and their text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AltNote {
    pub range: std::ops::Range<usize>,
    pub others: Vec<String>,
}

const MAX_UNDO: usize = 1000;

/// Shared by every document, so a window holding two can tell whose undo step came last.
static CLOCK: AtomicU64 = AtomicU64::new(1);

fn tick() -> u64 {
    CLOCK.fetch_add(1, Ordering::Relaxed)
}

pub fn new_link() -> u64 {
    tick()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepTag {
    /// When the step was made or last extended, on the clock all documents share.
    pub seq: u64,
    /// Set when the step is one half of a change spanning two documents.
    pub link: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleGroup {
    pub id: GroupId,
    pub scope: Scope,
    pub range: Range<usize>,
    /// 0 for top-level groups, 1 for groups inside a visible variant, …
    pub depth: usize,
    pub active: usize,
    pub variant_count: usize,
    /// Inside a shown cut: drawn at the cut opacity.
    pub ghosted: bool,
    /// The version showing is the words cut ([`Document::add_cut_version`]): the page draws the
    /// cut, with the group's dots and no underline.
    pub cut: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupInfo {
    pub id: GroupId,
    pub scope: Scope,
    pub active: usize,
    pub ghosted: bool,
    /// Index 0 is the original.
    pub variants: Vec<VariantInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantInfo {
    pub id: VariantId,
    pub by: Source,
    /// Display text of the variant (nested groups show their active variant). For a version
    /// that is the words cut, the cut's text.
    pub text: String,
    /// The version is the words cut: a version without them ([`Document::add_cut_version`]).
    pub cut: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycled {
    pub active: usize,
    pub is_original: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removed {
    pub text: String,
    /// The group was down to one variant and dissolved into plain text.
    pub dissolved: bool,
}

/// A scrap from another document's drawer is removed there by the app itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Carried {
    Page(Range<usize>),
    Note(usize),
    /// A cut, wherever it is (a trace on the page or a note): moving it removes it.
    Cut(CutId),
    Fragment(Fragment),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presentation {
    /// Editing (traces show) or writing (only fresh cuts show).
    pub editing: bool,
    /// The setting, for cuts with no look of their own.
    pub look: Look,
}

impl Default for Presentation {
    fn default() -> Self {
        Presentation { editing: true, look: Look::Text }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CutPlace {
    /// Its display range (empty when it takes no room), and whether its text shows.
    Page { range: Range<usize>, shown: bool },
    /// In a version that isn't showing; `at` is where that version's group is on the page.
    Hidden { at: usize },
    /// A note: no spot (no trace, or its place is gone).
    Note(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutInfo {
    pub id: CutId,
    /// Its text, without the blank line a paragraph cut takes with it.
    pub text: String,
    pub place: CutPlace,
    /// `None` follows the setting.
    pub look: Option<Look>,
    pub paragraph: bool,
    pub batch: Option<Batch>,
    /// Made since the last presentation change (shows faded whatever the setting).
    pub fresh: bool,
    pub lost: bool,
    /// For notes: the start of the paragraph it came from, and its file (a project's cuts).
    pub origin: Option<String>,
    pub source: Option<String>,
    /// The comments it took with it, their text in order.
    pub comments: Vec<String>,
    /// The cut is the whole of a version of this group (a version without the words).
    pub version: Option<GroupId>,
}

impl CutInfo {
    /// In the Cuts panel: every cut but a version without the words that isn't showing (that
    /// one is a version, not a cut).
    pub fn listed(&self) -> bool {
        !(self.version.is_some() && matches!(self.place, CutPlace::Hidden { .. }))
    }
}

/// Comments as they fall in the final text: their range there, and the comment.
pub type CleanComments = Vec<(Range<usize>, CommentInfo)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentInfo {
    pub id: CommentId,
    /// Empty only when its text is a cut that takes no room.
    pub range: Range<usize>,
    pub text: String,
    /// When it was written (RFC 3339), if known.
    pub created: Option<String>,
    /// `None` is the writer.
    pub author: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceInfo {
    pub id: CutId,
    /// Empty for a point.
    pub range: Range<usize>,
    pub shown: bool,
    pub look: Option<Look>,
    pub paragraph: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Between words at this offset; the spaces around it are fixed.
    Inline(usize),
    /// Its own paragraph before the one starting at this offset; the page's length is the end.
    Block(usize),
}

/// Punctuation that hangs on the word before it: no space goes in front of it.
fn closes(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '”' | '’' | '…'
    )
}

/// Punctuation that hangs on the word after it: no space goes after it.
fn opens(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '“' | '‘')
}

fn trim_range(t: &[char], r: Range<usize>) -> Range<usize> {
    let mut r = r;
    while r.start < r.end && t[r.start].is_whitespace() {
        r.start += 1;
    }
    while r.end > r.start && t[r.end - 1].is_whitespace() {
        r.end -= 1;
    }
    r
}

fn line_before(t: &[char], nl: usize) -> &[char] {
    let start = t[..nl]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |i| i + 1);
    &t[start..nl]
}

/// Paragraphs are runs of non-blank lines.
fn whole_paragraphs(t: &[char], r: &Range<usize>) -> bool {
    if r.is_empty() || t[r.start].is_whitespace() {
        return false;
    }
    let blank = |s: &[char]| s.iter().all(|c| c.is_whitespace());
    let starts = r.start == 0
        || (t[r.start - 1] == '\n' && (r.start == 1 || blank(line_before(t, r.start - 1))));
    let ends = r.end == t.len() || {
        let rest = &t[r.end..];
        let next_line_end = rest
            .iter()
            .skip(1)
            .position(|c| *c == '\n')
            .map_or(rest.len(), |i| i + 1);
        rest[0] == '\n' && blank(&rest[1..next_line_end])
    };
    starts && ends
}

/// The paragraphs and the blank lines after them, or, for the last paragraphs, before them.
fn paragraph_cut(t: &[char], r: Range<usize>) -> Range<usize> {
    let after = t[r.end..].iter().position(|c| !c.is_whitespace());
    match after {
        Some(k) => r.start..r.end + k,
        None => {
            let start = t[..r.start]
                .iter()
                .rposition(|c| !c.is_whitespace())
                .map_or(r.start, |i| i + 1);
            start..r.end
        }
    }
}

/// If trimmed `range` is whole paragraphs, the stretch a cut of them takes, blank lines included.
pub fn paragraph_span(text: &[char], range: Range<usize>) -> Option<Range<usize>> {
    if range.end > text.len() {
        return None;
    }
    let r = trim_range(text, range);
    whole_paragraphs(text, &r).then(|| paragraph_cut(text, r))
}

/// Its groups, and its cuts only when moved rather than copied.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Fragment {
    nodes: Vec<Node>,
}

impl Fragment {
    pub fn plain_text(&self) -> String {
        Container::from_nodes(self.nodes.clone()).text()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Spaces are fixed where it lands.
    pub fn trimmed(&self) -> Fragment {
        let mut c = Container::from_nodes(self.nodes.clone());
        let t = c.text();
        let trail = t.chars().rev().take_while(|c| c.is_whitespace()).count();
        let lead = t.chars().take_while(|c| c.is_whitespace()).count();
        if lead < c.len {
            let len = c.len;
            c.replace(len - trail, len, "");
            c.replace(0, lead, "");
        }
        Fragment { nodes: c.nodes }
    }
}

/// Text taken from the page for another document's drawer (a project's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scrap {
    pub fragment: Fragment,
    /// As [`NoteInfo::origin`].
    pub origin: Option<String>,
    /// As [`NoteInfo::paragraph`].
    pub paragraph: bool,
    /// As [`NoteInfo::lost`], with the look its trace had.
    pub lost: bool,
    pub look: Option<Look>,
    pub batch: Option<Batch>,
}

/// A note in the Overflow drawer (a scrap, in the app).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteInfo {
    pub id: CutId,
    pub text: String,
    /// Relative to the project, when the drawer is a project's rather than its file's.
    pub source: Option<String>,
    /// Start of the paragraph it was stashed from (up to 24 chars), for the `↩ ¶n` link.
    pub origin: Option<String>,
    /// Stashed as whole paragraphs; only set with an `origin` (the file keeps it on that record).
    pub paragraph: bool,
    /// Its spot is gone: its surroundings were deleted, or it couldn't be found in the file.
    pub lost: bool,
    /// The look its trace had, kept for a lost cut.
    pub look: Option<Look>,
    pub batch: Option<Batch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    OutOfBounds,
    EmptyRange,
    EmptyText,
    PartialOverlap,
    /// The range is exactly an existing group; open that one instead.
    AlreadyGroup(GroupId),
    NoSuchGroup,
    NoSuchVariant,
    LastVariant,
    NoSuchNote,
    NoSuchCut,
    NoSuchComment,
    /// The range is inside a cut's trace (put the cut back instead).
    InCut,
    /// The operation would change nothing (dropping text onto itself).
    Unchanged,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            EditError::OutOfBounds => "range is outside the document",
            EditError::EmptyRange => "range is empty",
            EditError::Unchanged => "nothing would change",
            EditError::EmptyText => "text is empty",
            EditError::PartialOverlap => "selection partly overlaps other alternatives",
            EditError::AlreadyGroup(_) => "selection already has alternatives",
            EditError::NoSuchGroup => "no such alternative group",
            EditError::NoSuchVariant => "no such variant",
            EditError::LastVariant => "a group needs at least one variant",
            EditError::NoSuchNote => "no such overflow note",
            EditError::NoSuchCut => "no such cut",
            EditError::NoSuchComment => "no such comment",
            EditError::InCut => "range is inside a cut",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for EditError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UndoKey {
    Typing,
    Cycle(GroupId),
    /// AI ideas arriving one by one for a group: one undo step removes them all.
    Ideas(GroupId),
    /// Adding a comment and typing in it are one step.
    Comment(CommentId),
}

/// A cut with no spot: a scrap, a cut with no trace, or one whose place is gone.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Note {
    id: CutId,
    content: Container,
    origin: Option<String>,
    paragraph: bool,
    source: Option<String>,
    lost: bool,
    look: Option<Look>,
    batch: Option<Arc<Batch>>,
}

impl Note {
    fn new(id: CutId, content: Container) -> Note {
        Note { id, content, origin: None, paragraph: false, source: None, lost: false, look: None, batch: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct State {
    page: Container,
    notes: Vec<Note>,
}

struct Entry {
    state: State,
    seq: u64,
    link: Option<u64>,
}

impl Entry {
    fn tag(&self) -> StepTag {
        StepTag { seq: self.seq, link: self.link }
    }
}

/// Page changes in order; each one's display offsets are in the text right before it, so a
/// view can replay them over its cache. A change may be wider than its edit, never narrower.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Styling in `old` may have changed too; outside it only offsets shift.
    Text { old: Range<usize>, new_len: usize },
    /// Same text, different styling in this range.
    Style(Range<usize>),
    /// Overflow notes changed (text, groups or order).
    Notes,
    /// Re-read everything; emitted instead of a very long backlog.
    Reset,
}

/// Pending changes beyond this collapse into one [`Change::Reset`].
const MAX_CHANGES: usize = 4096;

pub struct Document {
    state: State,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    last_key: Option<UndoKey>,
    next_id: u64,
    changes: Vec<Change>,
    presentation: Presentation,
    /// When the presentation last changed (the clock); cuts made after it are fresh.
    epoch: u64,
    /// The `source` of notes this document's cuts become (a project's chapter: its file).
    cut_source: Option<String>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self::from_text("")
    }

    pub fn from_text(text: &str) -> Self {
        Document {
            state: State {
                page: Container::from_text(text),
                notes: Vec::new(),
            },
            undo: Vec::new(),
            redo: Vec::new(),
            last_key: None,
            next_id: 0,
            changes: Vec::new(),
            presentation: Presentation::default(),
            epoch: 0,
            cut_source: None,
        }
    }

    /// Empty undo history; group ids are kept when unique, variant ids are always fresh.
    pub fn from_snapshot(tree: DocTree) -> Result<Self, SnapshotError> {
        let mut b = snapshot::Builder::new(&tree);
        let page = b.container(&tree.page, true)?;
        let mut notes = Vec::with_capacity(tree.notes.len());
        for n in &tree.notes {
            notes.push(Note {
                id: CutId(b.alloc()),
                content: b.container(&n.content, false)?,
                origin: n.origin.clone(),
                paragraph: n.paragraph,
                source: n.source.clone(),
                lost: n.lost,
                look: n.look,
                batch: n.batch.clone().map(Arc::new),
            });
        }
        let mut d = Self::new();
        d.state = State { page, notes };
        d.next_id = b.next_id;
        d.settle_comments();
        d.apply_presentation();
        Ok(d)
    }

    /// `Document::from_snapshot(d.snapshot())` reproduces `d` except for variant ids and undo.
    pub fn snapshot(&self) -> DocTree {
        let mut nums = snapshot::CommentNumbers::default();
        DocTree {
            page: snapshot::from_container(&self.state.page, &mut nums),
            notes: self
                .state
                .notes
                .iter()
                .map(|n| TreeNote {
                    content: snapshot::from_container(&n.content, &mut nums),
                    origin: n.origin.clone(),
                    paragraph: n.paragraph,
                    source: n.source.clone(),
                    lost: n.lost,
                    look: n.look,
                    batch: n.batch.as_deref().cloned(),
                })
                .collect(),
        }
    }

    /// The page as displayed: active variants, shown cuts' text included.
    pub fn text(&self) -> String {
        self.state.page.text()
    }

    /// Clamped to the page; doesn't build the whole page.
    pub fn text_range(&self, range: Range<usize>) -> String {
        let mut s = String::new();
        let e = range.end.min(self.len());
        self.state.page.text_range(range.start.min(e), e, &mut s);
        s
    }

    /// `None` when it isn't on the page (inactive variant, hidden cut, Overflow note).
    pub fn group_range(&self, id: GroupId) -> Option<Range<usize>> {
        self.state.page.visible_range(id, 0)
    }

    /// A new or loaded document starts with none: lay it out from scratch.
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    pub fn len(&self) -> usize {
        self.state.page.len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Active variants, cuts left out; where a cut was, whitespace is tidied (one blank line
    /// between blocks, one space in a line, none before punctuation). Elsewhere it is as written.
    pub fn clean_text(&self) -> String {
        self.clean_text_with_notes().0
    }

    /// [`Self::clean_text`] and where in it (chars) the places with other versions are.
    pub fn clean_text_with_notes(&self) -> (String, Vec<AltNote>) {
        let (text, notes, _) = self.clean_parts();
        (text, notes)
    }

    fn clean_parts(&self) -> (String, Vec<AltNote>, CleanComments) {
        const CUT: char = '\u{0}';
        // Private-use chars numbered by comment, so overlapping comments come out right.
        const COMMENT: u32 = 0xF0000;
        let comment_mark = |k: usize, end: bool| char::from_u32(COMMENT + 2 * k as u32 + u32::from(end)).unwrap_or('\u{F8F2}');
        let is_comment = |c: char| (c as u32) >= COMMENT;
        let mut comments: Vec<CommentInfo> = Vec::new();
        // Private-use marks ride through the tidying as ordinary chars and come out as ranges.
        const OPEN: char = '\u{F8F0}';
        const CLOSE: char = '\u{F8F1}';
        let is_mark = |c: char| c == OPEN || c == CLOSE || is_comment(c);
        let mut s = String::new();
        let mut others: Vec<Vec<String>> = Vec::new();
        let mut ends_with_newline = false;
        self.state.page.pieces_marked(false, &mut |p| match p {
            tree::Piece::Text(t, gh) => {
                // The final newline is the kept text's: a cut's text isn't in the document.
                if !t.is_empty() && !gh {
                    ends_with_newline = t.ends_with('\n');
                }
                if !gh {
                    s.push_str(t)
                } else if !t.is_empty() && !s.ends_with(CUT) {
                    s.push(CUT);
                }
            }
            tree::Piece::Point => {
                if !s.ends_with(CUT) {
                    s.push(CUT);
                }
            }
            tree::Piece::Open(o) => {
                others.push(o);
                s.push(OPEN);
            }
            tree::Piece::Close => s.push(CLOSE),
            tree::Piece::CommentStart(c) => {
                s.push(comment_mark(comments.len(), false));
                comments.push(CommentInfo { id: c.id, range: 0..0, text: c.text.clone(), created: c.created.clone(), author: c.author.clone() });
            }
            tree::Piece::CommentEnd(id) => {
                if let Some(k) = comments.iter().position(|c| c.id == id) {
                    s.push(comment_mark(k, true));
                }
            }
        });
        let tidied = if !s.contains(CUT) {
            s
        } else {
            let chars: Vec<char> = s.chars().collect();
            let mut out = String::with_capacity(s.len());
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                if c != CUT && !c.is_whitespace() {
                    out.push(c);
                    i += 1;
                    continue;
                }
                let start = i;
                // A comment's marks don't break a run: ends go before the tidied space, starts after.
                while i < chars.len() && (chars[i] == CUT || chars[i].is_whitespace() || is_comment(chars[i])) {
                    i += 1;
                }
                let with_marks = &chars[start..i];
                if !with_marks.contains(&CUT) {
                    out.extend(with_marks);
                    continue;
                }
                let run: Vec<char> = with_marks.iter().copied().filter(|&c| !is_comment(c)).collect();
                let run = &run[..];
                let marks: Vec<char> = with_marks.iter().copied().filter(|&c| is_comment(c)).collect();
                let (ends, starts): (Vec<char>, Vec<char>) = marks.into_iter().partition(|&c| (c as u32 - COMMENT) % 2 == 1);
                out.extend(ends);
                let tail_starts = starts;
                let newlines = run.iter().filter(|&&c| c == '\n').count();
                let at_start = out.chars().all(is_mark);
                let at_end = chars[i..].iter().all(|&c| is_mark(c) || c == CUT || c.is_whitespace());
                let next = chars[i..].iter().copied().find(|&c| !is_mark(c));
                if at_end {
                    if ends_with_newline && !at_start {
                        out.push('\n');
                    }
                } else if at_start {
                } else if newlines >= 2 {
                    out.push_str("\n\n");
                } else if newlines == 1 {
                    out.push('\n');
                } else if run.iter().any(|c| c.is_whitespace()) && !next.is_some_and(|n| ".,;:!?)]}”’".contains(n)) {
                    out.push(' ');
                }
                out.extend(tail_starts);
            }
            out
        };
        if others.is_empty() && comments.is_empty() {
            return (tidied, Vec::new(), Vec::new());
        }
        let mut text = String::with_capacity(tidied.len());
        let mut notes = Vec::new();
        let mut open: Vec<(usize, usize)> = Vec::new();
        let mut spans: Vec<Option<Range<usize>>> = vec![None; comments.len()];
        let (mut at, mut next) = (0usize, 0usize);
        for c in tidied.chars() {
            match c {
                c if is_comment(c) => {
                    let n = (c as u32 - COMMENT) as usize;
                    if let Some(sp) = spans.get_mut(n / 2) {
                        if n.is_multiple_of(2) {
                            *sp = Some(at..at);
                        } else if let Some(r) = sp {
                            r.end = at;
                        }
                    }
                }
                OPEN => {
                    open.push((at, next));
                    next += 1;
                }
                CLOSE => {
                    if let Some((start, k)) = open.pop() {
                        if at > start {
                            notes.push(AltNote { range: start..at, others: std::mem::take(&mut others[k]) });
                        }
                    }
                }
                c => {
                    text.push(c);
                    at += 1;
                }
            }
        }
        notes.sort_by_key(|n| (n.range.start, std::cmp::Reverse(n.range.end)));
        let found = comments
            .into_iter()
            .zip(spans)
            .filter_map(|(c, r)| r.filter(|r| !r.is_empty()).map(|r| (r, c)))
            .collect();
        (text, notes, found)
    }

    /// Cuts whose text shows, merged and sorted.
    pub fn ghost_ranges(&self) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for t in self.traces() {
            if !t.shown {
                continue;
            }
            match out.last_mut() {
                Some(r) if r.end == t.range.start => r.end = t.range.end,
                _ => out.push(t.range),
            }
        }
        out
    }

    /// The shown cut's stretch under the caret (edges included).
    pub fn ghost_range_at(&self, pos: usize) -> Option<Range<usize>> {
        self.ghost_ranges()
            .into_iter()
            .find(|r| r.start <= pos && pos <= r.end)
    }

    /// Shown cuts with their range and points (empty ranges); hidden versions' cuts left out.
    pub fn traces(&self) -> Vec<TraceInfo> {
        let mut all = Vec::new();
        self.state.page.collect_cuts(0, true, &mut all);
        all.into_iter()
            .filter(|(_, _, visible)| *visible)
            .map(|(c, range, _)| TraceInfo { id: c.id, range, shown: c.shown, look: c.look, paragraph: c.paragraph })
            .collect()
    }

    /// A shown cut with `pos` inside or at an edge, or a point at `pos`; a point wins.
    pub fn cut_at(&self, pos: usize) -> Option<CutId> {
        let traces = self.traces();
        traces
            .iter()
            .find(|t| !t.shown && t.range.start == pos)
            .or_else(|| traces.iter().find(|t| t.shown && t.range.start <= pos && pos <= t.range.end))
            .map(|t| t.id)
    }

    /// The cuts whose shown text overlaps `range` (or, for an empty range, holds it).
    pub fn cuts_in(&self, range: Range<usize>) -> Vec<CutId> {
        self.traces()
            .into_iter()
            .filter(|t| {
                if range.is_empty() {
                    t.range.start <= range.start && range.start <= t.range.end
                } else if t.range.is_empty() {
                    range.start < t.range.start && t.range.start < range.end
                } else {
                    t.range.start < range.end && range.start < t.range.end
                }
            })
            .map(|t| t.id)
            .collect()
    }

    /// Cuts with a spot in page order (hidden versions too), then the notes.
    pub fn cuts(&self) -> Vec<CutInfo> {
        let mut all = Vec::new();
        self.state.page.collect_cuts(0, true, &mut all);
        let mut versions = Vec::new();
        self.state.page.collect_cut_versions(&mut versions);
        let version_of = |id: CutId| versions.iter().find(|(c, _, _)| *c == id).map(|(_, g, _)| *g);
        let mut out: Vec<CutInfo> = all
            .into_iter()
            .map(|(c, range, visible)| CutInfo {
                id: c.id,
                text: cut_text(&c.content, c.paragraph),
                place: if visible {
                    CutPlace::Page { range, shown: c.shown }
                } else {
                    CutPlace::Hidden { at: range.start }
                },
                look: c.look,
                paragraph: c.paragraph,
                batch: c.batch.as_deref().cloned(),
                fresh: c.made > self.epoch,
                lost: false,
                origin: None,
                source: None,
                comments: comment_texts(&c.content),
                version: version_of(c.id),
            })
            .collect();
        out.extend(self.state.notes.iter().enumerate().map(|(i, n)| CutInfo {
            id: n.id,
            text: n.content.text(),
            place: CutPlace::Note(i),
            look: n.look,
            paragraph: n.paragraph,
            batch: n.batch.as_deref().cloned(),
            fresh: false,
            lost: n.lost,
            origin: n.origin.clone(),
            source: n.source.clone(),
            comments: comment_texts(&n.content),
            version: None,
        }));
        out
    }

    /// In start order; comments in a hidden version or a cut aren't on the page.
    pub fn comments(&self) -> Vec<CommentInfo> {
        let mut found = Vec::new();
        self.state.page.collect_comments(0, &mut found);
        found
            .into_iter()
            .map(|(c, range)| CommentInfo { id: c.id, range, text: c.text.clone(), created: c.created.clone(), author: c.author.clone() })
            .collect()
    }

    pub fn has_comments(&self) -> bool {
        self.state.page.has_comments()
    }

    /// `range` must be non-empty; the comment grows to hold a group a marker would split.
    /// `created` is RFC 3339. One undo step.
    pub fn add_comment(&mut self, range: Range<usize>, text: &str, created: Option<String>) -> Result<CommentId, EditError> {
        self.add_comment_by(range, text, created, None)
    }

    pub fn add_comment_by(&mut self, range: Range<usize>, text: &str, created: Option<String>, author: Option<String>) -> Result<CommentId, EditError> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Err(EditError::EmptyRange);
        }
        let id = CommentId(self.alloc_id());
        let comment = tree::Comment { id, text: text.to_string(), created, author };
        self.last_key = None;
        self.transact(Some(UndoKey::Comment(id)), |d| {
            d.state.page.add_comment(range.start, range.end, Node::CommentStart(Arc::new(comment)), Node::CommentEnd(id))?;
            d.settle_comments();
            // All of it was the text of a cut that shows, which the comment leaves.
            if d.state.page.find_comment(id).is_none() {
                return Err(EditError::InCut);
            }
            Ok(id)
        })
    }

    /// The comment's text, wherever it is (on the page, in a version, in a cut).
    pub fn comment_text(&self, id: CommentId) -> Option<String> {
        self.find_comment(id).map(|c| c.text.clone())
    }

    pub fn set_comment_text(&mut self, id: CommentId, text: &str) -> Result<(), EditError> {
        self.transact(None, |d| {
            let mut set = |c: &mut tree::Comment| c.text = text.to_string();
            if d.state.page.modify_comment(id, &mut set) || d.state.notes.iter_mut().any(|n| n.content.modify_comment(id, &mut set)) {
                Ok(())
            } else {
                Err(EditError::NoSuchComment)
            }
        })
    }

    /// `range` in chars of the comment's text; one undo step with adding it and earlier typing.
    pub fn replace_in_comment(&mut self, id: CommentId, range: Range<usize>, text: &str) -> Result<(), EditError> {
        let old = self.comment_text(id).ok_or(EditError::NoSuchComment)?;
        let chars: Vec<char> = old.chars().collect();
        if range.start > range.end || range.end > chars.len() {
            return Err(EditError::OutOfBounds);
        }
        let new: String = chars[..range.start].iter().copied().chain(text.chars()).chain(chars[range.end..].iter().copied()).collect();
        self.transact(Some(UndoKey::Comment(id)), |d| {
            let mut set = |c: &mut tree::Comment| c.text = new.clone();
            if d.state.page.modify_comment(id, &mut set) || d.state.notes.iter_mut().any(|n| n.content.modify_comment(id, &mut set)) {
                Ok(())
            } else {
                Err(EditError::NoSuchComment)
            }
        })
    }

    /// A comment left empty goes; straight after adding it, that leaves no undo step at all.
    pub fn finish_comment(&mut self, id: CommentId) {
        if self.comment_text(id).is_some_and(|t| t.trim().is_empty()) {
            let _ = self.transact(Some(UndoKey::Comment(id)), |d| {
                d.state.page.remove_comment(id);
                d.state.page.prune_empty();
                Ok(())
            });
        }
        self.last_key = None;
    }

    /// The page's comments as they fall in [`Self::clean_text`], in start order.
    pub fn clean_comments(&self) -> CleanComments {
        self.clean_parts().2
    }

    /// The text it is on stays.
    pub fn delete_comment(&mut self, id: CommentId) -> Result<(), EditError> {
        self.transact(None, |d| {
            if d.state.page.remove_comment(id) || d.state.notes.iter_mut().any(|n| n.content.remove_comment(id)) {
                d.state.page.prune_empty();
                Ok(())
            } else {
                Err(EditError::NoSuchComment)
            }
        })
    }

    /// Hidden versions' comments too, not those in cuts. One undo step; returns how many.
    pub fn delete_page_comments(&mut self) -> Result<usize, EditError> {
        self.transact(None, |d| {
            let n = d.state.page.remove_all_comments();
            d.state.page.prune_empty();
            if n == 0 {
                Err(EditError::NoSuchComment)
            } else {
                Ok(n)
            }
        })
    }

    fn find_comment(&self, id: CommentId) -> Option<&tree::Comment> {
        self.state.page.find_comment(id).or_else(|| self.state.notes.iter().find_map(|n| n.content.find_comment(id)))
    }

    /// After every change: each comment's markers are partners in one container with text
    /// between them; a marker left without a partner at the top goes.
    fn settle_comments(&mut self) {
        if !self.state.page.comments_settled() {
            self.state.page.settle_comments();
            self.state.page.prune_empty();
        }
        for n in &mut self.state.notes {
            if !n.content.comments_settled() {
                n.content.settle_comments();
            }
        }
    }

    /// [`Self::cuts`] the Cuts panel lists ([`CutInfo::listed`]).
    pub fn listed_cuts(&self) -> Vec<CutInfo> {
        self.cuts().into_iter().filter(CutInfo::listed).collect()
    }

    pub fn cut_info(&self, id: CutId) -> Option<CutInfo> {
        self.cuts().into_iter().find(|c| c.id == id)
    }

    /// A paragraph cut without its blank line.
    pub fn cut_fragment(&self, id: CutId) -> Result<Fragment, EditError> {
        if let Some(c) = self.state.page.find_cut(id) {
            return Ok(Fragment { nodes: note_content(c).nodes });
        }
        let n = self.state.notes.iter().find(|n| n.id == id).ok_or(EditError::NoSuchCut)?;
        Ok(Fragment { nodes: n.content.nodes.clone() })
    }

    pub fn presentation(&self) -> Presentation {
        self.presentation
    }

    /// The `source` this document's cuts get when they become notes (a project's chapter: its
    /// file), so they stay the project's. Not an undo step.
    pub fn set_cut_source(&mut self, source: Option<String>) {
        self.cut_source = source;
    }

    pub fn cut_source(&self) -> Option<&str> {
        self.cut_source.as_deref()
    }

    /// Groups on the page in document order (outer before inner).
    pub fn visible_groups(&self) -> Vec<VisibleGroup> {
        let mut out = Vec::new();
        self.state.page.collect_visible(0, 0, false, &mut out);
        out
    }

    /// Including inside inactive variants, cuts and Overflow notes.
    pub fn all_group_ids(&self) -> Vec<GroupId> {
        let mut out = Vec::new();
        self.state.page.collect_group_ids(&mut out);
        for n in &self.state.notes {
            n.content.collect_group_ids(&mut out);
        }
        out
    }

    pub fn group(&self, id: GroupId) -> Option<GroupInfo> {
        let g = self.find_group(id)?;
        let mut cuts = Vec::new();
        self.state.page.collect_cuts(0, true, &mut cuts);
        let ghosted = cuts.iter().any(|(c, _, _)| c.content.contains_group(id))
            || self.state.notes.iter().any(|n| n.content.contains_group(id));
        Some(GroupInfo {
            id: g.id,
            scope: g.scope,
            active: g.active,
            ghosted,
            variants: g
                .variants
                .iter()
                .map(|v| match v.words_cut() {
                    Some(c) => VariantInfo { id: v.id, by: v.by, text: c.content.text(), cut: true },
                    None => VariantInfo { id: v.id, by: v.by, text: v.content.text(), cut: false },
                })
                .collect(),
        })
    }

    pub fn notes(&self) -> Vec<NoteInfo> {
        self.state
            .notes
            .iter()
            .map(|n| NoteInfo {
                id: n.id,
                text: n.content.text(),
                source: n.source.clone(),
                origin: n.origin.clone(),
                paragraph: n.paragraph,
                lost: n.lost,
                look: n.look,
                batch: n.batch.as_deref().cloned(),
            })
            .collect()
    }

    /// Afterwards [`Self::text`] equals the same splice on a plain string; a cut whose spot is
    /// deleted becomes a note (*place gone*).
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), EditError> {
        self.check_range(&range)?;
        self.transact(Some(UndoKey::Typing), |d| {
            let evicted = d.state.page.replace(range.start, range.end, text);
            d.evict(evicted, range.start);
            Ok(())
        })
    }

    pub fn replace_in_note(
        &mut self,
        note: usize,
        range: Range<usize>,
        text: &str,
    ) -> Result<(), EditError> {
        let n = self.state.notes.get(note).ok_or(EditError::NoSuchNote)?;
        if range.start > range.end || range.end > n.content.len {
            return Err(EditError::OutOfBounds);
        }
        self.transact(Some(UndoKey::Typing), |d| {
            d.state.notes[note]
                .content
                .replace(range.start, range.end, text);
            Ok(())
        })
    }

    /// `range` is in [`CutInfo::text`]; a cut can't be emptied this way.
    pub fn replace_in_cut(&mut self, id: CutId, range: Range<usize>, text: &str) -> Result<(), EditError> {
        if let Some(i) = self.state.notes.iter().position(|n| n.id == id) {
            return self.replace_in_note(i, range, text);
        }
        let c = self.state.page.find_cut(id).ok_or(EditError::NoSuchCut)?;
        let lead = if c.paragraph && !c.content.text().trim().is_empty() { leading_ws(&c.content) } else { 0 };
        let len = cut_text(&c.content, c.paragraph).chars().count();
        if range.start > range.end || range.end > len {
            return Err(EditError::OutOfBounds);
        }
        if text.is_empty() && range.len() == len {
            return Err(EditError::EmptyText);
        }
        self.transact(Some(UndoKey::Typing), |d| {
            d.state.page.modify_cut(id, &mut |c| {
                c.content.replace(range.start + lead, range.end + lead, text);
            });
            Ok(())
        })
    }

    /// The new group's only variant (the original) is the current text; whole groups inside nest.
    pub fn create_group(
        &mut self,
        range: Range<usize>,
        scope: Scope,
    ) -> Result<GroupId, EditError> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Err(EditError::EmptyRange);
        }
        let gid = GroupId(self.alloc_id());
        let vid = VariantId(self.alloc_id());
        self.transact(None, |d| {
            wrap(&mut d.state.page, range.start, range.end, scope, gid, vid)?;
            Ok(gid)
        })
    }

    /// It does not become active.
    pub fn add_variant(
        &mut self,
        id: GroupId,
        text: &str,
        by: Source,
    ) -> Result<VariantId, EditError> {
        if text.is_empty() {
            return Err(EditError::EmptyText);
        }
        let vid = VariantId(self.alloc_id());
        self.transact(None, |d| {
            d.modify_group(id, &mut |g| {
                g.variants.push(Variant {
                    without: false,
                    id: vid,
                    by,
                    content: Container::from_text(text),
                })
            })?;
            Ok(vid)
        })
    }

    /// Not shown. Ideas added in a row for one group are one undo step, so ⌘Z removes a batch.
    pub fn add_idea(&mut self, id: GroupId, text: &str) -> Result<VariantId, EditError> {
        if text.is_empty() {
            return Err(EditError::EmptyText);
        }
        let vid = VariantId(self.alloc_id());
        self.transact(Some(UndoKey::Ideas(id)), |d| {
            d.modify_group(id, &mut |g| {
                g.variants.push(Variant {
                    without: false,
                    id: vid,
                    by: Source::Ai,
                    content: Container::from_text(text),
                })
            })?;
            Ok(vid)
        })
    }

    /// A version without the words: the original's text, cut. While it shows it is a cut like
    /// any other (faded or a mark, left out of the final text, in the Cuts panel); settling on it
    /// leaves an ordinary cut. One per word group on the page (not in a cut or a note).
    pub fn add_cut_version(&mut self, id: GroupId, by: Source) -> Result<VariantId, EditError> {
        self.push_cut_version(id, by, None, false)
    }

    /// [`Document::add_cut_version`], shown, in one undo step.
    pub fn add_cut_version_and_show(&mut self, id: GroupId, by: Source) -> Result<VariantId, EditError> {
        self.push_cut_version(id, by, None, true)
    }

    /// [`Document::add_cut_version`] for an AI idea: one undo step with the ideas around it.
    pub fn add_cut_idea(&mut self, id: GroupId) -> Result<VariantId, EditError> {
        self.push_cut_version(id, Source::Ai, Some(UndoKey::Ideas(id)), false)
    }

    fn push_cut_version(&mut self, id: GroupId, by: Source, key: Option<UndoKey>, show: bool) -> Result<VariantId, EditError> {
        let g = self.state.page.find_group(id).ok_or(EditError::NoSuchGroup)?;
        // Words and phrases only: a sentence or a paragraph goes by Trim or ⌘X.
        if g.scope != Scope::Word || g.variants.iter().any(|v| v.words_cut().is_some()) {
            return Err(EditError::Unchanged);
        }
        let text = g.variants[0].content.kept_text();
        if text.trim().is_empty() {
            return Err(EditError::EmptyText);
        }
        if self.group(id).is_some_and(|g| g.ghosted) {
            return Err(EditError::InCut);
        }
        let vid = VariantId(self.alloc_id());
        let mut cut = Cut { id: CutId(self.alloc_id()), content: Container::from_text(&text), shown: false, look: None, made: 0, paragraph: false, batch: None };
        cut.shown = self.wants_shown(&cut);
        let cut = Arc::new(cut);
        self.transact(key, |d| {
            d.modify_group(id, &mut |g| {
                g.variants.push(Variant { id: vid, by, content: Container::from_nodes(vec![Node::Cut(cut.clone())]), without: true });
                if show {
                    g.active = g.variants.len() - 1;
                }
            })?;
            Ok(vid)
        })
    }

    /// The group and version index when the cut is the whole of a version.
    fn cut_version(&self, id: CutId) -> Option<(GroupId, usize)> {
        let mut found = Vec::new();
        self.state.page.collect_cut_versions(&mut found);
        found.into_iter().find(|(c, _, _)| *c == id).map(|(_, g, k)| (g, k))
    }

    /// Each range becomes a word group with the fix as a shown AI variant; ranges that can't be
    /// grouped are skipped. One undo step; returns the new groups in page order.
    pub fn fix_words(&mut self, fixes: &[(Range<usize>, String)]) -> Result<Vec<GroupId>, EditError> {
        let offers: Vec<(Range<usize>, Vec<String>)> = fixes.iter().map(|(r, f)| (r.clone(), vec![f.clone()])).collect();
        self.words_to_groups(&offers, Scope::Word, true)
    }

    /// As [`Document::fix_words`], but the original stays showing with the alternatives after it.
    pub fn offer_words(&mut self, offers: &[(Range<usize>, Vec<String>)]) -> Result<Vec<GroupId>, EditError> {
        self.words_to_groups(offers, Scope::Word, false)
    }

    /// As [`Document::offer_words`], with sentence groups; word groups inside go along.
    pub fn offer_sentences(&mut self, offers: &[(Range<usize>, Vec<String>)]) -> Result<Vec<GroupId>, EditError> {
        self.words_to_groups(offers, Scope::Sentence, false)
    }

    fn words_to_groups(&mut self, offers: &[(Range<usize>, Vec<String>)], scope: Scope, show: bool) -> Result<Vec<GroupId>, EditError> {
        let mut order: Vec<(&Range<usize>, Vec<&String>)> = offers
            .iter()
            .map(|(r, alts)| (r, alts.iter().filter(|a| !a.is_empty()).collect::<Vec<_>>()))
            .filter(|(r, alts)| !r.is_empty() && !alts.is_empty() && r.end <= self.len())
            .collect();
        order.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
        let ids: Vec<(GroupId, VariantId, Vec<VariantId>)> = order
            .iter()
            .map(|(_, alts)| (GroupId(self.alloc_id()), VariantId(self.alloc_id()), alts.iter().map(|_| VariantId(self.alloc_id())).collect()))
            .collect();
        self.transact(None, |d| {
            let mut made = Vec::new();
            let mut last_start = usize::MAX;
            for ((range, alts), (gid, vid, alt_ids)) in order.iter().zip(ids) {
                if range.end > last_start {
                    continue;
                }
                let before = d.state.clone();
                if wrap(&mut d.state.page, range.start, range.end, scope, gid, vid).is_err() {
                    d.state = before;
                    continue;
                }
                d.modify_group(gid, &mut |g| {
                    for (alt, id) in alts.iter().zip(&alt_ids) {
                        g.variants.push(Variant { without: false, id: *id, by: Source::Ai, content: Container::from_text(alt) });
                    }
                    if show {
                        g.active = 1;
                    }
                })?;
                last_start = range.start;
                made.push(gid);
            }
            made.reverse();
            Ok(made)
        })
    }

    /// Skips ids that no longer exist; one undo step.
    pub fn settle_many(&mut self, ids: &[GroupId]) -> Result<usize, EditError> {
        self.transact(None, |d| {
            let mut n = 0;
            for &id in ids {
                if d.find_group(id).is_some() {
                    d.dissolve(id);
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Settles each group on its variant `index` (the original is 0), or what it shows if it has
    /// no such variant. Skips ids that no longer exist; one undo step.
    pub fn settle_to(&mut self, ids: &[GroupId], index: usize) -> Result<usize, EditError> {
        self.transact(None, |d| {
            let mut n = 0;
            for &id in ids {
                if d.find_group(id).is_some() {
                    d.modify_group(id, &mut |g| {
                        if index < g.variants.len() {
                            g.active = index;
                        }
                    })?;
                    d.dissolve(id);
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// One undo step of its own, so cycling right after it starts a new run.
    pub fn add_variant_and_show(
        &mut self,
        id: GroupId,
        text: &str,
        by: Source,
    ) -> Result<Cycled, EditError> {
        if text.is_empty() {
            return Err(EditError::EmptyText);
        }
        let vid = VariantId(self.alloc_id());
        self.transact(None, |d| {
            let mut active = 0;
            d.modify_group(id, &mut |g| {
                g.variants.push(Variant {
                    without: false,
                    id: vid,
                    by,
                    content: Container::from_text(text),
                });
                g.active = g.variants.len() - 1;
                active = g.active;
            })?;
            Ok(Cycled {
                active,
                is_original: false,
            })
        })
    }

    /// Combined in undo with cycling.
    pub fn set_active(&mut self, id: GroupId, index: usize) -> Result<Cycled, EditError> {
        self.transact(Some(UndoKey::Cycle(id)), |d| {
            let n = d
                .find_group(id)
                .ok_or(EditError::NoSuchGroup)?
                .variants
                .len();
            if index >= n {
                return Err(EditError::NoSuchVariant);
            }
            d.modify_group(id, &mut |g| g.active = index)?;
            Ok(Cycled {
                active: index,
                is_original: index == 0,
            })
        })
    }

    /// `delta > 0` next, else previous, wrapping around.
    pub fn cycle(&mut self, id: GroupId, delta: isize) -> Result<Cycled, EditError> {
        let g = self.find_group(id).ok_or(EditError::NoSuchGroup)?;
        let n = g.variants.len() as isize;
        let next = (g.active as isize + delta).rem_euclid(n) as usize;
        self.set_active(id, next)
    }

    /// If one is left, the group dissolves into text; removing the original makes the next one
    /// the original.
    pub fn remove_variant(&mut self, id: GroupId, index: usize) -> Result<Removed, EditError> {
        self.transact(None, |d| {
            let g = d.find_group(id).ok_or(EditError::NoSuchGroup)?;
            if index >= g.variants.len() {
                return Err(EditError::NoSuchVariant);
            }
            if g.variants.len() == 1 {
                return Err(EditError::LastVariant);
            }
            let text = g.variants[index].content.text();
            let left = d.modify_group(id, &mut |g| {
                g.variants.remove(index);
                g.keep_original();
                if index < g.active || (index == g.active && g.active > 0) {
                    g.active -= 1;
                }
                g.variants.len()
            })?;
            let dissolved = left == 1;
            if dissolved {
                d.dissolve(id);
            }
            Ok(Removed { text, dissolved })
        })
    }

    /// Keeps the active variant as plain text, its nested groups and cuts included.
    pub fn settle(&mut self, id: GroupId) -> Result<(), EditError> {
        self.transact(None, |d| {
            d.find_group(id).ok_or(EditError::NoSuchGroup)?;
            d.dissolve(id);
            Ok(())
        })
    }

    /// The same variant stays active. A version without the words can't be the original.
    pub fn make_original(&mut self, id: GroupId, index: usize) -> Result<(), EditError> {
        self.transact(None, |d| {
            let g = d.find_group(id).ok_or(EditError::NoSuchGroup)?;
            let n = g.variants.len();
            if index >= n {
                return Err(EditError::NoSuchVariant);
            }
            if g.variants[index].words_cut().is_some() {
                return Err(EditError::InCut);
            }
            d.modify_group(id, &mut |g| {
                let v = g.variants.remove(index);
                g.variants.insert(0, v);
                g.active = if g.active == index {
                    0
                } else if g.active < index {
                    g.active + 1
                } else {
                    g.active
                };
            })
        })
    }

    /// Only the stretch that changed is replaced, so nested groups and cuts outside it stay.
    pub fn edit_variant(&mut self, id: GroupId, index: usize, text: &str) -> Result<(), EditError> {
        if text.is_empty() {
            return Err(EditError::EmptyText);
        }
        self.transact(None, |d| {
            let g = d.find_group(id).ok_or(EditError::NoSuchGroup)?;
            let v = g.variants.get(index).ok_or(EditError::NoSuchVariant)?;
            if v.words_cut().is_some() {
                return Err(EditError::InCut);
            }
            let old: Vec<char> = v.content.text().chars().collect();
            let new: Vec<char> = text.chars().collect();
            let pre = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
            let suf = old[pre..]
                .iter()
                .rev()
                .zip(new[pre..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            let mid: String = new[pre..new.len() - suf].iter().collect();
            let end = old.len() - suf;
            let at = d.group_range(id).map_or(0, |r| r.start);
            let evicted = d.modify_group(id, &mut |g| g.variants[index].content.replace(pre, end, &mid))?;
            d.evict(evicted, at);
            Ok(())
        })
    }

    /// The cut shows, faded, with `look` of its own (`None` follows the setting). Partly covered
    /// groups and cuts grow the range; whole paragraphs take their blank line along.
    pub fn cut(&mut self, range: Range<usize>, look: Option<Look>) -> Result<(CutId, Range<usize>), EditError> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Err(EditError::EmptyRange);
        }
        let id = CutId(self.alloc_id());
        self.transact(None, |d| {
            let r = d.cut_inner(range, id, look, None)?;
            Ok((id, r))
        })
    }

    /// One undo step, one batch when `batch` is given; ranges already in a cut are skipped.
    pub fn cut_many(&mut self, ranges: &[Range<usize>], batch: Option<Batch>) -> Result<Vec<CutId>, EditError> {
        for r in ranges {
            self.check_range(r)?;
        }
        let mut order: Vec<Range<usize>> = ranges.iter().filter(|r| !r.is_empty()).cloned().collect();
        order.sort_by_key(|r| std::cmp::Reverse(r.start));
        let ids: Vec<CutId> = order.iter().map(|_| CutId(self.alloc_id())).collect();
        let batch = batch.map(Arc::new);
        self.transact(None, |d| {
            let mut made = Vec::new();
            for (r, id) in order.into_iter().zip(ids) {
                match d.cut_inner(r, id, None, batch.clone()) {
                    Ok(_) => made.push(id),
                    Err(EditError::InCut) => {}
                    Err(e) => return Err(e),
                }
            }
            // A later cut may have swallowed an earlier one.
            made.retain(|id| d.state.page.find_cut(*id).is_some());
            made.reverse();
            Ok(made)
        })
    }

    fn cut_inner(&mut self, range: Range<usize>, id: CutId, look: Option<Look>, batch: Option<Arc<Batch>>) -> Result<Range<usize>, EditError> {
        let t: Vec<char> = self.text().chars().collect();
        let trimmed = trim_range(&t, range.clone());
        let paragraph = whole_paragraphs(&t, &trimmed);
        let range = if paragraph {
            let p = paragraph_cut(&t, trimmed);
            range.start.min(p.start)..range.end.max(p.end)
        } else {
            range
        };
        let made = tick();
        let mut make = |content: Container| Cut { id, content, shown: true, look, made, paragraph, batch: batch.clone() };
        let mut lost = Vec::new();
        let r = self.state.page.cut(range.start, range.end, &mut make, &mut lost)?;
        self.evict(lost, r.start);
        Ok(r)
    }

    /// Returns where its text is now, if that is on the page. A version without the words is put
    /// back by showing the group's original.
    pub fn put_back(&mut self, id: CutId) -> Result<Option<Range<usize>>, EditError> {
        if let Some((gid, _)) = self.cut_version(id) {
            self.transact(None, |d| d.modify_group(gid, &mut |g| g.active = 0))?;
            return Ok(self.group_range(gid));
        }
        let len = self.state.page.find_cut(id).ok_or(EditError::NoSuchCut)?.content.len;
        let at = self.cut_info(id).and_then(|c| match c.place {
            CutPlace::Page { range, .. } => Some(range.start),
            _ => None,
        });
        self.transact(None, |d| {
            d.unwrap_cut(id);
            Ok(())
        })?;
        Ok(at.map(|a| a..a + len))
    }

    /// One undo step; returns how many.
    pub fn put_back_batch(&mut self, batch: &str) -> Result<usize, EditError> {
        let ids: Vec<CutId> = self.cuts().into_iter().filter(|c| c.batch.as_ref().is_some_and(|b| b.id == batch) && !matches!(c.place, CutPlace::Note(_))).map(|c| c.id).collect();
        if ids.is_empty() {
            return Err(EditError::NoSuchCut);
        }
        self.transact(None, |d| {
            for id in &ids {
                d.unwrap_cut(*id);
            }
            Ok(ids.len())
        })
    }

    /// A version without the words goes back by showing the group's original.
    fn unwrap_cut(&mut self, id: CutId) {
        if let Some((gid, _)) = self.cut_version(id) {
            let _ = self.modify_group(gid, &mut |g| g.active = 0);
            return;
        }
        self.state.page.modify_cut_parent(id, &mut |c, i| {
            let Node::Cut(cut) = c.nodes.remove(i) else { unreachable!() };
            c.nodes.splice(i..i, cut.content.nodes.iter().cloned());
        });
    }

    /// The trace goes; the cut stays with no trace and becomes the first note.
    pub fn forget_spot(&mut self, id: CutId) -> Result<(), EditError> {
        self.state.page.find_cut(id).ok_or(EditError::NoSuchCut)?;
        self.transact(None, |d| {
            let (cut, at) = d.remove_cut_node(id).ok_or(EditError::NoSuchCut)?;
            let origin = d.origin_quote(at);
            d.state.notes.insert(
                0,
                Note {
                    id: cut.id,
                    content: note_content(&cut),
                    origin: origin.clone(),
                    paragraph: cut.paragraph && origin.is_some(),
                    source: d.cut_source.clone(),
                    lost: false,
                    look: None,
                    batch: cut.batch.clone(),
                },
            );
            Ok(())
        })
    }

    fn remove_cut_node(&mut self, id: CutId) -> Option<(Arc<Cut>, usize)> {
        let at = self.cut_info(id).map_or(0, |c| match c.place {
            CutPlace::Page { range, .. } => range.start,
            CutPlace::Hidden { at } => at,
            CutPlace::Note(_) => 0,
        });
        let cut = self.state.page.modify_cut_parent(id, &mut |c, i| match c.nodes.remove(i) {
            Node::Cut(cut) => cut,
            _ => unreachable!(),
        })?;
        self.state.page.prune_empty();
        Some((cut, at))
    }

    /// An explicit change: the page may reflow, and the cut is no longer fresh.
    pub fn set_look(&mut self, id: CutId, look: Option<Look>) -> Result<(), EditError> {
        self.state.page.find_cut(id).ok_or(EditError::NoSuchCut)?;
        self.transact(None, |d| {
            let (p, epoch) = (d.presentation, d.epoch);
            d.state.page.modify_cut(id, &mut |c| {
                c.look = look;
                c.made = 0;
                c.shown = c.made > epoch || (p.editing && c.look.unwrap_or(p.look) == Look::Text);
            });
            Ok(())
        })
    }

    /// Wherever it is. One undo step.
    pub fn delete_cut(&mut self, id: CutId) -> Result<(), EditError> {
        if let Some(i) = self.state.notes.iter().position(|n| n.id == id) {
            return self.remove_note(i);
        }
        self.state.page.find_cut(id).ok_or(EditError::NoSuchCut)?;
        self.transact(None, |d| {
            d.delete_cut_node(id);
            Ok(())
        })
    }

    /// A version without the words that shows settles its group on it first, so nothing is
    /// kept; one that doesn't show just goes, with its version.
    fn delete_cut_node(&mut self, id: CutId) {
        if let Some((gid, k)) = self.cut_version(id) {
            if self.find_group(gid).is_some_and(|g| g.active == k) {
                self.dissolve(gid);
            }
        }
        self.remove_cut_node(id);
    }

    /// Every cut with a spot on this page, hidden versions included (not a version without the
    /// words that isn't showing: that is a version, not a cut). Returns how many.
    pub fn delete_page_cuts(&mut self) -> Result<usize, EditError> {
        let ids: Vec<CutId> = self
            .cuts()
            .into_iter()
            .filter(|c| !matches!(c.place, CutPlace::Note(_)) && !(c.version.is_some() && matches!(c.place, CutPlace::Hidden { .. })))
            .map(|c| c.id)
            .collect();
        if ids.is_empty() {
            return Ok(0);
        }
        self.transact(None, |d| {
            for id in &ids {
                d.delete_cut_node(*id);
            }
            Ok(ids.len())
        })
    }

    /// One undo step; returns how many.
    pub fn delete_batch(&mut self, batch: &str) -> Result<usize, EditError> {
        let ids: Vec<CutId> = self.cuts().into_iter().filter(|c| c.batch.as_ref().is_some_and(|b| b.id == batch)).map(|c| c.id).collect();
        if ids.is_empty() {
            return Err(EditError::NoSuchCut);
        }
        self.transact(None, |d| {
            for id in &ids {
                if let Some(i) = d.state.notes.iter().position(|n| n.id == *id) {
                    d.state.notes.remove(i);
                } else {
                    d.remove_cut_node(*id);
                }
            }
            Ok(ids.len())
        })
    }

    /// One undo step.
    pub fn take_cut(&mut self, id: CutId) -> Result<Scrap, EditError> {
        if let Some(i) = self.state.notes.iter().position(|n| n.id == id) {
            let (scrap, _) = self.note_scrap(i)?;
            self.remove_note(i)?;
            return Ok(scrap);
        }
        self.state.page.find_cut(id).ok_or(EditError::NoSuchCut)?;
        self.transact(None, |d| {
            let (cut, at) = d.remove_cut_node(id).ok_or(EditError::NoSuchCut)?;
            let origin = d.origin_quote(at);
            Ok(Scrap {
                fragment: Fragment { nodes: note_content(&cut).nodes },
                paragraph: cut.paragraph && origin.is_some(),
                origin,
                lost: false,
                look: None,
                batch: cut.batch.as_deref().cloned(),
            })
        })
    }

    /// Not an undo step. Every cut made so far stops being fresh.
    pub fn set_presentation(&mut self, p: Presentation) {
        let before = self.state.clone();
        self.presentation = p;
        self.epoch = tick();
        self.apply_presentation();
        if self.state != before {
            self.record_changes(&before);
        }
    }

    /// Ghosting what is already a cut changes nothing.
    pub fn ghost(&mut self, range: Range<usize>) -> Result<(), EditError> {
        match self.cut(range, None) {
            Ok(_) | Err(EditError::InCut) | Err(EditError::EmptyRange) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn ghost_many(&mut self, ranges: &[Range<usize>]) -> Result<(), EditError> {
        self.cut_many(ranges, None).map(|_| ())
    }

    /// Every cut whose trace overlaps the range.
    pub fn revive(&mut self, range: Range<usize>) -> Result<(), EditError> {
        self.check_range(&range)?;
        let shown: Vec<CutId> = self.traces().into_iter().filter(|t| t.shown).map(|t| t.id).collect();
        let ids: Vec<CutId> = self.cuts_in(range).into_iter().filter(|id| shown.contains(id)).collect();
        self.transact(None, |d| {
            for id in &ids {
                d.unwrap_cut(*id);
            }
            Ok(())
        })
    }

    /// Every cut whose text shows, in one undo step. Returns how many chars went.
    pub fn remove_ghosted(&mut self) -> Result<usize, EditError> {
        let shown: Vec<TraceInfo> = self.traces().into_iter().filter(|t| t.shown).collect();
        let n = shown.iter().map(|t| t.range.len()).sum();
        if n == 0 {
            return Ok(0);
        }
        self.transact(None, |d| {
            for t in &shown {
                d.delete_cut_node(t.id);
            }
            Ok(n)
        })
    }

    /// Cuts are left out: a copy carries only kept text.
    pub fn copy(&self, range: Range<usize>) -> Result<Fragment, EditError> {
        self.check_range(&range)?;
        Ok(Fragment {
            nodes: tree::drop_comments(self.state.page.extract_kept(range.start, range.end)),
        })
    }

    /// Fresh ids. Fragments never join groups at their edges; inside a variant they nest, inside
    /// a cut they become its text.
    pub fn paste(&mut self, at: usize, fragment: &Fragment) -> Result<(), EditError> {
        self.check_range(&(at..at))?;
        let mut next = self.next_id;
        let nodes = tree::reid(&fragment.nodes, &mut next);
        self.next_id = next;
        self.transact(None, |d| {
            d.put(at, nodes);
            Ok(())
        })
    }

    /// Every cut's text joined in; cuts that took no room stay where the range was.
    fn take_for_note(&mut self, range: Range<usize>) -> Vec<Node> {
        let taken = self.state.page.take(range.start, range.end);
        let (mut points, mut lost) = (Vec::new(), Vec::new());
        let nodes = tree::absorb_cuts(taken, true, &mut points, &mut lost);
        if !points.is_empty() {
            self.put(range.start, points);
        }
        self.evict(lost, range.start);
        nodes
    }

    /// Keeps its groups. Returns the note index (always 0).
    pub fn stash(&mut self, range: Range<usize>) -> Result<usize, EditError> {
        self.cut_without_trace(range).map(|_| 0)
    }

    /// Into a new cut with no trace, the first note.
    pub fn cut_without_trace(&mut self, range: Range<usize>) -> Result<CutId, EditError> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Err(EditError::EmptyRange);
        }
        let paragraph = self.is_whole_paragraphs(range.clone());
        let id = CutId(self.alloc_id());
        self.transact(None, |d| {
            let nodes = d.take_for_note(range.clone());
            // The link names the paragraph at the cut *after* it: what's left of it, or its neighbour.
            let origin = d.origin_quote(range.start);
            // The flag is saved on the origin's record, so only a note with an origin has it.
            let paragraph = paragraph && origin.is_some();
            let source = d.cut_source.clone();
            d.state.notes.insert(
                0,
                Note { origin, paragraph, source, ..Note::new(id, Container::from_nodes(nodes)) },
            );
            Ok(id)
        })
    }

    pub fn unstash(&mut self, note: usize, at: usize) -> Result<(), EditError> {
        self.check_range(&(at..at))?;
        let n = self.state.notes.get(note).ok_or(EditError::NoSuchNote)?;
        let mut next = self.next_id;
        let nodes = tree::reid(&n.content.nodes, &mut next);
        self.next_id = next;
        self.transact(None, |d| {
            d.state.notes.remove(note);
            d.put(at, nodes);
            Ok(())
        })
    }

    /// Own notes (those that name no file) become the project's cuts of `source`. One undo
    /// step; returns how many.
    pub fn claim_notes(&mut self, source: &str) -> Result<usize, EditError> {
        let n = self.state.notes.iter().filter(|n| n.source.is_none()).count();
        if n == 0 {
            return Err(EditError::NoSuchNote);
        }
        self.transact(None, |d| {
            for note in d.state.notes.iter_mut().filter(|n| n.source.is_none()) {
                note.source = Some(source.to_string());
            }
            Ok(n)
        })
    }

    /// Not an undo step: a project's two cuts files (`cuts.md`, an older `drawer.md`) read as one.
    pub fn absorb_notes(&mut self, other: &Document) {
        for n in &other.state.notes {
            let mut next = self.next_id;
            let nodes = tree::reid(&n.content.nodes, &mut next);
            self.next_id = next;
            let id = CutId(self.alloc_id());
            let mut n = n.clone();
            n.id = id;
            n.content.nodes = nodes;
            self.state.notes.push(n);
        }
        self.changes.push(Change::Notes);
    }

    pub fn add_note(&mut self, index: usize, text: &str) -> Result<(), EditError> {
        if index > self.state.notes.len() {
            return Err(EditError::NoSuchNote);
        }
        let id = CutId(self.alloc_id());
        self.transact(None, |d| {
            d.state.notes.insert(index, Note::new(id, Container::from_text(text)));
            Ok(())
        })
    }

    pub fn remove_note(&mut self, index: usize) -> Result<(), EditError> {
        if index >= self.state.notes.len() {
            return Err(EditError::NoSuchNote);
        }
        self.transact(None, |d| {
            d.state.notes.remove(index);
            Ok(())
        })
    }

    pub fn copy_note_to(&mut self, note: usize, at: usize) -> Result<(), EditError> {
        self.check_range(&(at..at))?;
        let n = self.state.notes.get(note).ok_or(EditError::NoSuchNote)?;
        let mut next = self.next_id;
        let nodes = tree::reid(&n.content.nodes, &mut next);
        self.next_id = next;
        self.transact(None, |d| {
            d.put(at, nodes);
            Ok(())
        })
    }

    pub fn move_note(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        let len = self.state.notes.len();
        if from >= len || to >= len {
            return Err(EditError::NoSuchNote);
        }
        self.transact(None, |d| {
            let n = d.state.notes.remove(from);
            d.state.notes.insert(to, n);
            Ok(())
        })
    }

    /// The page stays as it is. Returns the note index (always 0).
    pub fn stash_copy(&mut self, range: Range<usize>) -> Result<usize, EditError> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Err(EditError::EmptyRange);
        }
        let mut next = self.next_id;
        let nodes = tree::reid(
            &self.state.page.extract_kept(range.start, range.end),
            &mut next,
        );
        if nodes.is_empty() {
            return Err(EditError::EmptyText);
        }
        self.next_id = next;
        let id = CutId(self.alloc_id());
        let origin = self.origin_quote(range.start);
        let paragraph = origin.is_some() && self.is_whole_paragraphs(range);
        self.transact(None, |d| {
            d.state.notes.insert(0, Note { origin, paragraph, ..Note::new(id, Container::from_nodes(nodes)) });
            Ok(0)
        })
    }

    /// The page is unchanged; cuts are left out, as in any copy.
    pub fn scrap_of(&self, range: Range<usize>) -> Result<Scrap, EditError> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Err(EditError::EmptyRange);
        }
        let origin = self.origin_quote(range.start);
        Ok(Scrap {
            fragment: Fragment {
                nodes: tree::drop_comments(self.state.page.extract_kept(range.start, range.end)),
            },
            paragraph: origin.is_some() && self.is_whole_paragraphs(range),
            origin,
            lost: false,
            look: None,
            batch: None,
        })
    }

    /// [`Self::stash`] for a drawer in another document; one undo step.
    pub fn cut_scrap(&mut self, range: Range<usize>) -> Result<Scrap, EditError> {
        self.check_range(&range)?;
        if range.is_empty() {
            return Err(EditError::EmptyRange);
        }
        let paragraph = self.is_whole_paragraphs(range.clone());
        self.transact(None, |d| {
            let nodes = d.take_for_note(range.clone());
            let origin = d.origin_quote(range.start);
            Ok(Scrap {
                fragment: Fragment { nodes },
                paragraph: paragraph && origin.is_some(),
                origin,
                lost: false,
                look: None,
                batch: None,
            })
        })
    }

    fn scrap_note(&mut self, scrap: &Scrap, source: Option<String>, next: &mut u64) -> Note {
        let nodes = tree::flatten_all(tree::reid(&scrap.fragment.nodes, next));
        *next += 1;
        Note {
            id: CutId(*next),
            content: Container::from_nodes(nodes),
            origin: scrap.origin.clone(),
            paragraph: scrap.paragraph && scrap.origin.is_some(),
            source,
            lost: scrap.lost,
            look: scrap.look,
            batch: scrap.batch.clone().map(Arc::new),
        }
    }

    /// Fresh ids. `source`: the file it came from, relative to the project.
    pub fn add_scrap(
        &mut self,
        index: usize,
        scrap: &Scrap,
        source: Option<String>,
    ) -> Result<(), EditError> {
        if index > self.state.notes.len() {
            return Err(EditError::NoSuchNote);
        }
        let mut next = self.next_id;
        let note = self.scrap_note(scrap, source, &mut next);
        self.next_id = next;
        self.transact(None, |d| {
            d.state.notes.insert(index, note);
            Ok(())
        })
    }

    /// At the top of Overflow, in their order, as one undo step.
    pub fn add_scraps(&mut self, scraps: &[(Scrap, Option<String>)]) -> Result<(), EditError> {
        if scraps.is_empty() {
            return Err(EditError::EmptyText);
        }
        let mut next = self.next_id;
        let notes: Vec<Note> = scraps.iter().map(|(s, source)| self.scrap_note(s, source.clone(), &mut next)).collect();
        self.next_id = next;
        self.transact(None, |d| {
            d.state.notes.splice(0..0, notes);
            Ok(())
        })
    }

    pub fn note_fragment(&self, index: usize) -> Result<Fragment, EditError> {
        let n = self.state.notes.get(index).ok_or(EditError::NoSuchNote)?;
        Ok(Fragment {
            nodes: n.content.nodes.clone(),
        })
    }

    pub fn note_scrap(&self, index: usize) -> Result<(Scrap, Option<String>), EditError> {
        let n = self.state.notes.get(index).ok_or(EditError::NoSuchNote)?;
        Ok((
            Scrap {
                fragment: Fragment {
                    nodes: n.content.nodes.clone(),
                },
                origin: n.origin.clone(),
                paragraph: n.paragraph,
                lost: n.lost,
                look: n.look,
                batch: n.batch.as_deref().cloned(),
            },
            n.source.clone(),
        ))
    }

    /// Throughout the history too, so undo never brings back a gone name. Not an undo step.
    /// Returns whether anything changed.
    pub fn rename_sources(&mut self, map: impl Fn(&str) -> Option<String>) -> bool {
        let mut changed = false;
        let mut fix = |notes: &mut Vec<Note>| {
            for n in notes.iter_mut() {
                if let Some(new) = n.source.as_deref().and_then(&map) {
                    n.source = Some(new);
                    changed = true;
                }
            }
        };
        fix(&mut self.state.notes);
        for e in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            fix(&mut e.state.notes);
        }
        if changed {
            self.changes.push(Change::Notes);
        }
        changed
    }

    /// One undo step.
    pub fn clear_notes(&mut self) -> Result<(), EditError> {
        if self.state.notes.is_empty() {
            return Err(EditError::NoSuchNote);
        }
        self.transact(None, |d| {
            d.state.notes.clear();
            Ok(())
        })
    }

    /// `range` trimmed starts and ends at paragraph edges; dragged, such text goes between
    /// paragraphs, anything shorter between words.
    pub fn is_whole_paragraphs(&self, range: Range<usize>) -> bool {
        let t: Vec<char> = self.text().chars().collect();
        if range.end > t.len() {
            return false;
        }
        whole_paragraphs(&t, &trim_range(&t, range))
    }

    /// One undo step; groups and cuts travel with it. Spaces are fixed where it lands and where
    /// it left; whole paragraphs take their blank line. Returns the dropped text's range.
    /// Dropping page text onto itself is [`EditError::Unchanged`].
    pub fn drop_text(
        &mut self,
        what: Carried,
        to: Place,
        copy: bool,
    ) -> Result<Range<usize>, EditError> {
        let t: Vec<char> = self.text().chars().collect();
        let target = match to {
            Place::Inline(a) | Place::Block(a) => a,
        };
        if target > t.len() {
            return Err(EditError::OutOfBounds);
        }
        let source = match &what {
            Carried::Page(r) => {
                self.check_range(r)?;
                let r = trim_range(&t, r.clone());
                if r.is_empty() {
                    return Err(EditError::EmptyRange);
                }
                let block = whole_paragraphs(&t, &r);
                // Onto itself, or (as a paragraph) just after itself: nothing moves.
                let after_itself = matches!(to, Place::Block(_))
                    && target >= r.end
                    && t[r.end..target].iter().all(|c| c.is_whitespace());
                if (r.start <= target && target <= r.end) || after_itself {
                    return Err(EditError::Unchanged);
                }
                Some((r, block))
            }
            Carried::Note(i) => {
                if *i >= self.state.notes.len() {
                    return Err(EditError::NoSuchNote);
                }
                None
            }
            Carried::Cut(id) => {
                let info = self.cut_info(*id).ok_or(EditError::NoSuchCut)?;
                if let CutPlace::Page { range, shown: true } = &info.place {
                    if !copy && range.start < target && target < range.end {
                        return Err(EditError::Unchanged);
                    }
                }
                None
            }
            Carried::Fragment(f) => {
                if f.is_empty() {
                    return Err(EditError::EmptyText);
                }
                None
            }
        };
        self.transact(None, |d| {
            let mut at = target;
            let mut next = d.next_id;
            let nodes = match (&what, source) {
                (Carried::Page(_), Some((r, block))) => {
                    if copy {
                        tree::reid(&d.state.page.extract_kept(r.start, r.end), &mut next)
                    } else {
                        // Moved, so its cuts go with it (ED-9).
                        let nodes = d.state.page.take(r.start, r.end);
                        let cut = if block { paragraph_cut(&t, r.clone()) } else { r.clone() };
                        // The blank lines around whole paragraphs, now on either side of r.start.
                        let (lead, trail) = (r.start - cut.start, cut.end - r.end);
                        let evicted = d.state.page.replace(r.start - lead, r.start + trail, "");
                        d.evict(evicted, cut.start);
                        at = if at >= cut.end {
                            at - cut.len()
                        } else {
                            at.min(cut.start)
                        };
                        if !block {
                            if let Some(k) = d.tidy_hole(cut.start) {
                                if k < at {
                                    at -= 1;
                                }
                            }
                        }
                        nodes
                    }
                }
                (Carried::Cut(id), _) => {
                    // The text without the spaces at its edges: they are fixed where it lands.
                    let mut frag = Container::from_nodes(d.cut_fragment(*id)?.nodes);
                    let t = frag.text();
                    let trail = t.chars().rev().take_while(|c| c.is_whitespace()).count();
                    let lead = t.chars().take_while(|c| c.is_whitespace()).count();
                    if lead < frag.len {
                        let len = frag.len;
                        frag.replace(len - trail, len, "");
                        frag.replace(0, lead, "");
                    }
                    // A version without the words is copied: the version stays.
                    if !copy && d.cut_version(*id).is_none() {
                        let info = d.cut_info(*id).ok_or(EditError::NoSuchCut)?;
                        match info.place {
                            CutPlace::Note(i) => {
                                d.state.notes.remove(i);
                            }
                            CutPlace::Page { range, .. } => {
                                d.remove_cut_node(*id);
                                at = if at >= range.end { at - range.len() } else { at.min(range.start) };
                                if !info.paragraph {
                                    if let Some(k) = d.tidy_hole(range.start) {
                                        if k < at {
                                            at -= 1;
                                        }
                                    }
                                }
                            }
                            CutPlace::Hidden { .. } => {
                                d.remove_cut_node(*id);
                            }
                        }
                    }
                    tree::reid(&frag.nodes, &mut next)
                }
                (Carried::Note(i), _) => {
                    let nodes = tree::reid(&d.state.notes[*i].content.nodes, &mut next);
                    if !copy {
                        d.state.notes.remove(*i);
                    }
                    nodes
                }
                (Carried::Fragment(f), _) => tree::reid(&f.nodes, &mut next),
                (Carried::Page(_), None) => unreachable!(),
            };
            d.next_id = next;
            let now: Vec<char> = d.text().chars().collect();
            let before_len = d.len();
            match to {
                Place::Inline(_) => {
                    let (l, r) = (at.checked_sub(1).map(|i| now[i]), now.get(at).copied());
                    d.put(at, nodes);
                    let n = d.len() - before_len;
                    if n == 0 {
                        return Ok(at..at);
                    }
                    let first = d.text_range(at..at + 1).chars().next();
                    let last = d.text_range(at + n - 1..at + n).chars().next();
                    let space_after = r.is_some_and(|c| !c.is_whitespace() && !closes(c))
                        && !last.is_some_and(opens);
                    let space_before = l.is_some_and(|c| !c.is_whitespace() && !opens(c))
                        && !first.is_some_and(closes);
                    if space_after {
                        d.state.page.insert(at + n, Payload::Text(" "), false);
                    }
                    if space_before {
                        d.state.page.insert(at, Payload::Text(" "), false);
                        at += 1;
                    }
                    Ok(at..at + n)
                }
                Place::Block(_) if at >= now.len() => {
                    if !now.is_empty() {
                        let trailing = now.iter().rev().take_while(|c| **c == '\n').count();
                        let need = 2usize.saturating_sub(trailing);
                        d.state
                            .page
                            .insert(at, Payload::Text(&"\n".repeat(need)), false);
                        at += need;
                    }
                    let before = d.len();
                    d.put(at, nodes);
                    Ok(at..at + d.len() - before)
                }
                Place::Block(_) => {
                    let lead =
                        if at == 0 || (now[at - 1] == '\n' && (at == 1 || now[at - 2] == '\n')) {
                            ""
                        } else if now[at - 1] == '\n' {
                            "\n"
                        } else {
                            "\n\n"
                        };
                    if !lead.is_empty() {
                        d.state.page.insert(at, Payload::Text(lead), false);
                        at += lead.len();
                    }
                    let before = d.len();
                    d.put(at, nodes);
                    let n = d.len() - before;
                    d.state.page.insert(at + n, Payload::Text("\n\n"), false);
                    Ok(at..at + n)
                }
            }
        })
    }

    /// No doubled space, none before punctuation or at a line's edge. Returns where a char went.
    fn tidy_hole(&mut self, pos: usize) -> Option<usize> {
        let l = pos
            .checked_sub(1)
            .and_then(|i| self.text_range(i..pos).chars().next());
        let r = self.text_range(pos..pos + 1).chars().next();
        let k = if l == Some(' ') && r.is_none_or(|c| c == ' ' || c == '\n' || closes(c)) {
            pos - 1
        } else if l.is_none_or(|c| c == '\n') && r == Some(' ') {
            pos
        } else {
            return None;
        };
        self.state.page.replace(k, k + 1, "");
        Some(k)
    }

    pub fn break_undo_group(&mut self) {
        self.last_key = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_tag(&self) -> Option<StepTag> {
        self.undo.last().map(Entry::tag)
    }

    pub fn redo_tag(&self) -> Option<StepTag> {
        self.redo.last().map(Entry::tag)
    }

    /// Marks the last step as half of a change spanning two documents; also ends the step.
    pub fn link_last_step(&mut self, link: u64) {
        if let Some(t) = self.undo.last_mut() {
            t.link = Some(link);
        }
        self.last_key = None;
    }

    pub fn undo(&mut self) -> bool {
        let Some(e) = self.undo.pop() else {
            return false;
        };
        let prev = std::mem::replace(&mut self.state, e.state);
        self.apply_presentation();
        self.record_changes(&prev);
        self.redo.push(Entry { state: prev, seq: e.seq, link: e.link });
        self.last_key = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(r) = self.redo.pop() else {
            return false;
        };
        let prev = std::mem::replace(&mut self.state, r.state);
        self.apply_presentation();
        self.record_changes(&prev);
        self.undo.push(Entry { state: prev, seq: r.seq, link: r.link });
        self.last_key = None;
        true
    }

    /// Used by tests; cheap enough for debug builds.
    #[doc(hidden)]
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        self.state.page.check("page", &mut ids, true, true)?;
        for (i, n) in self.state.notes.iter().enumerate() {
            if !ids.insert(n.id.0) {
                return Err(format!("note{i}: duplicate cut id"));
            }
            n.content.check(&format!("note{i}"), &mut ids, true, false)?;
        }
        let mut wrong = Vec::new();
        self.state.page.collect_cuts(0, true, &mut wrong);
        for (c, _, _) in wrong {
            if c.shown != self.wants_shown(c) {
                return Err(format!("cut {}: shown is {} against the presentation", c.id.0, c.shown));
            }
        }
        if let Some(max) = ids.iter().max() {
            if *max > self.next_id {
                return Err(format!("id {max} above the allocator ({})", self.next_id));
            }
        }
        Ok(())
    }

    fn alloc_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn check_range(&self, r: &Range<usize>) -> Result<(), EditError> {
        if r.start > r.end || r.end > self.len() {
            Err(EditError::OutOfBounds)
        } else {
            Ok(())
        }
    }

    /// One undo step, or merged into the previous one with the same key; failures and no-ops
    /// leave history untouched.
    fn transact<T>(
        &mut self,
        key: Option<UndoKey>,
        f: impl FnOnce(&mut Self) -> Result<T, EditError>,
    ) -> Result<T, EditError> {
        let before = self.state.clone();
        match f(self) {
            Err(e) => {
                self.state = before;
                Err(e)
            }
            Ok(v) => {
                self.settle_comments();
                if self.state == before {
                    return Ok(v);
                }
                self.record_changes(&before);
                let coalesce = key.is_some() && key == self.last_key && !self.undo.is_empty();
                if !coalesce {
                    self.undo.push(Entry { state: before, seq: tick(), link: None });
                    if self.undo.len() > MAX_UNDO {
                        self.undo.remove(0);
                    }
                    self.last_key = key;
                } else if self.undo.last().is_some_and(|t| t.state == self.state) {
                    // A cycling run came back to where it started: nothing to undo.
                    self.undo.pop();
                    self.last_key = None;
                } else if let Some(t) = self.undo.last_mut() {
                    // The step grew: it now counts as the latest.
                    t.seq = tick();
                }
                self.redo.clear();
                Ok(v)
            }
        }
    }

    fn record_changes(&mut self, before: &State) {
        if self.changes.last() == Some(&Change::Reset) {
            return;
        }
        if self.changes.len() >= MAX_CHANGES {
            self.changes.clear();
            self.changes.push(Change::Reset);
            return;
        }
        if let Some((old, new_len)) = tree::diff(&before.page, &self.state.page) {
            let same_text = old.len() == new_len && {
                let (mut a, mut b) = (String::new(), String::new());
                before.page.text_range(old.start, old.end, &mut a);
                self.state
                    .page
                    .text_range(old.start, old.start + new_len, &mut b);
                a == b
            };
            self.changes.push(if same_text {
                Change::Style(old)
            } else {
                Change::Text { old, new_len }
            });
        }
        if before.notes != self.state.notes {
            self.changes.push(Change::Notes);
        }
    }

    /// Cuts whose spot was deleted become notes at the top, *place gone*, linked to the paragraph
    /// at `at`.
    fn evict(&mut self, cuts: Vec<Arc<Cut>>, at: usize) {
        if cuts.is_empty() {
            return;
        }
        let origin = self.origin_quote(at.min(self.len()));
        for c in cuts {
            self.state.notes.insert(
                0,
                Note {
                    id: c.id,
                    content: note_content(&c),
                    origin: origin.clone(),
                    paragraph: c.paragraph && origin.is_some(),
                    source: self.cut_source.clone(),
                    lost: true,
                    look: c.look,
                    batch: c.batch.clone(),
                },
            );
        }
    }

    /// Cuts that can't stay cuts there are kept as notes.
    fn put(&mut self, at: usize, nodes: Vec<Node>) {
        let lost = self.state.page.insert(at, Payload::Nodes(nodes), false);
        self.evict(lost, at);
    }

    fn wants_shown(&self, c: &Cut) -> bool {
        c.made > self.epoch || (self.presentation.editing && c.look.unwrap_or(self.presentation.look) == Look::Text)
    }

    /// Recomputes which cuts show, outside the history.
    fn apply_presentation(&mut self) {
        let (p, epoch) = (self.presentation, self.epoch);
        let want = move |c: &Cut| c.made > epoch || (p.editing && c.look.unwrap_or(p.look) == Look::Text);
        self.state.page.present(&want);
    }

    fn find_group(&self, id: GroupId) -> Option<&tree::Group> {
        self.state.page.find_group(id).or_else(|| {
            self.state
                .notes
                .iter()
                .find_map(|n| n.content.find_group(id))
        })
    }

    fn modify_group<R>(
        &mut self,
        id: GroupId,
        f: &mut dyn FnMut(&mut tree::Group) -> R,
    ) -> Result<R, EditError> {
        if let Some(r) = self.state.page.modify_group(id, f) {
            return Ok(r);
        }
        for n in &mut self.state.notes {
            if let Some(r) = n.content.modify_group(id, f) {
                return Ok(r);
            }
        }
        Err(EditError::NoSuchGroup)
    }

    /// Replaces group `id` with the nodes of its active variant (ghost flag carried over).
    fn dissolve(&mut self, id: GroupId) {
        let mut splice = |c: &mut Container, i: usize| {
            let Node::Group(g) = c.nodes.remove(i) else {
                unreachable!()
            };
            let inner = g.variants[g.active].content.clone();
            c.nodes.splice(i..i, inner.nodes);
        };
        if !self.state.page.modify_parent(id, &mut splice) {
            for n in &mut self.state.notes {
                if n.content.modify_parent(id, &mut splice) {
                    break;
                }
            }
        }
    }

    /// On a blank line, the next paragraph's, or the previous one's at the end of the page.
    fn origin_quote(&self, pos: usize) -> Option<String> {
        if let Some(q) = self.paragraph_quote(pos) {
            return Some(q);
        }
        let chars: Vec<char> = self.text().chars().collect();
        let blank_from = |i: usize| chars[i..].iter().take_while(|c| **c != '\n').all(|c| c.is_whitespace());
        let mut i = pos.min(chars.len());
        while i < chars.len() {
            if (i == 0 || chars[i - 1] == '\n') && !blank_from(i) {
                return self.paragraph_quote(i);
            }
            i += 1;
        }
        let mut i = pos.min(chars.len());
        while i > 0 {
            i -= 1;
            if !chars[i].is_whitespace() {
                return self.paragraph_quote(i);
            }
        }
        None
    }

    fn paragraph_quote(&self, pos: usize) -> Option<String> {
        let chars: Vec<char> = self.text().chars().collect();
        let mut s = pos.min(chars.len());
        while s > 0 && chars[s - 1] != '\n' {
            s -= 1;
        }
        let q: String = chars[s..]
            .iter()
            .take_while(|c| **c != '\n')
            .take(24)
            .collect();
        (!q.trim().is_empty()).then_some(q)
    }
}

fn wrap(
    c: &mut Container,
    s: usize,
    e: usize,
    scope: Scope,
    gid: GroupId,
    vid: VariantId,
) -> Result<(), EditError> {
    c.wrap(s, e, |content| tree::Group {
        id: gid,
        scope,
        active: 0,
        variants: vec![Variant { without: false, id: vid, by: Source::Me, content }],
    })
}

/// A cut's text as a note keeps it: a paragraph cut without the blank line it took along.
fn note_content(c: &Cut) -> Container {
    let mut content = c.content.clone();
    if c.paragraph {
        let lead = leading_ws(&content);
        let t = content.text();
        let trail = t.chars().rev().take_while(|c| c.is_whitespace()).count();
        let len = content.len;
        if lead < len {
            content.replace(len - trail, len, "");
            content.replace(0, lead, "");
        }
    }
    content
}

fn leading_ws(c: &Container) -> usize {
    c.text().chars().take_while(|c| c.is_whitespace()).count()
}

fn comment_texts(c: &Container) -> Vec<String> {
    let mut all = Vec::new();
    c.all_comments(&mut all);
    all.into_iter().map(|c| c.text.clone()).collect()
}

fn cut_text(c: &Container, paragraph: bool) -> String {
    let t = c.text();
    // As [`note_content`]: a cut that is only blank lines keeps them.
    if paragraph && !t.trim().is_empty() {
        t.trim().to_string()
    } else {
        t
    }
}
