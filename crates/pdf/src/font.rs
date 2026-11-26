//! The faces pages are set in. A family is looked for in the app's bundled font files first, then
//! among the system's by name, then among the faces that stand in for a family this system lacks
//! (Liberation Serif for Times New Roman, and so on), and last in a bundled face of the same
//! kind. A page always embeds real outlines: nothing falls to a face the PDF never names.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rustybuzz::Face;

/// A face, with the styles it is shown in when it has no bold or italic of its own.
#[derive(Clone, Copy)]
pub struct Font {
    pub rec: &'static FaceRec,
    pub faux_bold: bool,
    pub faux_italic: bool,
}

impl PartialEq for Font {
    fn eq(&self, other: &Font) -> bool {
        std::ptr::eq(self.rec, other.rec) && self.faux_bold == other.faux_bold && self.faux_italic == other.faux_italic
    }
}

impl Eq for Font {}

impl PartialOrd for Font {
    fn partial_cmp(&self, other: &Font) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Font {
    fn cmp(&self, other: &Font) -> std::cmp::Ordering {
        // By what the face is, never where it sits in memory: a file lists its faces in the same
        // order from one run to the next. The address only parts two copies of one face.
        let key = |f: &Font| (f.rec.family.as_str(), f.rec.bold, f.rec.italic, f.faux_bold, f.faux_italic, f.rec.index, f.rec.data.len(), f.rec as *const FaceRec);
        key(self).cmp(&key(other))
    }
}

impl std::hash::Hash for Font {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.rec as *const FaceRec).hash(state);
        (self.faux_bold, self.faux_italic).hash(state);
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}{}", self.rec.family, if self.faux_bold { " bold" } else { "" }, if self.faux_italic { " italic" } else { "" })
    }
}

pub struct FaceRec {
    pub face: Face<'static>,
    /// Lower-case family name.
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// The whole file, and the face's place in it (for embedding).
    pub data: &'static [u8],
    pub index: u32,
    /// Its licence lets it be embedded only whole.
    pub no_subset: bool,
}

#[derive(Default)]
struct System {
    faces: Vec<SystemFace>,
    by_family: HashMap<String, Vec<usize>>,
}

struct SystemFace {
    path: PathBuf,
    index: u32,
}

pub struct Fonts {
    bundled: Vec<Font>,
    families: HashMap<String, Vec<Font>>,
    /// The system's font folders, read the first time a family isn't bundled.
    system: OnceLock<Option<System>>,
    /// System faces already opened, by their place in the list; `None` for one that won't embed.
    loaded: Mutex<HashMap<usize, Option<Font>>>,
}

/// Where each system keeps fonts, a user's own first.
fn font_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut dirs = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(local) = var("LOCALAPPDATA") {
            dirs.push(local.join("Microsoft").join("Windows").join("Fonts"));
        }
        if let Some(dir) = var("WINDIR") {
            dirs.push(dir.join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = var("HOME") {
            dirs.push(home.join("Library").join("Fonts"));
        }
        dirs.push("/Library/Fonts".into());
        dirs.push("/System/Library/Fonts".into());
    } else {
        let home = var("HOME").unwrap_or_default();
        let data = var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
        dirs.push(data.join("fonts"));
        dirs.push(home.join(".fonts"));
        let defaults = || vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")];
        for dir in var("XDG_DATA_DIRS").map(|s| std::env::split_paths(&s).collect()).unwrap_or_else(defaults) {
            dirs.push(dir.join("fonts"));
        }
        dirs.push("/usr/local/share/fonts".into());
        dirs.push("/usr/share/fonts".into());
    }
    dirs
}

impl Fonts {
    /// `bundled`: the app's font files, each a TrueType or OpenType font or a collection. They are
    /// kept for as long as the app runs, so they must live as long too.
    pub fn new(bundled: &[&'static [u8]]) -> Fonts {
        let bundled: Vec<Font> = bundled.iter().flat_map(|data| load(data)).collect();
        let mut families: HashMap<String, Vec<Font>> = HashMap::new();
        for font in &bundled {
            families.entry(font.rec.family.clone()).or_default().push(*font);
        }
        Fonts { bundled, families, system: OnceLock::new(), loaded: Mutex::new(HashMap::new()) }
    }

    /// The face for a family and style: its own, a stand-in's, or a bundled face of the same kind.
    pub fn resolve(&self, family: &str, bold: bool, italic: bool) -> Option<Font> {
        let family = family.trim().to_lowercase();
        for name in std::iter::once(family.as_str()).chain(substitutes(&family).iter().copied()) {
            if let Some(f) = self.among(self.families.get(name), bold, italic) {
                return Some(f);
            }
            if let Some(f) = self.among(self.system_family(name).as_ref(), bold, italic) {
                return Some(f);
            }
        }
        self.among(self.families.get(&generic(&family)), bold, italic)
    }

    /// A face with a character its own lacks: another bundled one, else the system's first that
    /// has it.
    pub fn fallback(&self, miss: Font, c: char) -> Option<Font> {
        if let Some(f) = self.bundled.iter().copied().filter(|f| f.rec.family != miss.rec.family).find(|f| f.rec.face.glyph_index(c).is_some()) {
            return Some(f);
        }
        let count = self.with_system(|s| s.faces.len()).unwrap_or(0);
        (0..count).filter_map(|i| self.system_face(i, Some(c))).find(|f| f.rec.family != miss.rec.family && f.rec.face.glyph_index(c).is_some())
    }

    /// The face of its own or the same style when it has neither: the style synthesized if need be.
    fn among(&self, faces: Option<&Vec<Font>>, bold: bool, italic: bool) -> Option<Font> {
        let faces = faces?;
        let exact = faces.iter().copied().find(|f| f.rec.bold == bold && f.rec.italic == italic);
        let pick = exact.or_else(|| faces.iter().copied().find(|f| !f.rec.bold && !f.rec.italic)).or_else(|| faces.first().copied())?;
        Some(Font { faux_bold: bold && !pick.rec.bold, faux_italic: italic && !pick.rec.italic, ..pick })
    }

    fn with_system<T>(&self, f: impl FnOnce(&System) -> T) -> Option<T> {
        self.system.get_or_init(|| scan(&font_dirs())).as_ref().map(f)
    }

    fn system_family(&self, family: &str) -> Option<Vec<Font>> {
        let indices = self.with_system(|s| s.by_family.get(family).cloned())??;
        Some(indices.into_iter().filter_map(|i| self.system_face(i, None)).collect())
    }

    /// Opened once. `only_with`: just a face whose character map has that character, read from
    /// the file without its outlines, so one rare character doesn't open every font on the system.
    fn system_face(&self, i: usize, only_with: Option<char>) -> Option<Font> {
        if let Some(f) = self.loaded.lock().ok()?.get(&i) {
            return *f;
        }
        let (path, index) = self.with_system(|s| s.faces.get(i).map(|f| (f.path.clone(), f.index)))??;
        if only_with.is_some_and(|c| !has_glyph(&path, index, c)) {
            return None;
        }
        let font = std::fs::read(&path).ok().and_then(|data| {
            let data: &'static [u8] = Box::leak(data.into_boxed_slice());
            let rec = load_one(data, index)?;
            // A face whose licence forbids embedding is skipped for the next candidate.
            rec.embeddable().then_some(Font { rec, faux_bold: false, faux_italic: false })
        });
        self.loaded.lock().ok()?.insert(i, font);
        font
    }
}

/// Every face in a font file or collection.
fn load(data: &'static [u8]) -> Vec<Font> {
    let n = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    (0..n).filter_map(|index| load_one(data, index)).map(|rec| Font { rec, faux_bold: false, faux_italic: false }).collect()
}

fn load_one(data: &'static [u8], index: u32) -> Option<&'static FaceRec> {
    let face = Face::from_slice(data, index)?;
    let family = family_name(&face)?;
    let style = subfamily(&face).to_lowercase();
    let bold = face.is_bold() || style.contains("bold") || style.contains("demi");
    let italic = face.is_italic() || style.contains("italic") || style.contains("oblique");
    let os2 = face.tables().os2;
    let rec = FaceRec { family: family.trim().to_lowercase(), bold, italic, data, index, no_subset: os2.is_some_and(|t| !t.is_subsetting_allowed()), face };
    Some(Box::leak(Box::new(rec)))
}

impl FaceRec {
    /// Whether its licence lets a document embed it (OS/2 `fsType`).
    fn embeddable(&self) -> bool {
        // Without the table a face of outlines may be embedded; one that says so may not.
        self.face.tables().os2.is_none_or(|t| t.is_outline_embedding_allowed())
    }
}

fn family_name(face: &Face) -> Option<String> {
    face.names().into_iter().filter(|n| n.name_id == 16).find_map(name_to_string).or_else(|| face.names().into_iter().filter(|n| n.name_id == 1).find_map(name_to_string))
}

fn subfamily(face: &Face) -> String {
    face.names().into_iter().filter(|n| n.name_id == 2 || n.name_id == 17).find_map(name_to_string).unwrap_or_default()
}

fn name_to_string(name: ttf_parser::name::Name) -> Option<String> {
    if name.is_unicode() {
        name.to_string()
    } else {
        // Macintosh Roman: near enough to Latin 1 for a family's name.
        Some(name.name.iter().map(|&b| b as char).collect())
    }
}

fn scan(dirs: &[PathBuf]) -> Option<System> {
    let mut system = System::default();
    for dir in dirs {
        let mut stack = vec![dir.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(path);
                } else if is_font(&path) {
                    for (index, family) in peek(&path) {
                        system.by_family.entry(family).or_default().push(system.faces.len());
                        system.faces.push(SystemFace { path: path.clone(), index });
                    }
                }
            }
        }
    }
    (!system.faces.is_empty()).then_some(system)
}

fn is_font(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("ttf" | "otf" | "ttc"))
}

/// Reads a font file in pieces: what a face is named, and whether it has a character, without
/// reading its outlines.
struct Peek {
    file: std::fs::File,
}

impl Peek {
    fn open(path: &Path) -> Option<Peek> {
        Some(Peek { file: std::fs::File::open(path).ok()? })
    }

    fn read(&mut self, at: u64, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.file.seek(SeekFrom::Start(at)).ok()?;
        self.file.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    /// Where each face's table directory starts: one for a font, several for a collection.
    fn faces(&mut self) -> Option<Vec<u64>> {
        let head = self.read(0, 12)?;
        Some(match &head[..4] {
            b"ttcf" => self.read(12, be32(&head[8..12]) as usize * 4)?.as_chunks::<4>().0.iter().map(|c| u64::from(be32(c))).collect(),
            [0, 1, 0, 0] | b"OTTO" | b"true" => vec![0],
            _ => return None,
        })
    }

    fn table(&mut self, base: u64, wanted: &[u8; 4]) -> Option<Vec<u8>> {
        let dir = self.read(base, 12)?;
        let tables = self.read(base + 12, be16(&dir[4..6]) as usize * 16)?;
        let entry = tables.as_chunks::<16>().0.iter().find(|t| &t[..4] == wanted)?;
        self.read(be32(&entry[8..12]) as u64, be32(&entry[12..16]) as usize)
    }
}

/// Each face's family, from the file's `name` table.
fn peek(path: &Path) -> Vec<(u32, String)> {
    let Some(faces) = Peek::open(path).and_then(|mut p| p.faces()) else { return Vec::new() };
    let mut peek = Peek::open(path);
    let mut out = Vec::new();
    for (index, &base) in faces.iter().enumerate() {
        let Some(table) = peek.as_mut().and_then(|p| p.table(base, b"name")) else { continue };
        let (count, strings) = (be16(&table[2..4]) as usize, be16(&table[4..6]) as usize);
        // The best name so far and how good: a typographic family (16) over a legacy one, then
        // Windows' text, Unicode's, the Macintosh's.
        let mut family: Option<(usize, String)> = None;
        let Some(records) = table.get(6..) else { continue };
        for rec in records.as_chunks::<12>().0.iter().take(count) {
            let (platform, id) = (be16(&rec[0..2]), be16(&rec[6..8]));
            if !matches!(platform, 0 | 1 | 3) {
                continue;
            }
            let start = strings + be16(&rec[10..12]) as usize;
            let Some(bytes) = table.get(start..start + be16(&rec[8..10]) as usize) else { continue };
            let text: String = if platform == 1 {
                bytes.iter().map(|&b| b as char).collect()
            } else {
                char::decode_utf16(bytes.as_chunks::<2>().0.iter().map(|c| be16(c))).map(|c| c.unwrap_or('\u{FFFD}')).collect()
            };
            match id {
                1 | 16 => {
                    let rank = usize::from(id == 1) * 3 + match platform { 3 => 0, 0 => 1, _ => 2 };
                    if family.as_ref().is_none_or(|(r, _)| rank < *r) {
                        family = Some((rank, text.trim().to_lowercase()));
                    }
                }
                _ => {}
            }
        }
        if let Some((_, family)) = family {
            out.push((index as u32, family));
        }
    }
    out
}

/// Whether a face's character map has `c`, from the file's `cmap` table.
fn has_glyph(path: &Path, index: u32, c: char) -> bool {
    let Some(base) = Peek::open(path).and_then(|mut p| p.faces()).and_then(|f| f.get(index as usize).copied()) else { return false };
    let Some(cmap) = Peek::open(path).and_then(|mut p| p.table(base, b"cmap")) else { return false };
    let Some(records) = cmap.get(4..4 + be16(&cmap[2..4]) as usize * 8) else { return false };
    let code = c as u32;
    let unicode = |rec: &[u8]| {
        let (platform, encoding) = (be16(&rec[0..2]), be16(&rec[2..4]));
        platform == 0 || (platform == 3 && matches!(encoding, 1 | 10))
    };
    records
        .as_chunks::<8>()
        .0
        .iter()
        .filter(|rec| unicode(&rec[..]))
        .filter_map(|rec| cmap.get(be32(&rec[4..8]) as usize..))
        .any(|sub| match sub.get(..2).map(be16) {
            // Segments of the Basic Multilingual Plane.
            Some(4) if code <= 0xFFFF => {
                let segments = sub.get(6..8).map(be16).unwrap_or(0) as usize / 2;
                let (Some(ends), Some(starts)) = (sub.get(14..14 + segments * 2), sub.get(16 + segments * 2..16 + segments * 4)) else { return false };
                (0..segments).any(|i| be16(&starts[i * 2..]) <= code as u16 && code as u16 <= be16(&ends[i * 2..]))
            }
            // Groups, over everything.
            Some(12) => {
                let groups = sub.get(12..16).map(be32).unwrap_or(0) as usize;
                sub.get(16..).is_some_and(|g| g.as_chunks::<12>().0.iter().take(groups).any(|g| be32(&g[0..4]) <= code && code <= be32(&g[4..8])))
            }
            _ => false,
        })
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// The faces that stand in for a family this system may lack, nearest first.
fn substitutes(family: &str) -> &'static [&'static str] {
    match family {
        "times new roman" | "times" | "serif" => &["liberation serif", "tinos", "nimbus roman", "dejavu serif", "noto serif"],
        "georgia" => &["gelasio"],
        "palatino" | "palatino linotype" | "book antiqua" => &["palatino linotype", "book antiqua", "p052", "urw palladio l", "tex gyre pagella"],
        "charter" | "bitstream charter" | "charis sil" => &["bitstream charter", "charis sil"],
        "courier new" | "courier" | "monospace" => &["liberation mono", "cousine", "nimbus mono ps", "dejavu sans mono"],
        "arial" | "helvetica" | "helvetica neue" | "sans-serif" => &["liberation sans", "arimo", "nimbus sans", "noto sans", "dejavu sans"],
        "menlo" | "consolas" | "monaco" => &["dejavu sans mono", "liberation mono", "noto sans mono", "jetbrains mono"],
        _ => &[],
    }
}

/// A bundled family of the same kind, for when nothing else has it.
fn generic(family: &str) -> String {
    if family.contains("mono") || family.contains("code") || family.contains("courier") || ["menlo", "consolas", "monaco"].contains(&family) {
        "ibm plex mono".into()
    } else if family.contains("sans") || family.contains("arial") || family.contains("helvetica") || family.contains("grotesk") {
        "ia writer quattro s".into()
    } else {
        "literata".into()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{Fonts, generic, substitutes};

    pub(crate) const BUNDLED: [&[u8]; 19] = [
        include_bytes!("../../app/assets/fonts/JetBrainsMono-Regular.ttf"),
        include_bytes!("../../app/assets/fonts/JetBrainsMono-Bold.ttf"),
        include_bytes!("../../app/assets/fonts/JetBrainsMono-Italic.ttf"),
        include_bytes!("../../app/assets/fonts/JetBrainsMono-BoldItalic.ttf"),
        include_bytes!("../../app/assets/fonts/Caveat.ttf"),
        include_bytes!("../../app/assets/fonts/ShantellSans-Regular.ttf"),
        include_bytes!("../../app/assets/fonts/BadScript-Regular.ttf"),
        include_bytes!("../../app/assets/fonts/IBMPlexMono-Regular.ttf"),
        include_bytes!("../../app/assets/fonts/IBMPlexMono-Bold.ttf"),
        include_bytes!("../../app/assets/fonts/IBMPlexMono-Italic.ttf"),
        include_bytes!("../../app/assets/fonts/IBMPlexMono-BoldItalic.ttf"),
        include_bytes!("../../app/assets/fonts/iAWriterQuattroS-Regular.ttf"),
        include_bytes!("../../app/assets/fonts/iAWriterQuattroS-Bold.ttf"),
        include_bytes!("../../app/assets/fonts/iAWriterQuattroS-Italic.ttf"),
        include_bytes!("../../app/assets/fonts/iAWriterQuattroS-BoldItalic.ttf"),
        include_bytes!("../../app/assets/fonts/Literata-Regular.ttf"),
        include_bytes!("../../app/assets/fonts/Literata-Bold.ttf"),
        include_bytes!("../../app/assets/fonts/Literata-Italic.ttf"),
        include_bytes!("../../app/assets/fonts/Literata-BoldItalic.ttf"),
    ];

    /// The app's fonts and nothing of the system's: the same pages on every system.
    pub(crate) fn bundled() -> Fonts {
        Fonts::new(&BUNDLED)
    }

    #[test]
    fn bundled_families_resolve_their_four_styles() {
        let fonts = bundled();
        for family in ["IBM Plex Mono", "jetbrains mono", "iA Writer Quattro S", "Literata"] {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let f = fonts.resolve(family, bold, italic).unwrap();
                assert_eq!((f.rec.bold, f.rec.italic, f.faux_bold, f.faux_italic), (bold, italic, false, false), "{family} {bold} {italic}");
                assert_eq!(f.rec.family, family.to_lowercase());
            }
        }
    }

    #[test]
    fn a_family_with_one_face_gets_the_styles_synthesized() {
        let fonts = bundled();
        let bold = fonts.resolve("Caveat", true, false).unwrap();
        assert!(!bold.rec.bold && bold.faux_bold, "Caveat ships regular only");
        let italic = fonts.resolve("Bad Script", false, true).unwrap();
        assert!(italic.faux_italic && !italic.faux_bold);
    }

    #[test]
    fn an_unknown_family_falls_to_a_bundled_face_of_its_kind() {
        let fonts = bundled();
        assert_eq!(fonts.resolve("Some Made-up Serif", false, false).unwrap().rec.family, "literata");
        assert_eq!(fonts.resolve("Fancy Mono", false, false).unwrap().rec.family, "ibm plex mono");
        assert_eq!(fonts.resolve("My Sans", false, false).unwrap().rec.family, "ia writer quattro s");
        assert_eq!(generic("x mono"), "ibm plex mono");
        assert!(substitutes("times new roman").contains(&"liberation serif"));
    }

    #[test]
    fn a_names_table_is_read_from_the_file_alone() {
        // The bundled files stand in for the system's: the same names from the whole file and from its pieces.
        let fonts = bundled();
        for font in &fonts.bundled {
            let path = std::env::temp_dir().join(format!("dostoevsky-pdf-probe-{}.ttf", std::process::id()));
            std::fs::write(&path, font.rec.data).unwrap();
            let peeked = super::peek(&path);
            assert!(peeked.iter().any(|(_, family)| *family == font.rec.family), "{}: {:?}", font.rec.family, peeked);
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn a_character_map_is_read_from_the_file_alone() {
        let fonts = bundled();
        let font = fonts.resolve("Literata", false, false).unwrap();
        let path = std::env::temp_dir().join(format!("dostoevsky-pdf-probe-cmap-{}.ttf", std::process::id()));
        std::fs::write(&path, font.rec.data).unwrap();
        for c in ['a', 'я', 'Ё', 'ö', '–', '\u{1F600}', '\u{2E80}'] {
            assert_eq!(super::has_glyph(&path, 0, c), font.rec.face.glyph_index(c).is_some(), "{c:?} in Literata");
        }
        let _ = std::fs::remove_file(&path);
    }
}
