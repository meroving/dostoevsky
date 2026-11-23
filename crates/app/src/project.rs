use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use l10n::tr;

use crate::dates;
use crate::files;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Book,
    Journal,
    Folder,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Book, Kind::Journal, Kind::Folder];

    /// The word in `.dostoevsky`; stays English.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Book => "book",
            Kind::Journal => "journal",
            Kind::Folder => "folder",
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key().eq_ignore_ascii_case(key.trim()))
    }

    /// Shown only, never in a file name; a phrase around it should be a key of its own per kind.
    pub fn noun(self) -> &'static str {
        match self {
            Kind::Book => tr!("Chapter"),
            Kind::Journal => tr!("Day"),
            Kind::Folder => tr!("File"),
        }
    }

    pub fn list_tab(self) -> &'static str {
        match self {
            Kind::Book => tr!("Chapters"),
            Kind::Journal => tr!("Days"),
            Kind::Folder => tr!("Files"),
        }
    }

    /// Translated, for the panel; [`Kind::key`] is the file's word.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Book => tr!("book"),
            Kind::Journal => tr!("journal"),
            Kind::Folder => tr!("folder"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Day {
    pub fn today() -> Day {
        use chrono::Datelike as _;
        let d = chrono::Local::now().date_naive();
        Day { year: d.year(), month: d.month(), day: d.day() }
    }

    pub fn file_name(self) -> String {
        format!("{:04}-{:02}-{:02}.md", self.year, self.month, self.day)
    }

    /// Mon 23, as the Mac writes it in the UI's language.
    pub fn label(self) -> String {
        self.format(dates::Form::WeekdayDay).unwrap_or_else(|| format!(" {}", self.day))
    }

    fn format(self, form: dates::Form) -> Option<String> {
        dates::format(self.year, self.month, self.day, form)
    }

    /// The list writes months in lowercase.
    fn month_name(self) -> String {
        self.format(dates::Form::Month).unwrap_or_default().to_lowercase()
    }

    pub fn month_title(self) -> String {
        self.format(dates::Form::MonthYear).unwrap_or_default().to_lowercase()
    }

    /// The heading a new day starts with, in the UI's language; `None` for [`DateHeading::Off`].
    pub fn heading(self, form: DateHeading) -> Option<String> {
        match form {
            DateHeading::Off => None,
            DateHeading::Iso => Some(format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)),
            DateHeading::Long => self.format(dates::Form::Long),
            DateHeading::Short => self.format(dates::Form::Short),
        }
    }

    /// Whether `line` is a heading that says this day and nothing else, in any form and any
    /// language ([`dates::says`]): a day written under another UI language is still dated.
    pub fn is_heading(self, line: &str) -> bool {
        let Some(text) = line.trim().strip_prefix('#') else { return false };
        dates::says(text.trim_start_matches('#'), self.year, self.month, self.day)
    }
}

/// What a journal writes at the top of a new day (PROJ-6): saved as `date` in `.dostoevsky`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateHeading {
    #[default]
    Long,
    Short,
    Iso,
    Off,
}

impl DateHeading {
    pub const ALL: [DateHeading; 4] = [DateHeading::Long, DateHeading::Short, DateHeading::Iso, DateHeading::Off];

    /// The word in `.dostoevsky`; stays English.
    pub fn key(self) -> &'static str {
        match self {
            DateHeading::Long => "long",
            DateHeading::Short => "short",
            DateHeading::Iso => "iso",
            DateHeading::Off => "none",
        }
    }

    pub fn from_key(key: &str) -> Option<DateHeading> {
        DateHeading::ALL.into_iter().find(|d| d.key().eq_ignore_ascii_case(key.trim()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// The file's name without its number or extension.
    pub label: String,
    pub number: Option<u32>,
    pub day: Option<Day>,
    pub modified: SystemTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub title: Option<String>,
    /// Where new files in this group go.
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub root: PathBuf,
    pub kind: Kind,
    /// If not, `kind` and `notes_dir` are a guess from the file names.
    pub chosen: bool,
    /// It may not exist yet: Move to Notes makes it.
    pub notes_dir: Option<PathBuf>,
    pub main: Vec<Group>,
    /// Not counted, not in the whole-book text.
    pub notes: Vec<Group>,
    /// Neither in the list nor notes (an unnumbered chapter): listed, never counted, never hidden.
    pub loose: Vec<Entry>,
    pub counts: Counts,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub numbered: usize,
    pub dated: usize,
    pub total: usize,
}

pub const SETTINGS_FILE: &str = ".dostoevsky";

/// Every chapter's cuts in one Dostoevsky file at the root; it never shows in the list.
pub const CUTS_FILE: &str = "cuts.md";

/// The older cuts file: read while there is no `cuts.md`, renamed to it on the first write.
pub const DRAWER_FILE: &str = "drawer.md";

pub fn drawer_path(root: &Path) -> PathBuf {
    root.join(CUTS_FILE)
}

pub fn old_drawer_path(root: &Path) -> PathBuf {
    root.join(DRAWER_FILE)
}

pub fn is_cuts_file(root: &Path, path: &Path) -> bool {
    path == drawer_path(root) || path == old_drawer_path(root)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub kind: Kind,
    /// Relative to the project.
    pub notes: Option<String>,
    /// As Export sets them; kept here so they travel with the folder.
    pub title: Option<String>,
    pub author: Option<String>,
    /// A journal's; `None` keeps what the file says, and a file that says nothing means the default.
    pub date: Option<DateHeading>,
}

impl Settings {
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Dostoevsky keeps what this folder is here (PRD §6.11).\n");
        out.push_str(&format!("kind = \"{}\"\n", self.kind.key()));
        let quote = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
        if let Some(n) = &self.notes {
            out.push_str(&format!("notes = \"{}\"\n", quote(n)));
        }
        if let Some(t) = self.title.as_deref().filter(|t| !t.trim().is_empty()) {
            out.push_str(&format!("title = \"{}\"\n", quote(t.trim())));
        }
        if let Some(a) = self.author.as_deref().filter(|a| !a.trim().is_empty()) {
            out.push_str(&format!("author = \"{}\"\n", quote(a.trim())));
        }
        if let Some(d) = self.date {
            out.push_str(&format!("date = \"{}\"\n", d.key()));
        }
        out
    }

    /// Unknown keys and comments are skipped; `None` without a known kind.
    pub fn parse(text: &str) -> Option<Settings> {
        let mut kind = None;
        let mut notes = None;
        let (mut title, mut author, mut date) = (None, None, None);
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
            let value = value.replace("\\\"", "\"").replace("\\\\", "\\");
            match key.trim() {
                "kind" => kind = Kind::from_key(&value),
                "notes" => notes = Some(value).filter(|v| safe_relative(v)),
                "title" => title = Some(value).filter(|v| !v.trim().is_empty()),
                "author" => author = Some(value).filter(|v| !v.trim().is_empty()),
                "date" => date = DateHeading::from_key(&value),
                _ => {}
            }
        }
        Some(Settings { kind: kind?, notes, title, author, date })
    }
}

/// Relative, no `..`, nothing hidden.
fn safe_relative(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.trim().is_empty()
        && p.is_relative()
        && p.components().all(|c| matches!(c, std::path::Component::Normal(n) if !n.to_string_lossy().starts_with('.')))
}

pub fn read_settings(root: &Path) -> Option<Settings> {
    Settings::parse(&fs::read_to_string(root.join(SETTINGS_FILE)).ok()?)
}

/// What a journal at `root` starts a new day with.
pub fn date_heading(root: &Path) -> DateHeading {
    read_settings(root).and_then(|s| s.date).unwrap_or_default()
}

/// A book or a journal writes `.dostoevsky`; a plain folder stays untouched unless it has one.
pub fn keeps_settings_file(root: &Path, s: &Settings) -> bool {
    s.kind != Kind::Folder || root.join(SETTINGS_FILE).exists()
}

/// A title, author or date left `None` keeps what the file says; `Some("")` clears a title or author.
pub fn write_settings(root: &Path, s: &Settings) -> io::Result<()> {
    let mut s = s.clone();
    if s.title.is_none() || s.author.is_none() || s.date.is_none() {
        if let Some(old) = read_settings(root) {
            s.title = s.title.or(old.title);
            s.author = s.author.or(old.author);
            s.date = s.date.or(old.date);
        }
    }
    files::write_atomic(&root.join(SETTINGS_FILE), s.to_text().as_bytes())
}

/// (value, digits, rest): `01-the-garret` → (1, 2, "-the-garret"); `3d-printing` isn't numbered.
pub fn parse_number(stem: &str) -> Option<(u32, usize, &str)> {
    let digits = stem.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 6 {
        return None;
    }
    let rest = &stem[digits..];
    if !(rest.is_empty() || rest.starts_with(['-', '_', ' ', '.'])) {
        return None;
    }
    Some((stem[..digits].parse().ok()?, digits, rest))
}

pub fn parse_day(stem: &str) -> Option<Day> {
    let b = stem.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    if b.get(10).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let (year, month, day) = (stem.get(0..4)?.parse().ok()?, stem.get(5..7)?.parse().ok()?, stem.get(8..10)?.parse().ok()?);
    chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    Some(Day { year, month, day })
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.as_bytes(), b.as_bytes());
    loop {
        match (x.first(), y.first()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let n = x.iter().take_while(|c| c.is_ascii_digit()).count();
                let m = y.iter().take_while(|c| c.is_ascii_digit()).count();
                let trim = |d: &[u8]| -> Vec<u8> { d.iter().copied().skip_while(|c| *c == b'0').collect() };
                let (d, e) = (trim(&x[..n]), trim(&y[..m]));
                let ord = d.len().cmp(&e.len()).then_with(|| d.cmp(&e));
                if ord != Ordering::Equal {
                    return ord;
                }
                x = &x[n..];
                y = &y[m..];
            }
            (Some(p), Some(q)) => {
                if p != q {
                    return p.cmp(q);
                }
                x = &x[1..];
                y = &y[1..];
            }
        }
    }
}

pub fn spoken(name: &str) -> String {
    name.replace(['-', '_'], " ").trim().to_string()
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn folder_name(root: &Path) -> String {
    file_name(root)
}

/// Dot folders and files are skipped and symlinked folders aren't followed, so a loop can't trap the walk.
fn walk(root: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if file_name(&path).starts_with('.') || (dir == root && (file_name(&path) == DRAWER_FILE || file_name(&path) == CUTS_FILE)) {
                continue;
            }
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                dirs.push(path);
            } else if t.is_file() && files::is_markdown(&path) {
                let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                out.push((path, modified));
            }
        }
    }
    out
}

pub fn subfolders(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| natural_cmp(a, b));
    out
}

/// Folders holding none of `kind`'s chapters or days; in a plain folder any.
pub fn notes_candidates(root: &Path, kind: Kind) -> Vec<String> {
    let all = subfolders(root);
    if kind == Kind::Folder {
        return all;
    }
    let tree = scan_as(root, Some(Settings { kind, notes: None, title: None, author: None, date: None }));
    all.into_iter().filter(|f| !tree.in_order().any(|e| e.path.starts_with(root.join(f)))).collect()
}

/// Best first.
const NOTES_NAMES: [&str; 6] = ["notes", "research", "material", "materials", "reference", "codex"];

/// One with a usual name first; a plain folder only counts one called `notes`.
fn guess_notes(root: &Path, entries: &[Entry], kind: Kind) -> Option<String> {
    let top = |e: &Entry| -> Option<String> {
        let rel = e.path.strip_prefix(root).ok()?;
        let mut parts = rel.iter();
        let first = parts.next()?;
        parts.next().map(|_| first.to_string_lossy().into_owned())
    };
    let mut candidates: Vec<String> = Vec::new();
    let mut ruled_out: HashSet<String> = HashSet::new();
    for e in entries {
        let Some(dir) = top(e) else { continue };
        let main = match kind {
            Kind::Book => e.number.is_some(),
            Kind::Journal => e.day.is_some(),
            Kind::Folder => false,
        };
        if main {
            ruled_out.insert(dir);
        } else if !candidates.contains(&dir) {
            candidates.push(dir);
        }
    }
    candidates.retain(|d| !ruled_out.contains(d));
    if kind == Kind::Folder {
        return candidates.into_iter().find(|d| d.eq_ignore_ascii_case("notes"));
    }
    for name in NOTES_NAMES {
        if let Some(d) = candidates.iter().find(|d| d.eq_ignore_ascii_case(name)) {
            return Some(d.clone());
        }
    }
    candidates.sort_by(|a, b| natural_cmp(a, b));
    candidates.into_iter().next()
}

/// A kind needs one file in five to follow its pattern, so a couple of stray names in a large
/// folder don't turn it into a book, and a short book with plenty of notes stays one.
fn detect(numbered: usize, dated: usize, total: usize) -> Kind {
    let enough = |n: usize| n > 0 && n * 5 >= total;
    if enough(dated) && dated >= numbered {
        Kind::Journal
    } else if enough(numbered) {
        Kind::Book
    } else {
        Kind::Folder
    }
}

fn rel_title(root: &Path, dir: &Path) -> Option<String> {
    let rel = dir.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    Some(rel.iter().map(|c| spoken(&c.to_string_lossy())).collect::<Vec<_>>().join(" / "))
}

fn by_folder(root: &Path, entries: Vec<Entry>, order: impl Fn(&Entry, &Entry) -> Ordering) -> Vec<Group> {
    let mut map: BTreeMap<PathBuf, Vec<Entry>> = BTreeMap::new();
    for e in entries {
        map.entry(e.path.parent().unwrap_or(root).to_path_buf()).or_default().push(e);
    }
    let mut groups: Vec<Group> = map
        .into_iter()
        .map(|(dir, mut entries)| {
            entries.sort_by(&order);
            Group { title: rel_title(root, &dir), dir, entries }
        })
        .collect();
    groups.sort_by(|a, b| match (&a.title, &b.title) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(x), Some(y)) => natural_cmp(x, y),
    });
    groups
}

fn name_order(a: &Entry, b: &Entry) -> Ordering {
    natural_cmp(&file_name(&a.path), &file_name(&b.path))
}

fn chapter_order(a: &Entry, b: &Entry) -> Ordering {
    a.number.cmp(&b.number).then_with(|| name_order(a, b))
}

/// For a plain folder, what Application Support remembers. `None` until the user chooses.
pub fn chosen_settings(root: &Path) -> Option<Settings> {
    read_settings(root).or_else(|| {
        let m = files::project_memory(root);
        Some(Settings { kind: Kind::from_key(m.kind.as_deref()?)?, notes: m.notes.filter(|n| safe_relative(n)), title: None, author: None, date: None })
    })
}

pub fn open(root: &Path) -> Tree {
    scan_as(root, chosen_settings(root))
}

#[cfg(test)]
pub fn scan(root: &Path) -> Tree {
    scan_as(root, read_settings(root))
}

/// Lists only names and dates, so it is quick enough for the main thread; word counts come later.
pub fn scan_as(root: &Path, settings: Option<Settings>) -> Tree {
    let found = walk(root);
    let total = found.len();
    let entries: Vec<Entry> = found
        .into_iter()
        .map(|(path, modified)| {
            let s = stem(&path);
            let day = parse_day(&s);
            let number = if day.is_some() { None } else { parse_number(&s).map(|(n, ..)| n) };
            let label = s.clone();
            Entry { path, label, number, day, modified }
        })
        .collect();
    let numbered = entries.iter().filter(|e| e.number.is_some()).count();
    let dated = entries.iter().filter(|e| e.day.is_some()).count();
    let counts = Counts { numbered, dated, total };
    let chosen = settings.is_some();
    let (kind, notes) = match settings {
        Some(s) => (s.kind, s.notes),
        None => {
            let kind = detect(numbered, dated, total);
            (kind, guess_notes(root, &entries, kind))
        }
    };
    let notes_dir = notes.map(|n| root.join(n));
    let (notes, entries): (Vec<Entry>, Vec<Entry>) = entries.into_iter().partition(|e| notes_dir.as_ref().is_some_and(|d| e.path.starts_with(d)));
    let mut tree = Tree { root: root.to_path_buf(), kind, chosen, notes_dir: notes_dir.clone(), main: Vec::new(), notes: Vec::new(), loose: Vec::new(), counts };
    if let Some(d) = &notes_dir {
        tree.notes = by_folder(d, notes, name_order);
    }
    let loose = |entries: Vec<Entry>| -> Vec<Entry> {
        let mut out: Vec<Entry> = entries
            .into_iter()
            .map(|mut e| {
                if let Some(dir) = e.path.parent().and_then(|d| rel_title(root, d)) {
                    e.label = format!("{dir} / {}", e.label);
                }
                e
            })
            .collect();
        out.sort_by(|a, b| natural_cmp(&a.label, &b.label));
        out
    };
    match kind {
        Kind::Book => {
            let (chapters, rest): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| e.number.is_some());
            let chapters = chapters
                .into_iter()
                .map(|mut e| {
                    let s = stem(&e.path);
                    let rest = parse_number(&s).map_or("", |(.., r)| r);
                    let name = spoken(rest.trim_start_matches(['-', '_', ' ', '.']));
                    e.label = if name.is_empty() { s.clone() } else { name };
                    e
                })
                .collect();
            tree.main = by_folder(root, chapters, chapter_order);
            tree.loose = loose(rest);
        }
        Kind::Journal => {
            let (days, rest): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| e.day.is_some());
            tree.main = by_month(days, root);
            tree.loose = loose(rest);
        }
        Kind::Folder => {
            tree.main = by_folder(root, entries, |a, b| b.modified.cmp(&a.modified).then_with(|| name_order(a, b)));
        }
    }
    tree
}

fn by_month(mut days: Vec<Entry>, root: &Path) -> Vec<Group> {
    days.sort_by(|a, b| b.day.cmp(&a.day).then_with(|| name_order(a, b)));
    let mut groups: Vec<Group> = Vec::new();
    let mut last: Option<(i32, u32)> = None;
    for e in days {
        let Some(d) = e.day else { continue };
        if last != Some((d.year, d.month)) {
            // The year shows on the newest month of each year.
            let title = if last.map(|(y, _)| y) == Some(d.year) { d.month_name() } else { d.month_title() };
            groups.push(Group { title: Some(title), dir: root.to_path_buf(), entries: Vec::new() });
            last = Some((d.year, d.month));
        }
        if let Some(g) = groups.last_mut() {
            g.entries.push(e);
        }
    }
    groups
}

impl Tree {
    pub fn in_order(&self) -> impl Iterator<Item = &Entry> {
        self.main.iter().flat_map(|g| g.entries.iter())
    }

    /// The list, what's not in it, then the notes.
    pub fn all(&self) -> impl Iterator<Item = &Entry> {
        self.in_order().chain(self.loose.iter()).chain(self.notes.iter().flat_map(|g| g.entries.iter()))
    }

    pub fn is_empty(&self) -> bool {
        self.all().next().is_none()
    }

    pub fn entry(&self, path: &Path) -> Option<&Entry> {
        self.all().find(|e| e.path == path)
    }

    pub fn is_note(&self, path: &Path) -> bool {
        self.notes.iter().any(|g| g.entries.iter().any(|e| e.path == path))
    }

    pub fn is_loose(&self, path: &Path) -> bool {
        self.loose.iter().any(|e| e.path == path)
    }

    /// With a trailing slash: `research/`.
    pub fn notes_name(&self) -> Option<String> {
        let d = self.notes_dir.as_ref()?;
        Some(format!("{}/", d.strip_prefix(&self.root).unwrap_or(d).display()))
    }

    pub fn settings(&self) -> Settings {
        let notes = self.notes_dir.as_ref().and_then(|d| d.strip_prefix(&self.root).ok()).map(|r| r.to_string_lossy().into_owned());
        Settings { kind: self.kind, notes, title: None, author: None, date: None }
    }

    pub fn holds(&self, path: &Path) -> bool {
        path.starts_with(&self.root) && files::is_markdown(path) && !path.strip_prefix(&self.root).is_ok_and(|r| r.iter().any(|c| c.to_string_lossy().starts_with('.')))
    }

    /// Beside the newest entry, so year folders keep working.
    pub fn today_path(&self, day: Day) -> PathBuf {
        let year_dir = self.root.join(day.year.to_string());
        if self.in_order().next().is_none() && year_dir.is_dir() {
            return year_dir.join(day.file_name());
        }
        let dir = self.in_order().next().and_then(|e| e.path.parent()).unwrap_or(&self.root);
        let plain = dir.join(day.file_name());
        match dir.file_name().and_then(|n| n.to_str()).and_then(|n| n.parse::<i32>().ok()) {
            Some(y) if y != day.year && (1900..3000).contains(&y) => dir.with_file_name(day.year.to_string()).join(day.file_name()),
            _ => plain,
        }
    }

    pub fn entry_for_day(&self, day: Day) -> Option<&Entry> {
        self.in_order().find(|e| e.day == Some(day))
    }

    pub fn opening_file(&self, last: Option<&Path>, today: Day) -> Option<PathBuf> {
        if self.kind == Kind::Journal {
            return Some(self.entry_for_day(today).map_or_else(|| self.today_path(today), |e| e.path.clone()));
        }
        last.filter(|p| self.entry(p).is_some()).map(Path::to_path_buf).or_else(|| self.all().next().map(|e| e.path.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub from: PathBuf,
    pub to: PathBuf,
}

pub fn slug(name: &str) -> String {
    let cleaned: String = name.trim().chars().filter(|c| !matches!(c, '/' | '\\' | ':') && !c.is_control()).collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join("-");
    let joined = joined.trim_start_matches('.').to_string();
    if joined.is_empty() { "untitled".into() } else { joined }
}

fn with_number(path: &Path, dir: &Path, n: u32) -> PathBuf {
    let s = stem(path);
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "md".into());
    let (width, rest) = parse_number(&s).map_or((2, format!("-{s}")), |(_, w, r)| (w, r.to_string()));
    dir.join(format!("{n:0width$}{rest}.{ext}"))
}

/// Numbers for `order` (each with the number it has now, if any) that change as little as they
/// can: everything keeps its number until the one before catches up with it.
fn fitted(order: &[Option<u32>]) -> Vec<u32> {
    let mut out = Vec::with_capacity(order.len());
    let mut prev = 0;
    for n in order {
        let want = n.unwrap_or(prev + 1).max(prev + 1);
        out.push(want);
        prev = want;
    }
    out
}

/// `None` goes last. Only files whose number changes are touched; within a folder the numbers
/// in use stay in use and change hands, moving to another folder opens a gap there.
pub fn plan_move(tree: &Tree, path: &Path, dir: &Path, before: Option<&Path>) -> Vec<Rename> {
    let Some(group) = tree.main.iter().find(|g| g.dir == dir) else { return Vec::new() };
    let Some(moved) = tree.entry(path) else { return Vec::new() };
    let mut order: Vec<&Entry> = group.entries.iter().filter(|e| e.path != path).collect();
    let at = before.and_then(|b| order.iter().position(|e| e.path == b)).unwrap_or(order.len());
    order.insert(at, moved);
    // A file without a number (one not in the book yet) joins like one from another part.
    let same_folder = path.parent() == Some(dir) && moved.number.is_some();
    let numbers: Vec<u32> = if same_folder {
        let mut n: Vec<u32> = group.entries.iter().filter_map(|e| e.number).collect();
        n.sort_unstable();
        n
    } else {
        fitted(&order.iter().map(|e| if e.path == path { None } else { e.number }).collect::<Vec<_>>())
    };
    order
        .iter()
        .zip(numbers)
        .filter_map(|(e, n)| {
            let to = with_number(&e.path, dir, n);
            (to != e.path).then(|| Rename { from: e.path.clone(), to })
        })
        .collect()
}

/// `None` goes first when the group has files. Returns its path and the renames that make room.
pub fn plan_new_chapter(tree: &Tree, dir: &Path, after: Option<&Path>, name: &str) -> (PathBuf, Vec<Rename>) {
    let empty = Vec::new();
    let entries = tree.main.iter().find(|g| g.dir == dir).map_or(&empty, |g| &g.entries);
    let at = match after {
        Some(a) => entries.iter().position(|e| e.path == a).map_or(entries.len(), |i| i + 1),
        None => 0,
    };
    let mut order: Vec<Option<u32>> = entries.iter().map(|e| e.number).collect();
    order.insert(at, None);
    let numbers = fitted(&order);
    let width = entries.first().and_then(|e| parse_number(&stem(&e.path)).map(|(_, w, _)| w)).unwrap_or(2);
    let n = numbers[at];
    let path = dir.join(format!("{n:0width$}-{}.md", slug(name)));
    let mut renames = Vec::new();
    for (e, n) in entries.iter().zip(numbers.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, n)| *n)) {
        let to = with_number(&e.path, dir, n);
        if to != e.path {
            renames.push(Rename { from: e.path.clone(), to });
        }
    }
    (path, renames)
}

/// A chapter keeps its number and every file its extension; in a journal only what follows the date changes.
pub fn renamed(tree: &Tree, path: &Path, name: &str) -> PathBuf {
    let dir = path.parent().unwrap_or(&tree.root);
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "md".into());
    let s = stem(path);
    let new = match (tree.kind, tree.entry(path)) {
        (Kind::Book, Some(e)) if e.number.is_some() => {
            let (_, w, _) = parse_number(&s).unwrap_or((0, 2, ""));
            format!("{}-{}", &s[..w], slug(name))
        }
        (Kind::Journal, Some(e)) if e.day.is_some() => format!("{} {}", &s[..10], slug(name)),
        _ => slug(name),
    };
    dir.join(format!("{new}.{ext}"))
}

/// A chapter leaves its number behind, since numbers are the book's order.
pub fn plan_to_notes(tree: &Tree, path: &Path, notes_dir: &Path) -> Rename {
    let s = stem(path);
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "md".into());
    let name = match (tree.kind, parse_number(&s)) {
        (Kind::Book, Some((.., rest))) if !rest.trim_start_matches(['-', '_', ' ', '.']).is_empty() => rest.trim_start_matches(['-', '_', ' ', '.']).to_string(),
        _ => s,
    };
    Rename { from: path.to_path_buf(), to: unused(&notes_dir.join(format!("{name}.{ext}"))) }
}

pub fn plan_to_list(tree: &Tree, path: &Path) -> Rename {
    let s = stem(path);
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "md".into());
    let to = match tree.kind {
        Kind::Book => {
            let group = tree.main.last();
            let dir = group.map_or(tree.root.clone(), |g| g.dir.clone());
            let next = group.and_then(|g| g.entries.iter().filter_map(|e| e.number).max()).map_or(1, |n| n + 1);
            let width = group.and_then(|g| g.entries.first()).and_then(|e| parse_number(&stem(&e.path)).map(|(_, w, _)| w)).unwrap_or(2);
            let name = match parse_number(&s) {
                Some((.., rest)) => rest.trim_start_matches(['-', '_', ' ', '.']).to_string(),
                None => s.clone(),
            };
            let name = if name.is_empty() { "untitled".to_string() } else { name };
            dir.join(format!("{next:0width$}-{name}.{ext}"))
        }
        Kind::Journal => {
            let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
            let day = modified.map_or_else(Day::today, |t| {
                use chrono::Datelike as _;
                let d = chrono::DateTime::<chrono::Local>::from(t).date_naive();
                Day { year: d.year(), month: d.month(), day: d.day() }
            });
            let at = tree.today_path(day);
            let dir = at.parent().unwrap_or(&tree.root);
            let name = if parse_day(&s).is_some() { s.clone() } else { format!("{} {s}", &day.file_name()[..10]) };
            dir.join(format!("{name}.{ext}"))
        }
        Kind::Folder => tree.root.join(format!("{s}.{ext}")),
    };
    Rename { from: path.to_path_buf(), to: if to == path { to } else { unused(&to) } }
}

pub fn plan_numbering(order: &[PathBuf]) -> Vec<Rename> {
    order
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let dir = p.parent()?;
            let ext = p.extension()?.to_string_lossy().into_owned();
            let name = stem(p);
            Some(Rename { from: p.clone(), to: dir.join(format!("{:02}-{name}.{ext}", i + 1)) })
        })
        .collect()
}

pub fn unused(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let (s, ext) = (stem(path), path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default());
    let dir = path.parent().unwrap_or(Path::new("."));
    (2..).map(|i| dir.join(format!("{s}-{i}.{ext}"))).find(|p| !p.exists()).unwrap_or_else(|| path.to_path_buf())
}

/// All or none. Chains (`02 → 03`, `03 → 04`) are safe: everything passes through a private name first.
pub fn apply(renames: &[Rename]) -> io::Result<()> {
    // A save on its way would land under the old name after the move.
    crate::files::wait_for_writes(None);
    let leaving: HashSet<&Path> = renames.iter().map(|r| r.from.as_path()).collect();
    let mut targets = HashSet::new();
    for r in renames {
        // Changing only the case of a name lands on itself on a case-insensitive disk.
        let itself = r.to.as_os_str().eq_ignore_ascii_case(r.from.as_os_str());
        if !targets.insert(r.to.as_path()) || (!leaving.contains(r.to.as_path()) && !itself && r.to.exists()) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, tr!("{path} already exists", path = r.to.display())));
        }
    }
    let parked = |i: usize, r: &Rename| r.from.with_file_name(format!(".dostoevsky-move-{}-{i}", std::process::id()));
    for (i, r) in renames.iter().enumerate() {
        if let Err(e) = fs::rename(&r.from, parked(i, r)) {
            for (j, q) in renames[..i].iter().enumerate() {
                let _ = fs::rename(parked(j, q), &q.from);
            }
            return Err(e);
        }
    }
    for (i, r) in renames.iter().enumerate() {
        if let Some(dir) = r.to.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Err(e) = fs::rename(parked(i, r), &r.to) {
            for (j, q) in renames[..i].iter().enumerate() {
                let _ = fs::rename(&q.to, parked(j, q));
            }
            for (j, q) in renames.iter().enumerate() {
                let _ = fs::rename(parked(j, q), &q.from);
            }
            return Err(e);
        }
    }
    Ok(())
}

pub fn reversed(renames: &[Rename]) -> Vec<Rename> {
    renames.iter().map(|r| Rename { from: r.to.clone(), to: r.from.clone() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let d = std::env::temp_dir().join(format!("dsky-project-{name}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            Dir(d.canonicalize().unwrap())
        }

        fn with(self, names: &[&str]) -> Dir {
            for n in names {
                let p = self.0.join(n);
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(p, n.as_bytes()).unwrap();
            }
            self
        }

        fn names(&self) -> Vec<String> {
            let mut out: Vec<String> =
                walk(&self.0).into_iter().map(|(p, _)| p.strip_prefix(&self.0).unwrap().to_string_lossy().into_owned()).collect();
            out.sort();
            out
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn labels(g: &Group) -> Vec<&str> {
        g.entries.iter().map(|e| e.label.as_str()).collect()
    }

    #[test]
    fn numbers_and_days_are_read_from_the_start_of_a_name() {
        assert_eq!(parse_number("01-the-garret"), Some((1, 2, "-the-garret")));
        assert_eq!(parse_number("12"), Some((12, 2, "")));
        assert_eq!(parse_number("3 men"), Some((3, 1, " men")));
        assert_eq!(parse_number("3d-printing"), None);
        assert_eq!(parse_number("notes"), None);
        assert_eq!(parse_day("2026-11-01"), Some(Day { year: 2026, month: 11, day: 1 }));
        assert_eq!(parse_day("2026-11-01 evening").map(|d| d.day), Some(1));
        assert_eq!(parse_day("2026-13-01"), None);
        assert_eq!(parse_day("2026-02-30"), None);
        assert_eq!(parse_day("2026-11"), None);
    }

    #[test]
    fn natural_order_reads_numbers_as_numbers() {
        let mut v = vec!["part-10", "part-2", "Part-1", "appendix"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["appendix", "Part-1", "part-2", "part-10"]);
        assert_eq!(natural_cmp("01-a", "1-a"), Ordering::Equal);
    }

    #[test]
    fn numbered_files_suggest_a_book_with_parts_notes_and_strays() {
        let d = Dir::new("book").with(&[
            "part-2/01-the-summons.md",
            "part-1/02-marmeladov.md",
            "part-1/10-the-end.md",
            "part-1/01-the-garret.md",
            "people.md",
            "research/fever.md",
            ".git/01-not-mine.md",
            "cover.png",
        ]);
        let t = scan(&d.0);
        assert_eq!(t.kind, Kind::Book);
        assert!(!t.chosen, "a guess until the user says so");
        assert_eq!(t.counts, Counts { numbered: 4, dated: 0, total: 6 });
        assert_eq!(t.main.len(), 2);
        assert_eq!(t.main[0].title.as_deref(), Some("part 1"));
        assert_eq!(labels(&t.main[0]), ["the garret", "marmeladov", "the end"]);
        assert_eq!(labels(&t.main[1]), ["the summons"]);
        assert_eq!(t.notes_name().as_deref(), Some("research/"));
        assert_eq!(labels(&t.notes[0]), ["fever"]);
        assert_eq!(t.notes[0].title, None);
        assert!(t.is_note(&d.0.join("research/fever.md")));
        assert!(t.is_loose(&d.0.join("people.md")));
        assert!(!t.is_note(&d.0.join("people.md")));
        assert_eq!(t.all().count(), 6);
    }

    #[test]
    fn the_choice_in_the_folder_wins_over_the_names() {
        let d = Dir::new("chosen").with(&["01-a.md", "02-b.md", "notes/people.md", "plans/outline.md", "loose.md"]);
        write_settings(&d.0, &Settings { kind: Kind::Folder, notes: Some("plans".into()), title: None, author: None, date: None }).unwrap();
        let t = scan(&d.0);
        assert!(t.chosen);
        assert_eq!(t.kind, Kind::Folder);
        assert_eq!(t.notes_name().as_deref(), Some("plans/"));
        assert_eq!(t.notes.iter().map(|g| g.entries.len()).sum::<usize>(), 1);
        assert!(t.loose.is_empty(), "a plain folder lists everything");
        assert_eq!(t.in_order().count(), 4);
        assert_eq!(d.names(), ["01-a.md", "02-b.md", "loose.md", "notes/people.md", "plans/outline.md"]);
        assert!(!t.holds(&d.0.join(SETTINGS_FILE)));
    }

    #[test]
    fn a_chosen_book_lists_strays_after_the_chapters() {
        let d = Dir::new("strays").with(&["01-a.md", "part-2/01-b.md", "part-2/outline.md", "people.md", "notes/places.md"]);
        let t = scan_as(&d.0, Some(Settings { kind: Kind::Book, notes: Some("notes".into()), title: None, author: None, date: None }));
        assert_eq!(t.loose.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["part 2 / outline", "people"]);
        assert_eq!(labels(&t.notes[0]), ["places"]);
        assert_eq!(t.in_order().count(), 2);
        let no_notes = scan_as(&d.0, Some(Settings { kind: Kind::Book, notes: None, title: None, author: None, date: None }));
        assert!(no_notes.notes.is_empty());
        assert_eq!(no_notes.loose.len(), 3);
    }

    #[test]
    fn settings_read_back_and_shrug_off_junk() {
        let s = Settings { kind: Kind::Journal, notes: Some("my \"notes\"".into()), title: None, author: None, date: None };
        assert_eq!(Settings::parse(&s.to_text()), Some(s));
        assert_eq!(Settings::parse("kind = book"), Some(Settings { kind: Kind::Book, notes: None, title: None, author: None, date: None }));
        assert_eq!(Settings::parse("# nothing\nnotes = \"x\""), None);
        assert_eq!(Settings::parse("kind = \"novel\""), None);
        assert_eq!(Settings::parse("kind=\"Book\"\nnotes = \"../elsewhere\"\ncolour = \"red\""), Some(Settings { kind: Kind::Book, notes: None, title: None, author: None, date: None }));
        assert_eq!(Settings::parse("kind = \"book\"\nnotes = \".git\""), Some(Settings { kind: Kind::Book, notes: None, title: None, author: None, date: None }));
    }

    #[test]
    fn only_books_and_journals_write_into_the_folder() {
        let d = Dir::new("keeps");
        let folder = Settings { kind: Kind::Folder, notes: None, title: None, author: None, date: None };
        assert!(!keeps_settings_file(&d.0, &folder));
        assert!(keeps_settings_file(&d.0, &Settings { kind: Kind::Book, notes: None, title: None, author: None, date: None }));
        write_settings(&d.0, &Settings { kind: Kind::Book, notes: None, title: None, author: None, date: None }).unwrap();
        assert!(keeps_settings_file(&d.0, &folder), "once there, the file is kept up to date");
    }

    #[test]
    fn title_and_author_live_in_the_settings_file_and_survive_a_type_change() {
        let d = Dir::new("titled");
        let s = Settings { kind: Kind::Book, notes: None, title: Some("Crime and \"Punishment\"".into()), author: Some("Fyodor Dostoevsky".into()), date: None };
        write_settings(&d.0, &s).unwrap();
        assert_eq!(read_settings(&d.0), Some(s.clone()));
        write_settings(&d.0, &Settings { kind: Kind::Book, notes: Some("notes".into()), title: None, author: None, date: None }).unwrap();
        let back = read_settings(&d.0).unwrap();
        assert_eq!((back.title.as_deref(), back.author.as_deref(), back.notes.as_deref()), (Some("Crime and \"Punishment\""), Some("Fyodor Dostoevsky"), Some("notes")));
        write_settings(&d.0, &Settings { kind: Kind::Book, notes: None, title: Some(String::new()), author: None, date: None }).unwrap();
        assert_eq!(read_settings(&d.0).unwrap().title, None);
    }

    /// PROJ-6
    #[test]
    fn a_day_has_a_heading_in_each_form() {
        let day = Day { year: 2026, month: 11, day: 15 };
        assert_eq!(day.heading(DateHeading::Long).as_deref(), Some("Sunday, 15 November 2026"));
        assert_eq!(day.heading(DateHeading::Short).as_deref(), Some("15 Nov 2026"));
        assert_eq!(day.heading(DateHeading::Iso).as_deref(), Some("2026-11-15"));
        assert_eq!(day.heading(DateHeading::Off), None);
        assert_eq!(Day { year: 2027, month: 1, day: 4 }.heading(DateHeading::Long).as_deref(), Some("Monday, 4 January 2027"));
        assert!(day.is_heading("# Sunday, 15 November 2026"));
        assert!(day.is_heading("## 2026-11-15 "));
        assert!(!day.is_heading("2026-11-15"), "only as a heading");
        assert!(!day.is_heading("# 15 Nov 2025"));
        assert!(!day.is_heading("# Trip to the river"));
        assert!(day.is_heading("# воскресенье, 15 ноября 2026 г."), "written under another UI language");
        assert!(day.is_heading("# Воскресенье, 15 ноября 2026"), "as Dostoevsky 1.0 wrote it");
        assert!(!day.is_heading("# воскресенье, 15 ноября 2025 г."));
    }

    /// PROJ-6
    #[test]
    fn the_date_form_lives_in_the_settings_file_and_survives_other_changes() {
        let d = Dir::new("dated");
        assert_eq!(date_heading(&d.0), DateHeading::Long, "the default, with no file");
        write_settings(&d.0, &Settings { kind: Kind::Journal, notes: None, title: None, author: None, date: None }).unwrap();
        assert_eq!(date_heading(&d.0), DateHeading::Long, "the default, with a file that says nothing");
        assert!(!fs::read_to_string(d.0.join(SETTINGS_FILE)).unwrap().contains("date"));
        write_settings(&d.0, &Settings { kind: Kind::Journal, notes: None, title: None, author: None, date: Some(DateHeading::Iso) }).unwrap();
        assert!(fs::read_to_string(d.0.join(SETTINGS_FILE)).unwrap().contains("date = \"iso\""));
        write_settings(&d.0, &Settings { kind: Kind::Journal, notes: Some("notes".into()), title: None, author: None, date: None }).unwrap();
        assert_eq!(date_heading(&d.0), DateHeading::Iso);
        write_settings(&d.0, &Settings { kind: Kind::Journal, notes: None, title: None, author: None, date: Some(DateHeading::Off) }).unwrap();
        assert_eq!(date_heading(&d.0), DateHeading::Off);
        assert_eq!(Settings::parse("kind = journal\ndate = \"Short\"").and_then(|s| s.date), Some(DateHeading::Short));
        assert_eq!(Settings::parse("kind = journal\ndate = \"%Y\"").and_then(|s| s.date), None);
    }

    #[test]
    fn the_notes_folder_guess_prefers_usual_names() {
        let d = Dir::new("guess").with(&["01-a.md", "02-b.md", "drafts/old.md", "Research/fever.md", "part-1/03-c.md"]);
        assert_eq!(scan(&d.0).notes_name().as_deref(), Some("Research/"));
        let d = Dir::new("guess-any").with(&["01-a.md", "02-b.md", "zzz/x.md", "drafts/old.md"]);
        assert_eq!(scan(&d.0).notes_name().as_deref(), Some("drafts/"));
        let d = Dir::new("guess-none").with(&["01-a.md", "part-1/02-b.md"]);
        assert_eq!(scan(&d.0).notes_dir, None);
        let d = Dir::new("guess-folder").with(&["a.md", "drafts/x.md", "notes/y.md"]);
        assert_eq!(scan(&d.0).notes_name().as_deref(), Some("notes/"));
        let d = Dir::new("guess-folder-2").with(&["a.md", "drafts/x.md"]);
        assert_eq!(scan(&d.0).notes_dir, None);
    }

    #[test]
    fn a_folder_of_chapters_is_never_offered_for_notes() {
        let d = Dir::new("candidates").with(&["part-1/01-a.md", "part-2/01-b.md", "research/x.md", "2026/2026-01-01.md"]);
        assert_eq!(notes_candidates(&d.0, Kind::Book), ["2026", "research"]);
        assert_eq!(notes_candidates(&d.0, Kind::Journal), ["part-1", "part-2", "research"]);
        assert_eq!(notes_candidates(&d.0, Kind::Folder).len(), 4);
    }

    #[test]
    fn move_to_notes_leaves_the_number_behind() {
        let d = Dir::new("to-notes").with(&["01-a.md", "02-the-visit.md", "people.md", "notes/people.md"]);
        let t = scan_as(&d.0, Some(Settings { kind: Kind::Book, notes: Some("notes".into()), title: None, author: None, date: None }));
        let n = d.0.join("notes");
        assert_eq!(plan_to_notes(&t, &d.0.join("02-the-visit.md"), &n).to, n.join("the-visit.md"));
        assert_eq!(plan_to_notes(&t, &d.0.join("people.md"), &n).to, n.join("people-2.md"));
        let r = plan_to_notes(&t, &d.0.join("01-a.md"), &d.0.join("new-notes"));
        apply(&[r]).unwrap();
        assert!(d.0.join("new-notes/a.md").exists(), "the folder is made on the way");
    }

    #[test]
    fn add_to_the_book_takes_the_next_number_in_the_last_part() {
        let d = Dir::new("to-book").with(&["part-1/01-a.md", "part-2/001-b.md", "part-2/007-c.md", "people.md", "notes/03-old.md"]);
        let t = scan_as(&d.0, Some(Settings { kind: Kind::Book, notes: Some("notes".into()), title: None, author: None, date: None }));
        assert_eq!(plan_to_list(&t, &d.0.join("people.md")).to, d.0.join("part-2/008-people.md"));
        assert_eq!(plan_to_list(&t, &d.0.join("notes/03-old.md")).to, d.0.join("part-2/008-old.md"));
        let empty = Dir::new("to-book-empty").with(&["people.md"]);
        let t = scan_as(&empty.0, Some(Settings { kind: Kind::Book, notes: None, title: None, author: None, date: None }));
        assert_eq!(plan_to_list(&t, &empty.0.join("people.md")).to, empty.0.join("01-people.md"));
    }

    #[test]
    fn add_to_the_journal_dates_a_file_by_when_it_changed() {
        let d = Dir::new("to-journal").with(&["2026/2026-11-01.md", "ideas.md", "notes/2026-10-02.md"]);
        let t = scan_as(&d.0, Some(Settings { kind: Kind::Journal, notes: Some("notes".into()), title: None, author: None, date: None }));
        let to = plan_to_list(&t, &d.0.join("ideas.md")).to;
        let name = to.file_name().unwrap().to_string_lossy().into_owned();
        assert!(parse_day(&name[..10]).is_some() && name.ends_with(" ideas.md"), "{name}");
        assert_eq!(plan_to_list(&t, &d.0.join("notes/2026-10-02.md")).to, d.0.join("2026/2026-10-02.md"));
        let f = Dir::new("to-folder").with(&["a.md", "notes/b.md"]);
        let t = scan_as(&f.0, Some(Settings { kind: Kind::Folder, notes: Some("notes".into()), title: None, author: None, date: None }));
        assert_eq!(plan_to_list(&t, &f.0.join("notes/b.md")).to, f.0.join("b.md"));
    }

    #[test]
    fn a_new_journal_with_year_folders_starts_in_this_years() {
        let d = Dir::new("years-new");
        fs::create_dir_all(d.0.join("2026")).unwrap();
        let t = scan_as(&d.0, Some(Settings { kind: Kind::Journal, notes: None, title: None, author: None, date: None }));
        assert_eq!(t.today_path(Day { year: 2026, month: 11, day: 5 }), d.0.join("2026/2026-11-05.md"));
    }

    #[test]
    fn dated_files_make_a_journal_newest_first_by_month() {
        let d = Dir::new("journal").with(&["2026-11-01.md", "2026-10-31.md", "2026-10-02.md", "2025-12-30.md", "ideas.md"]);
        let t = scan(&d.0);
        assert_eq!(t.kind, Kind::Journal);
        let titles: Vec<_> = t.main.iter().filter_map(|g| g.title.clone()).collect();
        assert_eq!(titles, ["november 2026", "october", "december 2025"]);
        assert_eq!(t.main[1].entries[0].day.map(|x| x.day), Some(31));
        assert!(t.is_loose(&d.0.join("ideas.md")));
    }

    #[test]
    fn a_day_in_a_year_folder_goes_where_the_year_says() {
        let d = Dir::new("years").with(&["2025/2025-12-30.md"]);
        let t = scan(&d.0);
        let day = Day { year: 2026, month: 1, day: 1 };
        assert_eq!(t.today_path(day), d.0.join("2026/2026-01-01.md"));
        let d = Dir::new("flat").with(&["2025-12-30.md"]);
        assert_eq!(scan(&d.0).today_path(day), d.0.join("2026-01-01.md"));
    }

    #[test]
    fn anything_else_stays_a_folder() {
        let d = Dir::new("folder").with(&["keep-the-words.md", "on-margins.md", "sub/draft.md"]);
        let t = scan(&d.0);
        assert_eq!(t.kind, Kind::Folder);
        assert!(t.notes.is_empty() && t.loose.is_empty());
        assert_eq!(t.main[1].title.as_deref(), Some("sub"));
    }

    #[test]
    fn a_stray_number_or_date_doesnt_change_what_a_folder_is() {
        assert_eq!(detect(1, 0, 40), Kind::Folder);
        assert_eq!(detect(0, 2, 40), Kind::Folder);
        assert_eq!(detect(3, 0, 13), Kind::Book);
        assert_eq!(detect(1, 0, 1), Kind::Book);
        assert_eq!(detect(3, 9, 12), Kind::Journal);
        assert_eq!(detect(0, 0, 0), Kind::Folder);
    }

    #[test]
    fn opening_a_project_shows_today_or_where_you_were() {
        let today = Day { year: 2026, month: 11, day: 1 };
        let d = Dir::new("open").with(&["01-a.md", "02-b.md"]);
        let t = scan(&d.0);
        assert_eq!(t.opening_file(None, today), Some(d.0.join("01-a.md")));
        assert_eq!(t.opening_file(Some(&d.0.join("02-b.md")), today), Some(d.0.join("02-b.md")));
        assert_eq!(t.opening_file(Some(&d.0.join("gone.md")), today), Some(d.0.join("01-a.md")));
        let j = Dir::new("open-j").with(&["2026-10-31.md"]);
        assert_eq!(scan(&j.0).opening_file(Some(&j.0.join("2026-10-31.md")), today), Some(j.0.join("2026-11-01.md")));
        let e = Dir::new("open-e");
        assert_eq!(scan(&e.0).opening_file(None, today), None);
    }

    fn moved(d: &Dir, path: &str, before: Option<&str>) {
        let t = scan(&d.0);
        let before = before.map(|b| d.0.join(b));
        let dir = d.0.join(path).parent().unwrap().to_path_buf();
        apply(&plan_move(&t, &d.0.join(path), &dir, before.as_deref())).unwrap();
    }

    #[test]
    fn moving_a_chapter_up_renames_only_the_files_it_passes() {
        let d = Dir::new("up").with(&["01-a.md", "02-b.md", "03-c.md", "04-d.md", "05-e.md"]);
        let t = scan(&d.0);
        let plan = plan_move(&t, &d.0.join("04-d.md"), &d.0, Some(&d.0.join("02-b.md")));
        assert_eq!(plan.len(), 3);
        moved(&d, "04-d.md", Some("02-b.md"));
        assert_eq!(d.names(), ["01-a.md", "02-d.md", "03-b.md", "04-c.md", "05-e.md"]);
    }

    #[test]
    fn moving_a_chapter_down_or_to_the_end_keeps_the_numbers_in_use() {
        let d = Dir::new("down").with(&["01-a.md", "02-b.md", "03-c.md", "07-d.md"]);
        moved(&d, "01-a.md", Some("07-d.md"));
        assert_eq!(d.names(), ["01-b.md", "02-c.md", "03-a.md", "07-d.md"]);
        moved(&d, "01-b.md", None);
        assert_eq!(d.names(), ["01-c.md", "02-a.md", "03-d.md", "07-b.md"]);
    }

    #[test]
    fn a_move_that_changes_nothing_renames_nothing() {
        let d = Dir::new("same").with(&["01-a.md", "02-b.md", "03-c.md"]);
        let t = scan(&d.0);
        assert!(plan_move(&t, &d.0.join("02-b.md"), &d.0, Some(&d.0.join("03-c.md"))).is_empty());
    }

    #[test]
    fn moving_to_another_part_opens_a_gap_there() {
        let d = Dir::new("parts").with(&["part-1/01-a.md", "part-1/02-b.md", "part-2/01-c.md", "part-2/02-d.md", "part-2/05-e.md"]);
        let t = scan(&d.0);
        let (from, to) = (d.0.join("part-1/02-b.md"), d.0.join("part-2"));
        apply(&plan_move(&t, &from, &to, Some(&to.join("02-d.md")))).unwrap();
        assert_eq!(d.names(), ["part-1/01-a.md", "part-2/01-c.md", "part-2/02-b.md", "part-2/03-d.md", "part-2/05-e.md"]);
    }

    #[test]
    fn number_widths_and_extensions_survive_a_move() {
        let d = Dir::new("width").with(&["001-a.markdown", "002-b.txt", "003-c.md"]);
        moved(&d, "003-c.md", Some("001-a.markdown"));
        assert_eq!(d.names(), ["001-c.md", "002-a.markdown", "003-b.txt"]);
    }

    #[test]
    fn a_new_chapter_makes_room_after_the_current_one() {
        let d = Dir::new("new").with(&["01-a.md", "02-b.md", "03-c.md", "07-d.md"]);
        let t = scan(&d.0);
        let (path, renames) = plan_new_chapter(&t, &d.0, Some(&d.0.join("01-a.md")), "the visit");
        assert_eq!(path, d.0.join("02-the-visit.md"));
        apply(&renames).unwrap();
        assert_eq!(d.names(), ["01-a.md", "03-b.md", "04-c.md", "07-d.md"]);
        let (path, renames) = plan_new_chapter(&t, &d.0, Some(&d.0.join("07-d.md")), "end");
        assert_eq!((path, renames), (d.0.join("08-end.md"), Vec::new()));
    }

    #[test]
    fn the_first_chapter_of_an_empty_part_is_numbered_one() {
        let d = Dir::new("first");
        let t = scan(&d.0);
        let (path, renames) = plan_new_chapter(&t, &d.0, None, "untitled");
        assert_eq!((path, renames), (d.0.join("01-untitled.md"), Vec::new()));
    }

    #[test]
    fn renames_are_all_or_nothing() {
        let d = Dir::new("atomic").with(&["01-a.md", "02-b.md", "03-c.md"]);
        let plan = vec![
            Rename { from: d.0.join("01-a.md"), to: d.0.join("01-x.md") },
            Rename { from: d.0.join("02-b.md"), to: d.0.join("03-c.md") },
        ];
        assert!(apply(&plan).is_err());
        assert_eq!(d.names(), ["01-a.md", "02-b.md", "03-c.md"]);
        assert_eq!(fs::read_to_string(d.0.join("03-c.md")).unwrap(), "03-c.md");
        let missing = vec![
            Rename { from: d.0.join("01-a.md"), to: d.0.join("09-a.md") },
            Rename { from: d.0.join("gone.md"), to: d.0.join("08-g.md") },
        ];
        assert!(apply(&missing).is_err());
        assert_eq!(d.names(), ["01-a.md", "02-b.md", "03-c.md"]);
        assert!(fs::read_dir(&d.0).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')));
    }

    #[test]
    fn renames_keep_each_files_words_and_reverse_cleanly() {
        let d = Dir::new("swap").with(&["01-a.md", "02-b.md"]);
        let plan = vec![
            Rename { from: d.0.join("01-a.md"), to: d.0.join("02-a.md") },
            Rename { from: d.0.join("02-b.md"), to: d.0.join("01-b.md") },
        ];
        apply(&plan).unwrap();
        assert_eq!(fs::read_to_string(d.0.join("02-a.md")).unwrap(), "01-a.md");
        apply(&reversed(&plan)).unwrap();
        assert_eq!(d.names(), ["01-a.md", "02-b.md"]);
    }

    #[test]
    fn renaming_keeps_the_number_the_date_and_the_extension() {
        let d = Dir::new("rename").with(&["01-a.markdown", "2026-11-01.md", "people.txt"]);
        let t = scan(&d.0);
        assert_eq!(renamed(&t, &d.0.join("people.txt"), "the cast"), d.0.join("the-cast.txt"));
        let b = Dir::new("rename-b").with(&["01-a.markdown", "02-b.md"]);
        assert_eq!(renamed(&scan(&b.0), &b.0.join("01-a.markdown"), "the garret"), b.0.join("01-the-garret.markdown"));
        let j = Dir::new("rename-j").with(&["2026-11-01.md"]);
        assert_eq!(renamed(&scan(&j.0), &j.0.join("2026-11-01.md"), "evening"), j.0.join("2026-11-01 evening.md"));
    }

    #[test]
    fn in_a_plain_folder_the_whole_name_is_yours_to_change() {
        let d = Dir::new("rename-f").with(&["01-intro.md", "2026-11-01 walk.md", "letter.md"]);
        let t = scan_as(&d.0, Some(Settings { kind: Kind::Folder, notes: None, title: None, author: None, date: None }));
        assert_eq!(t.kind, Kind::Folder);
        assert_eq!(renamed(&t, &d.0.join("01-intro.md"), "intro"), d.0.join("intro.md"));
        assert_eq!(renamed(&t, &d.0.join("01-intro.md"), "02-intro"), d.0.join("02-intro.md"));
        assert_eq!(renamed(&t, &d.0.join("2026-11-01 walk.md"), "walk"), d.0.join("walk.md"));
    }

    #[test]
    fn slugs_are_safe_names() {
        assert_eq!(slug("  the  visit "), "the-visit");
        assert_eq!(slug("a/b:c"), "abc");
        assert_eq!(slug("../.."), "untitled");
        assert_eq!(slug("   "), "untitled");
        assert_eq!(slug("Глава первая"), "Глава-первая");
    }

    #[test]
    fn a_folder_numbered_in_order_reads_as_a_book() {
        let d = Dir::new("number").with(&["b.md", "a.md"]);
        apply(&plan_numbering(&[d.0.join("b.md"), d.0.join("a.md")])).unwrap();
        let t = scan(&d.0);
        assert_eq!(t.kind, Kind::Book);
        assert_eq!(labels(&t.main[0]), ["b", "a"]);
    }

    #[test]
    fn unused_names_step_past_what_exists() {
        let d = Dir::new("unused").with(&["untitled.md", "untitled-2.md"]);
        assert_eq!(unused(&d.0.join("untitled.md")), d.0.join("untitled-3.md"));
        assert_eq!(unused(&d.0.join("free.md")), d.0.join("free.md"));
    }

    #[test]
    fn a_project_holds_its_markdown_but_not_what_it_hides() {
        let d = Dir::new("holds").with(&["01-a.md"]);
        let t = scan(&d.0);
        assert!(t.holds(&d.0.join("new.md")));
        assert!(!t.holds(&d.0.join("cover.png")));
        assert!(!t.holds(&d.0.join(".git/x.md")));
        assert!(!t.holds(Path::new("/elsewhere/x.md")));
    }
}
