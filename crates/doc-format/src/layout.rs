#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawBlock {
    /// `<!-- dostoevsky:overflow`: the unescaped text between the header and `-->`.
    Overflow(String),
    /// `<!-- dostoevsky:meta v<N>`: the lines between the header and `-->`.
    Meta { version: u64, lines: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split<'a> {
    pub before: &'a str,
    pub trailer: &'a str,
    pub blocks: Vec<RawBlock>,
}

pub const OVERFLOW_HEADER: &str = "<!-- dostoevsky:overflow";
pub const META_HEADER: &str = "<!-- dostoevsky:meta v";

/// Kept so a save writes them back: plain files stay byte-identical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Encoding {
    pub bom: bool,
    pub crlf: bool,
}

/// Normalises `\r\n` to `\n` only when *every* break is one; mixed endings stay (exact round trip).
pub fn decode(file: &str) -> (String, Encoding) {
    let (text, bom) = match file.strip_prefix('\u{feff}') {
        Some(t) => (t, true),
        None => (file, false),
    };
    let newlines = text.matches('\n').count();
    let crlf = newlines > 0 && text.matches("\r\n").count() == newlines;
    let text = if crlf {
        text.replace("\r\n", "\n")
    } else {
        text.to_string()
    };
    (text, Encoding { bom, crlf })
}

pub fn encode(text: &str, enc: Encoding) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    if enc.bom {
        out.push('\u{feff}');
    }
    if enc.crlf {
        out.push_str(&text.replace('\n', "\r\n"));
    } else {
        out.push_str(text);
    }
    out
}

fn header(line: &str) -> Option<Option<u64>> {
    let line = line.trim_end();
    if line == OVERFLOW_HEADER {
        return Some(None);
    }
    let v = line.strip_prefix(META_HEADER)?;
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse().ok().map(Some)
}

/// The longest run of recognised blocks at the very end, separated only by blank lines.
pub fn split(text: &str) -> Option<Split<'_>> {
    if !text.contains("<!-- dostoevsky:") {
        return None;
    }
    // (byte start, line without '\n')
    let mut lines = Vec::new();
    let mut at = 0;
    for l in text.split_inclusive('\n') {
        lines.push((at, l.strip_suffix('\n').unwrap_or(l)));
        at += l.len();
    }
    let candidates = lines
        .iter()
        .enumerate()
        .filter(|(_, (_, l))| header(l).is_some())
        .map(|(i, _)| i);
    for first in candidates {
        if let Some(blocks) = parse_blocks(&lines[first..]) {
            let start = lines[first].0;
            return Some(Split {
                before: &text[..start],
                trailer: &text[start..],
                blocks,
            });
        }
    }
    None
}

fn parse_blocks(lines: &[(usize, &str)]) -> Option<Vec<RawBlock>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].1;
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        let version = header(line)?;
        // Contents can't contain `-->` (it is escaped), so the first line with one closes.
        let close = i + 1 + lines[i + 1..].iter().position(|(_, l)| l.contains("-->"))?;
        if lines[close].1.trim() != "-->" {
            return None;
        }
        let inner: Vec<&str> = lines[i + 1..close].iter().map(|(_, l)| *l).collect();
        out.push(match version {
            None => RawBlock::Overflow(unescape_comment(&inner.join("\n"))),
            Some(version) => RawBlock::Meta {
                version,
                lines: inner
                    .iter()
                    .map(|l| l.trim_end_matches('\r').to_string())
                    .collect(),
            },
        });
        i = close + 1;
    }
    Some(out)
}

/// `--` + k backslashes + `>` → `--` + (k+1) backslashes + `>`.
pub fn escape_comment(s: &str) -> String {
    rewrite_dashes(s, |k| k + 1)
}

/// Inverse of [`escape_comment`]: k ≥ 1 backslashes lose one.
pub fn unescape_comment(s: &str) -> String {
    rewrite_dashes(s, |k| k.saturating_sub(1))
}

fn rewrite_dashes(s: &str, f: impl Fn(usize) -> usize) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut copied = 0;
    while i + 1 < b.len() {
        if b[i] == b'-' && b[i + 1] == b'-' {
            let mut j = i + 2;
            while j < b.len() && b[j] == b'\\' {
                j += 1;
            }
            if j < b.len() && b[j] == b'>' {
                let k = j - (i + 2);
                if f(k) != k {
                    out.push_str(&s[copied..i + 2]);
                    out.extend(std::iter::repeat_n('\\', f(k)));
                    out.push('>');
                    copied = j + 1;
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&s[copied..]);
    out
}

/// Meta lines: `-->` can only occur inside JSON strings, where `>` reads back as `>`.
pub fn escape_json_line(s: &str) -> String {
    s.replace("-->", "--\\u003e")
}

/// Notes are split by one blank line; inside a note an empty line is a lone `\` and a line
/// of only backslashes gets one more, so notes with blank lines survive.
pub fn write_notes(notes: &[String]) -> String {
    let encoded: Vec<String> = notes
        .iter()
        .map(|n| {
            n.split('\n')
                .map(|l| {
                    if l.bytes().all(|b| b == b'\\') {
                        format!("{l}\\")
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    escape_comment(&encoded.join("\n\n"))
}

/// Inverse of [`write_notes`], after [`unescape_comment`].
pub fn read_notes(block: &str) -> Vec<String> {
    let mut notes = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for l in block.split('\n') {
        if l.is_empty() {
            if !cur.is_empty() {
                notes.push(cur.join("\n"));
                cur.clear();
            }
            continue;
        }
        if l.bytes().all(|b| b == b'\\') {
            cur.push(l[1..].to_string());
        } else {
            cur.push(l.to_string());
        }
    }
    if !cur.is_empty() {
        notes.push(cur.join("\n"));
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_escaping_is_lossless() {
        assert_eq!(escape_comment("a --> b"), "a --\\> b");
        assert_eq!(escape_comment("--\\>"), "--\\\\>");
        assert_eq!(escape_comment("--->"), "---\\>");
        assert_eq!(escape_comment("-- >"), "-- >");
        for s in [
            "a --> b", "--\\\\>", "--->-->", "->", "--", "x--\\", "é-->é",
        ] {
            assert_eq!(unescape_comment(&escape_comment(s)), s);
            assert!(!escape_comment(s).contains("-->"));
        }
    }

    #[test]
    fn notes_with_blank_lines_survive() {
        let notes: Vec<String> = ["one", "two\n\nthree", "", "\\", "a\n", "\n\\\\\nb"]
            .map(String::from)
            .into();
        let block = write_notes(&notes);
        assert_eq!(read_notes(&unescape_comment(&block)), notes);
        assert_eq!(read_notes("a\n\n\n\nb\n"), ["a", "b"]);
    }

    #[test]
    fn trailer_is_the_last_run_of_recognised_blocks() {
        let f = "Body <!-- x -->\n\n<!-- dostoevsky:overflow\nnote --\\> here\n-->\n\n<!-- dostoevsky:meta v1\n{\"kind\":\"doc\"}\n-->\n";
        let s = split(f).unwrap();
        assert_eq!(s.before, "Body <!-- x -->\n\n");
        assert_eq!(
            s.blocks,
            [
                RawBlock::Overflow("note --> here".into()),
                RawBlock::Meta {
                    version: 1,
                    lines: vec!["{\"kind\":\"doc\"}".into()]
                }
            ]
        );
        assert!(split("<!-- dostoevsky:meta v1\n-->\ntext\n").is_none());
        assert!(split("```\n<!-- dostoevsky:meta v1\n-->\n```\n").is_none());
        assert!(split("<!-- dostoevsky:meta vX\n-->\n").is_none());
        assert_eq!(
            split("x\n<!-- dostoevsky:meta v2\n{}\n-->").unwrap().blocks,
            [RawBlock::Meta {
                version: 2,
                lines: vec!["{}".into()]
            }]
        );
    }

    #[test]
    fn line_endings() {
        let (t, e) = decode("\u{feff}a\r\nb\r\n");
        assert_eq!(
            (t.as_str(), e),
            (
                "a\nb\n",
                Encoding {
                    bom: true,
                    crlf: true
                }
            )
        );
        assert_eq!(encode(&t, e), "\u{feff}a\r\nb\r\n");
        let (t, e) = decode("a\r\nb\n");
        assert_eq!((t.as_str(), e), ("a\r\nb\n", Encoding::default()));
    }
}
