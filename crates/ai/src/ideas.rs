use crate::client::ChatRequest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Word,
    Sentence,
    Paragraph,
}

impl Scope {
    fn noun(self) -> &'static str {
        match self {
            Scope::Word => "word or phrase",
            Scope::Sentence => "sentence",
            Scope::Paragraph => "paragraph",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ask<'a> {
    pub title: Option<&'a str>,
    pub before: &'a str,
    /// The group's paragraph with the group's text marked as ⟦…⟧.
    pub paragraph: &'a str,
    pub after: &'a str,
    pub current: &'a str,
    pub scope: Scope,
    /// The original first.
    pub existing: &'a [String],
    /// The words after `??` (`?? plainer`).
    pub steer: Option<&'a str>,
    pub count: usize,
}

/// `start`, `end` in chars: `…the ⟦friction⟧ in…`.
pub fn mark(paragraph: &str, start: usize, end: usize) -> String {
    let cs: Vec<char> = paragraph.chars().collect();
    let (s, e) = (start.min(cs.len()), end.min(cs.len()));
    format!("{}⟦{}⟧{}", cs[..s].iter().collect::<String>(), cs[s..e].iter().collect::<String>(), cs[e..].iter().collect::<String>())
}

/// The line that means “no word here”: a version without the marked words.
pub const NONE: &str = "∅";

pub fn request(model: &str, a: &Ask) -> ChatRequest {
    let n = a.count;
    let noun = a.scope.noun();
    // A word or phrase can be one that the text reads better without (a doubled word, filler).
    let none = if a.scope == Scope::Word {
        format!(" If the text reads better with the marked words simply gone (a repeated word, filler), one of the lines may be just {NONE}, and only one.")
    } else {
        String::new()
    };
    let system = format!(
        "You help a writer choose words. You suggest replacements for the marked {noun} in their text. \
         Each suggestion must fit the sentence exactly where the marked text is, in the same grammatical role, \
         keeping the writer's voice, tense and register. A suggestion replaces only the marked text: never repeat the words just before or after it. Take the surrounding paragraphs into account: you are not a thesaurus. \
         Suggest different directions, not small variations of one idea. Never repeat a version the writer already has. \
         Write in the language of the text, whatever it is.{none} \
         Reply with exactly {n} suggestions, one per line, nothing else: no numbering, no quotes, no explanations."
    );
    let mut user = String::new();
    if let Some(t) = a.title.filter(|t| !t.trim().is_empty()) {
        user.push_str(&format!("Title: {}\n\n", t.trim()));
    }
    if !a.before.trim().is_empty() {
        user.push_str(&format!("{}\n\n", a.before.trim()));
    }
    user.push_str(&format!("{}\n\n", a.paragraph.trim()));
    if !a.after.trim().is_empty() {
        user.push_str(&format!("{}\n\n", a.after.trim()));
    }
    user.push_str(&format!("The marked {noun}: {}\n", a.current));
    if a.existing.len() > 1 {
        user.push_str("Versions the writer already has (don't repeat them; they show which way the writer is going):\n");
        for e in a.existing {
            user.push_str(&format!("- {e}\n"));
        }
    }
    if let Some(s) = a.steer.map(str::trim).filter(|s| !s.is_empty()) {
        user.push_str(&format!("Make the suggestions: {s}\n"));
    }
    user.push_str(&format!("\n{n} suggestions, one per line:"));
    // The limits fit five ideas; more ideas get room in proportion.
    let five = match a.scope {
        Scope::Word => 200,
        Scope::Sentence => 600,
        Scope::Paragraph => 2400,
    };
    let max_tokens = (five * n.max(5)).div_ceil(5) as u32;
    ChatRequest { model: model.to_string(), system, user, schema: None, max_tokens: Some(max_tokens) }
}

#[derive(Default)]
pub struct Lines {
    buf: String,
}

impl Lines {
    pub fn feed(&mut self, piece: &str) -> Vec<String> {
        self.buf.push_str(piece);
        let mut out = Vec::new();
        while let Some(i) = self.buf.find('\n') {
            let line: String = self.buf[..i].to_string();
            self.buf.drain(..=i);
            out.push(line);
        }
        out
    }

    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buf);
        (!rest.trim().is_empty()).then_some(rest)
    }
}

/// The line without numbering, bullets and quotes.
fn strip(line: &str) -> &str {
    let mut t = line.trim();
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 2 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            t = r.trim();
        }
    }
    for p in ["- ", "• ", "* ", "– "] {
        if let Some(r) = t.strip_prefix(p) {
            t = r.trim();
        }
    }
    for (a, b) in [('"', '"'), ('“', '”'), ('‘', '’'), ('⟦', '⟧'), ('*', '*')] {
        if t.chars().count() >= 2 && t.starts_with(a) && t.ends_with(b) {
            t = t[a.len_utf8()..t.len() - b.len_utf8()].trim();
        }
    }
    t.trim_matches(|c| c == '⟦' || c == '⟧').trim()
}

/// The line is [`NONE`]: the idea is a version without the marked words (words and phrases only).
pub fn is_none(line: &str, scope: Scope) -> bool {
    scope == Scope::Word && strip(line) == NONE
}

/// `None` for blank lines, preambles, repeats, [`NONE`] and lines far too long for the scope.
pub fn clean(line: &str, scope: Scope, existing: &[String]) -> Option<String> {
    let t = strip(line);
    if t.is_empty() || t == NONE {
        return None;
    }
    let lower = t.to_lowercase();
    if lower.ends_with(':') || lower.starts_with("here are") || lower.starts_with("sure") || lower.starts_with("suggestions") {
        return None;
    }
    if existing.iter().any(|e| e.trim().to_lowercase() == lower) {
        return None;
    }
    let words = english::word_count(t);
    let too_long = match scope {
        Scope::Word => words > 8,
        Scope::Sentence => words > 80,
        Scope::Paragraph => false,
    };
    if too_long {
        return None;
    }
    Some(t.to_string())
}

/// The idea without the words around the marked text it repeats: `for just as much time` for
/// `long` in `for just as long,` is `much time`. `before` and `after` are the text either side.
/// `None` if nothing is left.
pub fn unrepeat(idea: &str, before: &str, after: &str) -> Option<String> {
    let same = |a: &[char], b: &[char]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_lowercase().eq(y.to_lowercase()));
    let word = |c: &char| c.is_alphanumeric();
    let mut s: Vec<char> = idea.trim().chars().collect();
    let b: Vec<char> = before.trim_end().chars().collect();
    // The longest start of the idea that ends `before`, in whole words.
    let lead = (1..=s.len().min(b.len())).rev().find(|&k| {
        let tail = &b[b.len() - k..];
        tail.iter().any(word) && same(&s[..k], tail) && s.get(k).is_none_or(|c| !word(c)) && (k == b.len() || !word(&b[b.len() - k - 1]) || !word(&tail[0]))
    });
    if let Some(k) = lead {
        s.drain(..k);
    }
    let a: Vec<char> = after.trim_start().chars().collect();
    // The longest end of the idea that starts `after`.
    let trail = (1..=s.len().min(a.len())).rev().find(|&k| {
        let head = &a[..k];
        head.iter().any(word) && same(&s[s.len() - k..], head) && a.get(k).is_none_or(|c| !word(c)) && (k == s.len() || !word(&s[s.len() - k - 1]) || !word(&head[0]))
    });
    if let Some(k) = trail {
        s.truncate(s.len() - k);
    }
    let t: String = s.into_iter().collect();
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idea_drops_the_words_around_it_it_repeats() {
        let (before, after) = ("plays. And for just as ", ", I've been looking for the perfect app");
        assert_eq!(unrepeat("for just as much time", before, after).as_deref(), Some("much time"));
        assert_eq!(unrepeat("For Just As far back", before, after).as_deref(), Some("far back"));
        assert_eq!(unrepeat("as many years", before, after).as_deref(), Some("many years"));
        assert_eq!(unrepeat("much time, I've been", before, after).as_deref(), Some("much time"));
        assert_eq!(unrepeat("equally long", before, after).as_deref(), Some("equally long"));
        // Only whole words: `ask` doesn't end `as`, `stay` isn't `as`.
        assert_eq!(unrepeat("ask", before, after).as_deref(), Some("ask"));
        assert_eq!(unrepeat("ages", "just as", "").as_deref(), Some("ages"));
        assert_eq!(unrepeat("s long", "just as", "").as_deref(), Some("s long"));
        assert_eq!(unrepeat("for just as", before, after), None, "nothing new");
        assert_eq!(unrepeat("much time", "", "").as_deref(), Some("much time"));
    }

    #[test]
    fn lines_arrive_whole() {
        let mut l = Lines::default();
        assert!(l.feed("margin no").is_empty());
        assert_eq!(l.feed("te\naside\nglo"), ["margin note", "aside"]);
        assert_eq!(l.finish().as_deref(), Some("glo"));
        assert_eq!(l.finish(), None);
    }

    #[test]
    fn cleaning() {
        let have = vec!["suggestion".to_string(), "note".to_string()];
        assert_eq!(clean("1. margin note", Scope::Word, &have).as_deref(), Some("margin note"));
        assert_eq!(clean("- “aside”", Scope::Word, &have).as_deref(), Some("aside"));
        assert_eq!(clean("Note", Scope::Word, &have), None);
        assert_eq!(clean("Here are five ideas:", Scope::Word, &have), None);
        assert_eq!(clean("   ", Scope::Word, &have), None);
        assert_eq!(clean("one two three four five six seven eight nine", Scope::Word, &have), None);
        assert_eq!(clean("don't", Scope::Word, &have).as_deref(), Some("don't"));
        assert_eq!(clean("∅", Scope::Word, &have), None);
        assert!(is_none("3. ∅", Scope::Word) && is_none(" “∅” ", Scope::Word));
        assert!(!is_none("∅", Scope::Sentence) && !is_none("∅ maybe", Scope::Word));
    }

    #[test]
    fn a_word_may_be_none() {
        let have = vec!["the".to_string()];
        let ask = |scope| Ask { title: None, before: "", paragraph: "the ⟦the⟧ sentence", after: "", current: "the", scope, existing: &have, steer: None, count: 5 };
        assert!(request("m", &ask(Scope::Word)).system.contains("may be just ∅"));
        assert!(!request("m", &ask(Scope::Sentence)).system.contains('∅'));
    }

    #[test]
    fn the_prompt_has_the_context_and_the_versions() {
        let have = vec!["suggestion".to_string(), "note".to_string(), "alternative".to_string()];
        let p = mark("They write an alternative in the margin.", 14, 25);
        assert_eq!(p, "They write an ⟦alternative⟧ in the margin.");
        let a = Ask { title: Some("Keep the Words"), before: "Editors know.", paragraph: &p, after: "", current: "alternative", scope: Scope::Word, existing: &have, steer: Some("plainer"), count: 5 };
        let r = request("m", &a);
        assert!(r.user.contains("Title: Keep the Words"));
        assert!(r.user.contains("⟦alternative⟧"));
        assert!(r.user.contains("- note"));
        assert!(r.user.contains("Make the suggestions: plainer"));
        assert!(r.system.contains("exactly 5 suggestions"));
        assert_eq!(r.max_tokens, Some(200));
    }

    #[test]
    fn more_ideas_get_more_room() {
        let have = vec!["alternative".to_string()];
        let ask = |count, scope| Ask { title: None, before: "", paragraph: "⟦alternative⟧", after: "", current: "alternative", scope, existing: &have, steer: None, count };
        let r = request("m", &ask(10, Scope::Sentence));
        assert!(r.system.contains("exactly 10 suggestions"));
        assert!(r.user.ends_with("10 suggestions, one per line:"));
        assert_eq!(r.max_tokens, Some(1200));
        assert_eq!(request("m", &ask(8, Scope::Word)).max_tokens, Some(320));
        // Fewer ideas keep the room for five: a long reply is cut by the count, not the limit.
        assert_eq!(request("m", &ask(3, Scope::Paragraph)).max_tokens, Some(2400));
    }
}
