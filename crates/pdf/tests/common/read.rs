//! A small reader for the PDFs this crate writes, so the tests read the files back as a viewer
//! would: the cross-reference table, the objects, the streams decompressed, the pages in order,
//! and a page's text through its fonts' maps from glyphs to characters.

#![allow(dead_code)]

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Ref(u32),
    Name(Vec<u8>),
    Number(f64),
    Bytes(Vec<u8>),
    Array(Vec<Object>),
    Dict(Vec<(Vec<u8>, Object)>),
    /// A stream's dictionary and its bytes as the file holds them.
    Stream(Vec<(Vec<u8>, Object)>, Vec<u8>),
}

impl Object {
    fn entries(&self) -> Option<&[(Vec<u8>, Object)]> {
        match self {
            Object::Dict(d) | Object::Stream(d, _) => Some(d),
            _ => None,
        }
    }

    fn member<'a>(&'a self, key: &[u8]) -> Option<&'a Object> {
        self.entries()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn number(&self) -> Option<f64> {
        match self {
            Object::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn name(&self) -> Option<&[u8]> {
        match self {
            Object::Name(n) => Some(n),
            _ => None,
        }
    }
}

pub struct Pdf {
    objects: BTreeMap<u32, Object>,
}

/// The bytes of a PDF, as values and bare words.
struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
}

const DELIMITERS: &[u8] = b"[]<>()/%";

impl<'a> Lexer<'a> {
    fn skip(&mut self) {
        loop {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
            if self.bytes.get(self.at) == Some(&b'%') {
                while self.bytes.get(self.at).is_some_and(|b| !matches!(b, b'\n' | b'\r')) {
                    self.at += 1;
                }
            } else {
                return;
            }
        }
    }

    fn starts(&mut self, word: &[u8]) -> bool {
        self.skip();
        self.bytes[self.at..].starts_with(word)
    }

    /// A bare word: an operator, or a keyword such as `obj` or `trailer`.
    fn word(&mut self) -> Option<String> {
        self.skip();
        let from = self.at;
        while self.bytes.get(self.at).is_some_and(|b| !b.is_ascii_whitespace() && !DELIMITERS.contains(b)) {
            self.at += 1;
        }
        (self.at > from).then(|| String::from_utf8_lossy(&self.bytes[from..self.at]).into_owned())
    }

    fn take(&mut self, word: &[u8]) -> bool {
        if self.starts(word) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    /// The next value where one starts, else `None` (and nothing read).
    fn value(&mut self) -> Option<Object> {
        self.skip();
        match *self.bytes.get(self.at)? {
            b'/' => {
                self.at += 1;
                let from = self.at;
                while self.bytes.get(self.at).is_some_and(|b| !b.is_ascii_whitespace() && !DELIMITERS.contains(b)) {
                    self.at += 1;
                }
                Some(Object::Name(unescape_name(&self.bytes[from..self.at])))
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                while !self.take(b"]") {
                    items.push(self.value()?);
                }
                Some(Object::Array(items))
            }
            b'<' if self.bytes[self.at..].starts_with(b"<<") => {
                self.at += 2;
                let mut entries = Vec::new();
                while !self.take(b">>") {
                    let Object::Name(key) = self.value()? else { return None };
                    assert!(entries.iter().all(|(k, _)| *k != key), "a key twice in one dictionary: {}", String::from_utf8_lossy(&key));
                    entries.push((key, self.value()?));
                }
                Some(Object::Dict(entries))
            }
            // A string in hexadecimal.
            b'<' => {
                self.at += 1;
                let end = self.at + self.bytes[self.at..].iter().position(|b| *b == b'>')?;
                let hex: Vec<u8> = self.bytes[self.at..end].iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
                self.at = end + 1;
                let mut bytes = Vec::new();
                for pair in hex.chunks(2) {
                    let digits = std::str::from_utf8(pair).ok()?;
                    bytes.push(u8::from_str_radix(digits, 16).ok()?);
                }
                Some(Object::Bytes(bytes))
            }
            // A string between parentheses.
            b'(' => {
                self.at += 1;
                let mut bytes = Vec::new();
                let mut depth = 0usize;
                while let Some(&b) = self.bytes.get(self.at) {
                    self.at += 1;
                    match b {
                        b'(' => {
                            depth += 1;
                            bytes.push(b'(');
                        }
                        b')' if depth == 0 => break,
                        b')' => {
                            depth -= 1;
                            bytes.push(b')');
                        }
                        b'\\' => {
                            match self.bytes.get(self.at) {
                                Some(b'n') => bytes.push(b'\n'),
                                Some(b'r') => bytes.push(b'\r'),
                                Some(b't') => bytes.push(b'\t'),
                                Some(b'b') => bytes.push(8),
                                Some(b'f') => bytes.push(12),
                                Some(&other @ (b'(' | b')' | b'\\')) => bytes.push(other),
                                Some(d) if d.is_ascii_digit() => {
                                    let (mut n, mut len) = (0u16, 0);
                                    while let Some(d) = self.bytes.get(self.at + len).filter(|d| d.is_ascii_digit() && len < 3) {
                                        n = n * 8 + u16::from(d - b'0');
                                        len += 1;
                                    }
                                    bytes.push(n as u8);
                                    self.at += len - 1;
                                }
                                _ => bytes.push(b'\\'),
                            }
                            self.at += 1;
                        }
                        b'\r' if self.bytes.get(self.at) == Some(&b'\n') => self.at += 1,
                        b => bytes.push(b),
                    }
                }
                Some(Object::Bytes(bytes))
            }
            b'-' | b'+' | b'.' | b'0'..=b'9' => {
                let from = self.at;
                while self.bytes.get(self.at).is_some_and(|b| matches!(b, b'-' | b'+' | b'.' | b'0'..=b'9')) {
                    self.at += 1;
                }
                let n: f64 = String::from_utf8_lossy(&self.bytes[from..self.at]).parse().ok()?;
                // `12 0 R` is a reference to an object.
                let after = self.at;
                if let Some(Object::Number(generation)) = self.value() {
                    if generation == 0. && self.word().as_deref() == Some("R") {
                        return Some(Object::Ref(n as u32));
                    }
                }
                self.at = after;
                Some(Object::Number(n))
            }
            _ => None,
        }
    }
}

fn unescape_name(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' && i + 2 < bytes.len() {
            if let Ok(n) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// A string as text: UTF-16 with its mark, else one byte to a character.
fn text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(&[0xFF, 0xFE]) {
        String::from_utf16_lossy(&bytes[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>())
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

impl Pdf {
    /// Reads a file from its cross-reference table: every object where the table says it is.
    pub fn parse(bytes: &[u8]) -> Pdf {
        assert!(bytes.starts_with(b"%PDF-"), "a PDF starts with its header");
        let tail = &bytes[bytes.len().saturating_sub(1024)..];
        let at = tail.windows(9).rposition(|w| w == b"startxref").expect("startxref at the end");
        let mut lexer = Lexer { bytes: tail, at: at + 9 };
        let table: usize = lexer.word().and_then(|w| w.parse().ok()).expect("the table's offset");
        let mut lexer = Lexer { bytes, at: table };
        assert_eq!(lexer.word().as_deref(), Some("xref"), "the table is where the trailer says");
        let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
        loop {
            let head = lexer.word().expect("subsections, then the trailer");
            if head == "trailer" {
                break;
            }
            let first: u32 = head.parse().expect("a subsection's first object");
            let count: u32 = lexer.word().and_then(|c| c.parse().ok()).expect("a subsection's count");
            lexer.skip();
            for i in 0..count {
                let entry = &bytes[lexer.at..lexer.at + 20];
                let offset: usize = std::str::from_utf8(&entry[..10]).unwrap().trim().parse().expect("an offset");
                if entry[17] == b'n' {
                    offsets.insert(first + i, offset);
                }
                lexer.at += 20;
            }
            lexer.skip();
        }
        let Object::Dict(trailer) = lexer.value().expect("a trailer") else { panic!("the trailer is a dictionary") };
        let size = trailer.iter().find(|(k, _)| k == b"Size").and_then(|(_, v)| v.number()).expect("the trailer's size") as u32;
        let objects: BTreeMap<u32, Object> = offsets
            .into_iter()
            .filter_map(|(id, offset)| {
                let mut lexer = Lexer { bytes, at: offset };
                assert_eq!((lexer.word(), lexer.word(), lexer.word()), (Some(id.to_string()), Some("0".into()), Some("obj".into())), "object {id} is where the table says");
                let mut object = lexer.value()?;
                if lexer.word().as_deref() == Some("stream") {
                    if bytes.get(lexer.at) == Some(&b'\r') {
                        lexer.at += 1;
                    }
                    lexer.at += 1;
                    let len = object.member(b"Length").and_then(Object::number).unwrap_or(0.) as usize;
                    let Object::Dict(dictionary) = object else { panic!("a stream's dictionary") };
                    object = Object::Stream(dictionary, bytes[lexer.at..lexer.at + len].to_vec());
                }
                Some((id, object))
            })
            .collect();
        assert_eq!(objects.keys().next_back().copied().unwrap_or(0) + 1, size, "every object of the table is there, besides the free one");
        Pdf { objects }
    }

    pub fn get(&self, id: u32) -> &Object {
        self.objects.get(&id).unwrap_or_else(|| panic!("object {id}"))
    }

    pub fn object<'a>(&'a self, value: &'a Object) -> &'a Object {
        match value {
            Object::Ref(id) => self.get(*id),
            other => other,
        }
    }

    /// A dictionary member, followed to its object.
    pub fn member<'a>(&'a self, dict: &'a Object, key: &[u8]) -> Option<&'a Object> {
        dict.member(key).map(|v| self.object(v))
    }

    /// A stream's bytes, inflated where the file says they are.
    pub fn data(&self, stream: &Object) -> Vec<u8> {
        let Object::Stream(entries, bytes) = stream else { panic!("a stream: {stream:?}") };
        match entries.iter().find(|(k, _)| k == b"Filter").map(|(_, v)| v) {
            None => bytes.clone(),
            Some(Object::Name(filter)) if filter == b"FlateDecode" => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(bytes, 1 << 28).expect("the stream inflates"),
            Some(other) => panic!("a filter the reader doesn't know: {other:?}"),
        }
    }

    /// The pages in their order.
    pub fn pages(&self) -> Vec<&Object> {
        let catalog = self.objects.values().find(|o| o.member(b"Type").and_then(Object::name) == Some(b"Catalog")).expect("a catalog");
        let tree = self.member(catalog, b"Pages").expect("a page tree");
        let count = self.member(tree, b"Count").and_then(Object::number).expect("a count");
        fn kids<'a>(pdf: &'a Pdf, node: &'a Object, out: &mut Vec<&'a Object>) {
            match pdf.member(node, b"Type").and_then(Object::name) {
                Some(b"Pages") => {
                    if let Object::Array(children) = pdf.member(node, b"Kids").expect("a tree's kids") {
                        for child in children {
                            kids(pdf, pdf.object(child), out);
                        }
                    }
                }
                _ => out.push(node),
            }
        }
        let mut pages = Vec::new();
        kids(self, tree, &mut pages);
        assert_eq!(pages.len() as f64, count, "as many pages as the tree counts");
        pages
    }

    /// The document's information dictionary, as (key, text).
    pub fn info(&self) -> BTreeMap<String, String> {
        self.objects.values().filter_map(|o| o.entries()).flatten().filter_map(|(k, v)| match v {
            Object::Bytes(b) => Some((String::from_utf8_lossy(k).into_owned(), text(b))),
            _ => None,
        }).filter(|(k, _)| ["Title", "Author", "Creator"].contains(&k.as_str())).collect()
    }

    /// The faces a page writes in, by the name its content gives them.
    pub fn fonts<'a>(&'a self, page: &'a Object) -> BTreeMap<Vec<u8>, &'a Object> {
        let Some(Object::Dict(fonts)) = self.member(page, b"Resources").and_then(|r| self.member(r, b"Font")) else { return BTreeMap::new() };
        fonts.iter().map(|(name, font)| (name.clone(), self.object(font))).collect()
    }

    /// A font's map from glyph codes to characters: its `ToUnicode` table.
    pub fn unicode(&self, font: &Object) -> BTreeMap<u16, String> {
        let mut map = BTreeMap::new();
        let Some(table) = self.member(font, b"ToUnicode") else { return map };
        let content = String::from_utf8_lossy(&self.data(self.object(table))).into_owned();
        let (mut chars, mut range) = (false, false);
        for line in content.lines().map(str::trim) {
            match line {
                _ if line.ends_with("beginbfchar") => chars = true,
                "endbfchar" => chars = false,
                _ if line.ends_with("beginbfrange") => range = true,
                "endbfrange" => range = false,
                _ if chars => {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    if let [code, value, ..] = fields[..] {
                        map.insert(code_of(code), utf16(value));
                    }
                }
                _ if range => {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    if let [from, to, first, ..] = fields[..] {
                        for (i, code) in (code_of(from)..=code_of(to)).enumerate() {
                            map.insert(code, utf16(first).chars().map(|c| c as u32 + i as u32).filter_map(char::from_u32).collect());
                        }
                    }
                }
                _ => {}
            }
        }
        map
    }

    /// A page's text, line by line, read through its fonts' maps: what selecting it gives.
    pub fn text(&self, page: &Object) -> Vec<String> {
        let maps: BTreeMap<Vec<u8>, BTreeMap<u16, String>> = self.fonts(page).iter().map(|(name, font)| (name.clone(), self.unicode(font))).collect();
        let content = self.member(page, b"Contents").map(|c| self.data(self.object(c))).unwrap_or_default();
        let mut lexer = Lexer { bytes: &content, at: 0 };
        let mut operands: Vec<Object> = Vec::new();
        let mut font: BTreeMap<u16, String> = BTreeMap::new();
        // Each line as its baseline and its text.
        let mut lines: Vec<(f64, String)> = Vec::new();
        while lexer.at < content.len() {
            lexer.skip();
            if lexer.at >= content.len() {
                break;
            }
            let before = lexer.at;
            if let Some(value) = lexer.value() {
                operands.push(value);
                continue;
            }
            lexer.at = before;
            let Some(operator) = lexer.word() else { break };
            match operator.as_str() {
                "Tf" => font = operands.first().and_then(Object::name).and_then(|n| maps.get(n)).cloned().unwrap_or_default(),
                // A run starts at its text matrix: a new line unless it shares the last one's baseline.
                "Tm" => {
                    let baseline = operands.get(5).and_then(Object::number).unwrap_or(0.);
                    // A run joins the line it shares a baseline with, wherever in the stream that began.
                    match lines.iter().position(|(y, _)| (y - baseline).abs() < 0.5) {
                        Some(i) => {
                            let line = lines.remove(i);
                            lines.push(line);
                        }
                        None => lines.push((baseline, String::new())),
                    }
                }
                "TJ" => {
                    if let Some(Object::Array(items)) = operands.last() {
                        for item in items {
                            if let Object::Bytes(bytes) = item {
                                lines.last_mut().expect("a text matrix came first").1.push_str(&shown(&font, bytes));
                            }
                        }
                    }
                }
                "Tj" | "'" | "\"" => {
                    if let Some(Object::Bytes(bytes)) = operands.last() {
                        lines.last_mut().expect("a text matrix came first").1.push_str(&shown(&font, bytes));
                    }
                }
                _ => {}
            }
            operands.clear();
        }
        // Top to bottom, as a reader selecting the page gets them.
        lines.sort_by(|a, b| b.0.total_cmp(&a.0));
        lines.into_iter().map(|(_, text)| text).collect()
    }
}

/// A string's characters: two bytes a code, each through the font's map.
fn shown(font: &BTreeMap<u16, String>, bytes: &[u8]) -> String {
    bytes.as_chunks::<2>().0.iter().map(|c| font.get(&u16::from_be_bytes([c[0], c[1]])).cloned().unwrap_or_default()).collect()
}

fn code_of(hex: &str) -> u16 {
    u16::from_str_radix(hex.trim_matches(|c| c == '<' || c == '>'), 16).unwrap()
}

fn utf16(hex: &str) -> String {
    let hex = hex.trim_matches(|c| c == '<' || c == '>');
    String::from_utf16_lossy(&hex.as_bytes().chunks(4).filter_map(|c| std::str::from_utf8(c).ok().and_then(|c| u16::from_str_radix(c, 16).ok())).collect::<Vec<_>>())
}
