use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineInfo {
    pub char_start: usize,
    pub byte_start: usize,
    pub utf16_start: usize,
    /// Without the trailing `\n`.
    pub char_len: usize,
    pub byte_len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct TextIndex {
    pub text: String,
    pub lines: Vec<LineInfo>,
    chars: usize,
    utf16: usize,
}

impl TextIndex {
    pub fn new(text: String) -> Self {
        let mut lines = Vec::new();
        let (mut c, mut b, mut u) = (0, 0, 0);
        for line in text.split('\n') {
            let (mut cl, mut ul) = (0, 0);
            for ch in line.chars() {
                cl += 1;
                ul += ch.len_utf16();
            }
            lines.push(LineInfo {
                char_start: c,
                byte_start: b,
                utf16_start: u,
                char_len: cl,
                byte_len: line.len(),
            });
            c += cl + 1;
            b += line.len() + 1;
            u += ul + 1;
        }
        TextIndex {
            chars: c - 1,
            utf16: u - 1,
            text,
            lines,
        }
    }

    pub fn len(&self) -> usize {
        self.chars
    }

    pub fn len_utf16(&self) -> usize {
        self.utf16
    }

    pub fn line_str(&self, i: usize) -> &str {
        let l = &self.lines[i];
        &self.text[l.byte_start..l.byte_start + l.byte_len]
    }

    /// An offset at a line end belongs to that line.
    pub fn line_of(&self, c: usize) -> usize {
        match self.lines.binary_search_by(|l| l.char_start.cmp(&c)) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn line_of_utf16(&self, u: usize) -> usize {
        match self.lines.binary_search_by(|l| l.utf16_start.cmp(&u)) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn line_of_byte(&self, b: usize) -> usize {
        match self.lines.binary_search_by(|l| l.byte_start.cmp(&b)) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn byte_in_line(&self, line: usize, c: usize) -> usize {
        let l = &self.lines[line];
        let s = self.line_str(line);
        let k = c.saturating_sub(l.char_start);
        s.char_indices().nth(k).map_or(s.len(), |(i, _)| i)
    }

    pub fn char_to_byte(&self, c: usize) -> usize {
        let c = c.min(self.chars);
        let i = self.line_of(c);
        self.lines[i].byte_start + self.byte_in_line(i, c)
    }

    pub fn byte_to_char(&self, b: usize) -> usize {
        let b = b.min(self.text.len());
        let i = self.line_of_byte(b);
        let l = &self.lines[i];
        let within = (b - l.byte_start).min(l.byte_len);
        l.char_start + self.line_str(i)[..floor_boundary(self.line_str(i), within)].chars().count()
    }

    pub fn char_to_utf16(&self, c: usize) -> usize {
        let c = c.min(self.chars);
        let i = self.line_of(c);
        let l = &self.lines[i];
        l.utf16_start
            + self
                .line_str(i)
                .chars()
                .take(c - l.char_start)
                .map(char::len_utf16)
                .sum::<usize>()
    }

    pub fn utf16_to_char(&self, u: usize) -> usize {
        let u = u.min(self.utf16);
        let i = self.line_of_utf16(u);
        let l = &self.lines[i];
        let mut acc = l.utf16_start;
        let mut c = l.char_start;
        for ch in self.line_str(i).chars() {
            if acc >= u {
                break;
            }
            acc += ch.len_utf16();
            c += 1;
        }
        c
    }

    pub fn slice(&self, r: Range<usize>) -> &str {
        &self.text[self.char_to_byte(r.start)..self.char_to_byte(r.end)]
    }

    pub fn char_at(&self, c: usize) -> Option<char> {
        if c >= self.chars {
            return None;
        }
        self.text[self.char_to_byte(c)..].chars().next()
    }

    pub fn prev_grapheme(&self, c: usize) -> usize {
        if c == 0 {
            return 0;
        }
        let i = self.line_of(c);
        let l = self.lines[i];
        if c == l.char_start {
            return c - 1;
        }
        let s = self.line_str(i);
        let b = self.byte_in_line(i, c);
        let start = s[..b].grapheme_indices(true).next_back().map_or(0, |(i, _)| i);
        l.char_start + s[..start].chars().count()
    }

    pub fn next_grapheme(&self, c: usize) -> usize {
        if c >= self.chars {
            return self.chars;
        }
        let i = self.line_of(c);
        let l = self.lines[i];
        if c == l.char_start + l.char_len {
            return c + 1;
        }
        let s = self.line_str(i);
        let b = self.byte_in_line(i, c);
        let g = s[b..].graphemes(true).next().map_or(0, str::len);
        l.char_start + s[..b + g].chars().count()
    }

    pub fn prev_word(&self, c: usize) -> usize {
        let chars: Vec<char> = self.window(c, true);
        let mut k = chars.len();
        while k > 0 && !english::is_word_char(chars[k - 1]) {
            k -= 1;
        }
        while k > 0 && english::is_word_char(chars[k - 1]) {
            k -= 1;
        }
        c - (chars.len() - k)
    }

    pub fn next_word(&self, c: usize) -> usize {
        let chars: Vec<char> = self.window(c, false);
        let mut k = 0;
        while k < chars.len() && !english::is_word_char(chars[k]) {
            k += 1;
        }
        while k < chars.len() && english::is_word_char(chars[k]) {
            k += 1;
        }
        c + k
    }

    /// For double-click; a lone non-word char is its own word.
    pub fn word_at(&self, c: usize) -> Range<usize> {
        let before = self.window(c, true);
        let after = self.window(c, false);
        let at_word = after.first().is_some_and(|ch| english::is_word_char(*ch))
            || before.last().is_some_and(|ch| english::is_word_char(*ch));
        if !at_word {
            return c..self.next_grapheme(c).min(self.line_end(self.line_of(c)));
        }
        let s = before.iter().rev().take_while(|ch| english::is_word_char(**ch)).count();
        let e = after.iter().take_while(|ch| english::is_word_char(**ch)).count();
        c - s..c + e
    }

    pub fn line_end(&self, line: usize) -> usize {
        let l = &self.lines[line];
        l.char_start + l.char_len
    }

    pub fn line_range(&self, line: usize) -> Range<usize> {
        self.lines[line].char_start..self.line_end(line)
    }

    /// The run of non-blank lines around `c`.
    pub fn paragraph_at(&self, c: usize) -> Range<usize> {
        let mut a = self.line_of(c);
        let mut b = a;
        if self.line_str(a).trim().is_empty() {
            return self.line_range(a);
        }
        while a > 0 && !self.line_str(a - 1).trim().is_empty() {
            a -= 1;
        }
        while b + 1 < self.lines.len() && !self.line_str(b + 1).trim().is_empty() {
            b += 1;
        }
        self.lines[a].char_start..self.line_end(b)
    }

    fn window(&self, c: usize, before: bool) -> Vec<char> {
        let b = self.char_to_byte(c);
        if before {
            let mut v: Vec<char> = self.text[..b].chars().rev().take(256).collect();
            v.reverse();
            v
        } else {
            self.text[b..].chars().take(256).collect()
        }
    }
}

fn floor_boundary(s: &str, mut b: usize) -> usize {
    while b > 0 && !s.is_char_boundary(b) {
        b -= 1;
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_round_trip() {
        let t = TextIndex::new("héllo 😀\n\nwörld".to_string());
        assert_eq!(t.len(), 14);
        assert_eq!(t.lines.len(), 3);
        for c in 0..=t.len() {
            assert_eq!(t.byte_to_char(t.char_to_byte(c)), c);
            assert_eq!(t.utf16_to_char(t.char_to_utf16(c)), c);
        }
        assert_eq!(t.char_to_utf16(8), 9); // the emoji is two UTF-16 units
        assert_eq!(t.slice(9..14), "wörld");
        assert_eq!(t.line_of(7), 0);
        assert_eq!(t.line_of(8), 1);
        assert_eq!(t.line_of(9), 2);
    }

    #[test]
    fn words_and_graphemes() {
        let t = TextIndex::new("Most of the friction, here".to_string());
        assert_eq!(t.word_at(14), 12..20);
        assert_eq!(t.next_word(11), 20);
        assert_eq!(t.prev_word(20), 12);
        let e = TextIndex::new("a👍🏽b".to_string());
        assert_eq!(e.next_grapheme(1), 3);
        assert_eq!(e.prev_grapheme(3), 1);
    }

    #[test]
    fn paragraphs() {
        let t = TextIndex::new("one\ntwo\n\nthree".to_string());
        assert_eq!(t.paragraph_at(5), 0..7);
        assert_eq!(t.paragraph_at(10), 9..14);
    }
}
