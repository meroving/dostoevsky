pub use prose::{sentences, Sentence};

/// `[1] First sentence.` one per line, a blank line between paragraphs.
pub fn numbered(sentences: &[Sentence]) -> String {
    let mut out = String::new();
    let mut para = None;
    for (i, s) in sentences.iter().enumerate() {
        if para.is_some() && para != Some(s.paragraph) {
            out.push('\n');
        }
        para = Some(s.paragraph);
        out.push_str(&format!("[{}] {}\n", i + 1, s.text));
    }
    out
}

pub fn word_count(sentences: &[Sentence]) -> usize {
    sentences.iter().map(|s| english::word_count(&s.text)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_form_has_blank_lines_between_paragraphs() {
        let t = "One. Two.\nThree.";
        let n = numbered(&sentences(t, 0..t.len()));
        assert_eq!(n, "[1] One.\n[2] Two.\n\n[3] Three.\n");
    }
}
