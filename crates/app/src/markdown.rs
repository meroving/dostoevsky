use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Blank,
    Paragraph,
    Heading(u8),
    Quote,
    List,
    Rule,
    Fence,
    Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Span {
    Marker,
    Strong,
    Emphasis,
    StrongEmphasis,
    Code,
    Link,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LineStyle {
    pub kind: BlockKind,
    /// Byte ranges within the line.
    pub spans: Vec<(Range<usize>, Span)>,
    /// Bytes at the start hidden when the caret is elsewhere (heading markers).
    pub hidden_prefix: usize,
}

#[cfg(test)]
pub fn style_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<LineStyle> {
    style_lines_cached(lines, &mut HashMap::new())
}

/// Cache keyed by line text and fence state; replaced by the lines seen this time.
pub fn style_lines_cached<'a>(
    lines: impl Iterator<Item = &'a str>,
    cache: &mut HashMap<u64, LineStyle>,
) -> Vec<LineStyle> {
    let mut in_fence = false;
    let mut next = HashMap::with_capacity(cache.len());
    let out = lines
        .map(|l| {
            let mut h = DefaultHasher::new();
            (l, in_fence).hash(&mut h);
            let key = h.finish();
            let style = match cache.get(&key) {
                Some(s) => s.clone(),
                None => style_one(l, in_fence),
            };
            if style.kind == BlockKind::Fence {
                in_fence = !in_fence;
            }
            next.insert(key, style.clone());
            style
        })
        .collect();
    *cache = next;
    out
}

fn style_one(l: &str, in_fence: bool) -> LineStyle {
    {
        {
            let trimmed = l.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                return LineStyle {
                    kind: BlockKind::Fence,
                    spans: vec![(0..l.len(), Span::Marker)],
                    hidden_prefix: 0,
                };
            }
            if in_fence {
                return LineStyle {
                    kind: BlockKind::Code,
                    spans: vec![(0..l.len(), Span::Code)],
                    hidden_prefix: 0,
                };
            }
            style_line(l)
        }
    }
}

pub fn style_line(line: &str) -> LineStyle {
    let mut spans = Vec::new();
    if line.trim().is_empty() {
        return LineStyle {
            kind: BlockKind::Blank,
            spans,
            hidden_prefix: 0,
        };
    }
    let t = line.trim();
    if t.len() >= 3
        && (t.chars().all(|c| c == '-' || c == ' ')
            || t.chars().all(|c| c == '*' || c == ' ')
            || t.chars().all(|c| c == '_' || c == ' '))
        && t.chars().filter(|c| !c.is_whitespace()).count() >= 3
    {
        return LineStyle {
            kind: BlockKind::Rule,
            spans: vec![(0..line.len(), Span::Marker)],
            hidden_prefix: 0,
        };
    }

    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        let prefix = hashes + 1;
        spans.push((0..prefix, Span::Marker));
        inline(line, prefix, &mut spans);
        return LineStyle {
            kind: BlockKind::Heading(hashes as u8),
            spans,
            hidden_prefix: prefix,
        };
    }

    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let (kind, marker) = if rest.starts_with("> ") || rest == ">" {
        (BlockKind::Quote, 1 + usize::from(rest.len() > 1))
    } else if rest.len() >= 2
        && matches!(rest.as_bytes()[0], b'-' | b'*' | b'+')
        && rest.as_bytes()[1] == b' '
    {
        (BlockKind::List, 2)
    } else if let Some(n) = ordered_marker(rest) {
        (BlockKind::List, n)
    } else {
        (BlockKind::Paragraph, 0)
    };
    if marker > 0 {
        spans.push((indent..indent + marker, Span::Marker));
    }
    inline(line, indent + marker, &mut spans);
    LineStyle {
        kind,
        spans,
        hidden_prefix: 0,
    }
}

fn ordered_marker(s: &str) -> Option<usize> {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let b = s.as_bytes();
    (b.get(digits).is_some_and(|c| *c == b'.' || *c == b')') && b.get(digits + 1) == Some(&b' '))
        .then_some(digits + 2)
}

/// Code first: nothing nests inside it.
fn inline(line: &str, from: usize, spans: &mut Vec<(Range<usize>, Span)>) {
    let b = line.as_bytes();
    let mut taken: Vec<Range<usize>> = Vec::new();

    let mut i = from;
    while i < b.len() {
        if b[i] == b'`' {
            let ticks = b[i..].iter().take_while(|c| **c == b'`').count();
            let close = find_seq(b, i + ticks, &vec![b'`'; ticks]);
            if let Some(j) = close {
                spans.push((i..i + ticks, Span::Marker));
                spans.push((i + ticks..j, Span::Code));
                spans.push((j..j + ticks, Span::Marker));
                taken.push(i..j + ticks);
                i = j + ticks;
                continue;
            }
            i += ticks;
            continue;
        }
        i += 1;
    }

    let free = |r: &Range<usize>, taken: &[Range<usize>]| {
        taken.iter().all(|t| r.end <= t.start || r.start >= t.end)
    };

    let mut i = from;
    while i < b.len() {
        if b[i] == b'[' && free(&(i..i + 1), &taken) {
            if let Some(close) = b[i + 1..].iter().position(|c| *c == b']').map(|p| i + 1 + p) {
                if b.get(close + 1) == Some(&b'(') {
                    if let Some(end) = b[close + 2..].iter().position(|c| *c == b')') {
                        let end = close + 2 + end;
                        if free(&(i..end + 1), &taken) {
                            spans.push((i..i + 1, Span::Marker));
                            spans.push((i + 1..close, Span::Link));
                            spans.push((close..end + 1, Span::Marker));
                            taken.push(i..end + 1);
                            i = end + 1;
                            continue;
                        }
                    }
                }
            }
        }
        i += 1;
    }

    for (delim, kind) in [
        ("***", Span::StrongEmphasis),
        ("**", Span::Strong),
        ("__", Span::Strong),
        ("*", Span::Emphasis),
        ("_", Span::Emphasis),
    ] {
        let d = delim.as_bytes();
        let mut i = from;
        while i + d.len() <= b.len() {
            let opens = &b[i..i + d.len()] == d
                && b.get(i + d.len()).is_some_and(|c| !c.is_ascii_whitespace())
                && (i == 0 || !b[i - 1].is_ascii_alphanumeric() || d[0] == b'*')
                && free(&(i..i + d.len()), &taken);
            if opens {
                let mut j = i + d.len() + 1;
                let mut found = None;
                while j + d.len() <= b.len() {
                    if &b[j..j + d.len()] == d
                        && !b[j - 1].is_ascii_whitespace()
                        && (d[0] == b'*' || b.get(j + d.len()).is_none_or(|c| !c.is_ascii_alphanumeric()))
                    {
                        found = Some(j);
                        break;
                    }
                    j += 1;
                }
                if let Some(j) = found {
                    let whole = i..j + d.len();
                    if free(&whole, &taken) {
                        spans.push((i..i + d.len(), Span::Marker));
                        spans.push((i + d.len()..j, kind));
                        spans.push((j..j + d.len(), Span::Marker));
                        taken.push(whole.clone());
                        i = whole.end;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }
    spans.sort_by_key(|(r, _)| r.start);
}

fn find_seq(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from > hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, Span)> {
        style_line(line)
            .spans
            .into_iter()
            .map(|(r, s)| (line[r].to_string(), s))
            .collect()
    }

    /// ED-2
    #[test]
    fn headings_hide_their_marker() {
        let s = style_line("## Keep the Words");
        assert_eq!(s.kind, BlockKind::Heading(2));
        assert_eq!(s.hidden_prefix, 3);
        assert_eq!(style_line("#hashtag").kind, BlockKind::Paragraph);
    }

    /// ED-2
    #[test]
    fn blocks() {
        assert_eq!(style_line("> quoted").kind, BlockKind::Quote);
        assert_eq!(style_line("- item").kind, BlockKind::List);
        assert_eq!(style_line("12. item").kind, BlockKind::List);
        assert_eq!(style_line("---").kind, BlockKind::Rule);
        assert_eq!(style_line("* * *").kind, BlockKind::Rule);
        assert_eq!(style_line("   ").kind, BlockKind::Blank);
        let fenced = style_lines(["```", "let x = *y*;", "```", "*z*"].into_iter());
        assert_eq!(fenced[1].kind, BlockKind::Code);
        assert_eq!(fenced[3].spans.len(), 3);
    }

    /// ED-3
    #[test]
    fn inline_spans() {
        assert_eq!(
            kinds("a **b** and *c* `d*` [e](f)"),
            vec![
                ("**".into(), Span::Marker),
                ("b".into(), Span::Strong),
                ("**".into(), Span::Marker),
                ("*".into(), Span::Marker),
                ("c".into(), Span::Emphasis),
                ("*".into(), Span::Marker),
                ("`".into(), Span::Marker),
                ("d*".into(), Span::Code),
                ("`".into(), Span::Marker),
                ("[".into(), Span::Marker),
                ("e".into(), Span::Link),
                ("](f)".into(), Span::Marker),
            ]
        );
        assert!(kinds("snake_case_name and 2 * 3 * 4").is_empty());
    }
}
