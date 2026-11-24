use std::ops::Range;

use l10n::{tr, trn};
use serde_json::{json, Value};

use crate::anchor;
use crate::client::{AiError, ChatRequest};
use crate::segment::{numbered, Sentence};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pass {
    Mistakes,
    HardToRead,
    Tone,
}

impl Pass {
    pub const ALL: [Pass; 3] = [Pass::Mistakes, Pass::HardToRead, Pass::Tone];

    pub fn menu_label(self) -> &'static str {
        match self {
            Pass::Mistakes => tr!("Fix mistakes"),
            Pass::HardToRead => tr!("Hard to read"),
            Pass::Tone => tr!("Word choice"),
        }
    }

    pub fn menu_hint(self) -> &'static str {
        match self {
            Pass::Mistakes => tr!("Spelling, punctuation, agreement, tense"),
            Pass::HardToRead => tr!("Long, tangled or passive sentences"),
            Pass::Tone => tr!("Off-tone words, jargon, clichés, echoes"),
        }
    }

    pub fn menu_outcome(self) -> &'static str {
        match self {
            Pass::Mistakes => tr!("fixes as alternatives"),
            Pass::HardToRead => tr!("versions"),
            Pass::Tone => tr!("alternatives"),
        }
    }

    pub fn working_label(self) -> String {
        match self {
            Pass::Mistakes => tr!("Looking for mistakes…"),
            Pass::HardToRead => tr!("Looking for tangled sentences…"),
            Pass::Tone => tr!("Looking for words that jar…"),
        }
        .into()
    }

    pub fn nothing_label(self) -> String {
        match self {
            Pass::Mistakes => tr!("No mistakes found"),
            Pass::HardToRead => tr!("Nothing hard to read"),
            Pass::Tone => tr!("Nothing to mark in word choice"),
        }
        .into()
    }

    pub fn word_level(self) -> bool {
        self == Pass::Tone
    }

    /// What *Suggest alternatives* / *Suggest versions* on a mark asks the ideas model for.
    pub fn steer(self, note: &str, found: Found) -> String {
        let note = note.trim();
        match found {
            Found::Passive => return "in the active voice: whoever acts is the subject and comes first; the same meaning".into(),
            Found::Echo => return "without the repetition".into(),
            Found::Model | Found::Long => {}
        }
        let because = |s: &str| if note.is_empty() { s.to_string() } else { format!("{s} (why it was marked: {note})") };
        match self {
            Pass::Tone => {
                if note.is_empty() {
                    "fit the tone of the rest of the text".into()
                } else {
                    format!("fit the tone of the rest of the text (the marked words jar: {note})")
                }
            }
            Pass::HardToRead => because("easy to read the first time: shorter, plain order, the subject up front, the same meaning; two sentences if it needs them"),
            Pass::Mistakes => because("grammatically correct, in the tense the passage is told in: change only what is wrong, keep the writer's words, meaning and voice"),
        }
    }

    fn task(self) -> String {
        match self {
            Pass::Mistakes => "Find mistakes a careful reader would trip on: typos, misspellings, doubled words, wrong or missing punctuation, \
                 and grammar a spell checker misses: a verb that slips out of the tense the passage is told in, a broken sequence of tenses \
                 (\"he said he will come\"), a subject and verb that don't agree, a wrong article or preposition, a missing word. \
                 Judge tense against the paragraphs around it: dialogue, a flashback, a general truth or a deliberate shift is not a mistake. \
                 Only real mistakes: never change style, word choice, wording you find weak, deliberate fragments or dialect, and never shorten anything. \
                 For each, quote the smallest stretch that contains the mistake exactly as written (a word or a few words; \
                 when putting it right changes the words beside it too, as agreement does, quote them all), \
                 give the corrected text for that stretch as \"fix\" (it must differ from the quote), and a note that names the mistake in one to four words, \
                 like \"spelling\", \"doubled word\", \"agreement\", \"missing comma\" or \"tense: past\" (the fix itself is shown beside it). \
                 When changing a few words can't put a sentence right (a dangling or misplaced modifier, broken parallelism, \
                 a sentence that doesn't hold together), quote the whole sentence, leave \"fix\" empty and say in the note what is wrong."
                .into(),
            Pass::HardToRead => format!(
                "Find convoluted sentences: ones a reader has to read twice because of nested clauses, misplaced modifiers, \
                 buried subjects or tangled order. Sentences over {LONG_SENTENCE} words are counted separately: don't list a sentence only for its length. \
                 Only clear cases. For each, quote the whole sentence exactly and say in one short line what makes it hard."
            ),
            Pass::Tone => "Find words or short phrases that don't fit the tone of the rest of the text (too formal, too casual, jargon, cliché). \
                 Only clear cases. Quote just the word or phrase exactly as written and say in one short line why it jars."
                .into(),
        }
    }

    fn schema(self) -> Value {
        let mut props = json!({
            "sentence": {"type": "integer", "description": "The sentence number, as in [n]."},
            "quote": {"type": "string", "description": "Copied exactly from that sentence."},
            "note": {"type": "string", "description": "One short line."},
        });
        let mut required = vec!["sentence", "quote", "note"];
        if self == Pass::Mistakes {
            props["fix"] = json!({"type": "string", "description": "The corrected text for the quote; empty when the sentence needs rebuilding."});
            required.push("fix");
        }
        json!({
            "type": "object",
            "properties": {"marks": {"type": "array", "items": {
                "type": "object", "properties": props, "required": required, "additionalProperties": false
            }}},
            "required": ["marks"],
            "additionalProperties": false
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    /// Display chars in the document.
    pub range: Range<usize>,
    pub note: String,
    /// `None`: the sentence needs rebuilding.
    pub fix: Option<String>,
    pub found: Found,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Found {
    #[default]
    Model,
    Long,
    Passive,
    Echo,
}

pub const LONG_SENTENCE: usize = 32;

pub fn long_sentences(sentences: &[Sentence]) -> Vec<Mark> {
    sentences
        .iter()
        .filter_map(|s| {
            let n = english::word_count(&s.text);
            (n > LONG_SENTENCE).then(|| Mark { range: s.range.clone(), note: trn!("{n} word.", "{n} words.", n), fix: None, found: Found::Long })
        })
        .collect()
}

pub fn request(pass: Pass, model: &str, title: Option<&str>, sentences: &[Sentence]) -> ChatRequest {
    let shape = if pass == Pass::Mistakes {
        r#"{"marks": [{"sentence": 3, "quote": "teh", "fix": "the", "note": "spelling"}, {"sentence": 5, "quote": "…", "fix": "", "note": "…"}]}"#
    } else {
        r#"{"marks": [{"sentence": 3, "quote": "…", "note": "…"}]}"#
    };
    let system = format!(
        "You are a careful editor reading a writer's draft. The draft comes as numbered sentences, [n] before each, \
         with a blank line between paragraphs. {} \
         Never rewrite the text. Notes are read beside the text, so never mention sentence numbers in them. \
         Write the notes in the language the draft is written in. \
         If nothing qualifies, return an empty list. \
         Reply with JSON only, shaped like {shape}.",
        pass.task()
    );
    let mut user = String::new();
    if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
        user.push_str(&format!("Title: {}\n\n", t.trim()));
    }
    // The document first, so passes in a row reuse the provider's prompt cache.
    user.push_str(&numbered(sentences));
    ChatRequest { model: model.to_string(), system, user, schema: Some(("marks".into(), pass.schema())), max_tokens: Some(8000) }
}

/// What versions of a word mark are asked with (the Word choice pass).
#[derive(Clone, Debug)]
pub struct WordAsk<'a> {
    pub title: Option<&'a str>,
    pub before: &'a str,
    /// The paragraph with the marked words as ⟦…⟧.
    pub paragraph: &'a str,
    pub after: &'a str,
    /// The marked sentence, as written.
    pub sentence: &'a str,
    pub current: &'a str,
    /// What the versions should be ([`Pass::steer`]).
    pub why: &'a str,
    /// An echo: the earlier word it repeats, and that word's sentence.
    pub echo: Option<(&'a str, &'a str)>,
    pub count: usize,
}

/// A version of a word mark: `range` (display chars, inside the marked sentence, taking in the
/// marked words) reads `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordVersion {
    pub range: Range<usize>,
    pub text: String,
}

/// A word can't always change alone: another gender or number changes the adjective and the verb
/// beside it, a noun its article, a pronoun further on. Models get a whole sentence right far more
/// often than a splice, so each version is the marked sentence written out; [`parse_word_versions`]
/// finds what changed.
pub fn word_versions_request(model: &str, a: &WordAsk) -> ChatRequest {
    let n = a.count;
    let system = format!(
        "You help a writer replace marked words in their draft. The marked words are ⟦…⟧ in the text; the sentence they are in follows it. \
         Suggest {n} versions of that sentence, each with the marked words replaced, written out in full. \
         Change only what the replacement needs: the marked words and, where the sentence would not read right otherwise, the words that go with them \
         (gender, number and case agreement, an article, a preposition, a verb form, a pronoun further on that points back to them). \
         Copy every other word and mark of the sentence exactly as written. Each version must read correctly and naturally as a whole. \
         Keep the writer's meaning, voice, tense and register, and take the surrounding paragraphs into account: you are not a thesaurus. \
         Suggest different directions, not small variations of one idea. A version may simply leave the marked words out, when the sentence reads better without them. \
         Write in the language of the text, whatever it is. \
         Reply with JSON only, shaped like {{\"versions\": [{{\"sentence\": \"…\"}}]}}."
    );
    let mut user = String::new();
    if let Some(t) = a.title.filter(|t| !t.trim().is_empty()) {
        user.push_str(&format!("Title: {}\n\n", t.trim()));
    }
    for part in [a.before, a.paragraph, a.after] {
        if !part.trim().is_empty() {
            user.push_str(&format!("{}\n\n", part.trim()));
        }
    }
    user.push_str(&format!("The sentence: {}\nThe marked words: {}\n", a.sentence.trim(), a.current));
    if let Some((first, where_)) = a.echo {
        user.push_str(&format!(
            "They repeat \"{first}\", used just before (in: {}). Every version must leave the text without that repetition: \
             another word, a pronoun, the word left out, or the sentence turned around it; if both are in this sentence, the earlier one may change instead.\n",
            where_.trim()
        ));
    }
    if !a.why.trim().is_empty() {
        user.push_str(&format!("Make the replacements: {}\n", a.why.trim()));
    }
    let schema = json!({
        "type": "object",
        "properties": {"versions": {"type": "array", "items": {
            "type": "object",
            "properties": {"sentence": {"type": "string", "description": "The whole sentence with the replacement in place."}},
            "required": ["sentence"],
            "additionalProperties": false
        }}},
        "required": ["versions"],
        "additionalProperties": false
    });
    let room = (a.sentence.chars().count() / 2 + 40) * n.max(3) + 100;
    ChatRequest { model: model.to_string(), system, user, schema: Some(("versions".into(), schema)), max_tokens: Some(room as u32) }
}

/// What each version changed in `sentence` (display chars in `text`), in whole words. A version
/// must change the words at `mark` (or `first`, the word an echo repeats, when it's in this
/// sentence) and share a third of the sentence's other words (else it's about another sentence);
/// agreement may carry a change across all of it (a noun changed at the start, its verb and
/// pronoun at the end). Repeats of the sentence or of one another are dropped.
pub fn parse_word_versions(reply: &str, text: &[char], sentence: Range<usize>, mark: Range<usize>, first: Option<Range<usize>>) -> Result<Vec<WordVersion>, AiError> {
    let v = crate::json_object(reply).ok_or_else(|| AiError::BadReply("no JSON".into()))?;
    let old = &text[sentence.clone()];
    // Leading and trailing spaces are the sentence's own, not the model's to change.
    let lead = old.iter().take_while(|c| c.is_whitespace()).count();
    let trail = old[lead..].iter().rev().take_while(|c| c.is_whitespace()).count();
    let core = &old[lead..old.len() - trail];
    let base = sentence.start + lead;
    let words = |cs: &[char]| -> Vec<String> {
        cs.iter().collect::<String>().split(|c: char| !english::is_word_char(c)).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
    };
    let marked = words(&text[mark.clone()]);
    let others: Vec<String> = words(core).into_iter().filter(|w| !marked.contains(w)).collect();
    let mut out: Vec<WordVersion> = Vec::new();
    for it in v["versions"].as_array().cloned().unwrap_or_default() {
        let new: Vec<char> = it["sentence"].as_str().unwrap_or("").trim().chars().collect();
        if new.is_empty() {
            continue;
        }
        let (was, now) = changed(core, &new);
        let range = base + was.start..base + was.end;
        let touches = |m: &Range<usize>| range.start < m.end && m.start < range.end || (range.is_empty() && m.start <= range.start && range.start <= m.end);
        if was.is_empty() && now.is_empty() || !(touches(&mark) || first.as_ref().is_some_and(touches)) {
            continue;
        }
        let new_words = words(&new);
        if others.iter().filter(|w| new_words.contains(w)).count() * 3 < others.len() {
            continue;
        }
        let version = WordVersion { range, text: new[now].iter().collect() };
        if out.iter().any(|o| spliced(text, o) == spliced(text, &version)) {
            continue;
        }
        out.push(version);
    }
    Ok(out)
}

/// The stretch of `old` that `new` changes and what it reads there, both in whole words: the
/// longest common start and end, loosely (curly and straight quotes alike), backed off to the
/// nearest word boundary.
fn changed(old: &[char], new: &[char]) -> (Range<usize>, Range<usize>) {
    let loose = |c: char| match c {
        '\u{2018}' | '\u{2019}' | '\u{02BC}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}' => '"',
        '\u{00A0}' => ' ',
        c => c,
    };
    let same = |a: char, b: char| loose(a) == loose(b);
    let word = |c: char| english::is_word_char(c);
    let mut p = old.iter().zip(new).take_while(|(a, b)| same(**a, **b)).count();
    while p > 0 && word(old[p - 1]) && (old.get(p).is_some_and(|c| word(*c)) || new.get(p).is_some_and(|c| word(*c))) {
        p -= 1;
    }
    let room = old.len().min(new.len()) - p;
    let mut s = old.iter().rev().zip(new.iter().rev()).take(room).take_while(|(a, b)| same(**a, **b)).count();
    while s > 0 && word(old[old.len() - s]) && (old.len() - s > p && word(old[old.len() - s - 1]) || new.len() - s > p && word(new[new.len() - s - 1])) {
        s -= 1;
    }
    (p..old.len() - s, p..new.len() - s)
}

/// The sentence-wide text with `v` in place, for comparing versions with different stretches.
fn spliced(text: &[char], v: &WordVersion) -> String {
    let mut s: String = text[..v.range.start].iter().collect();
    s.push_str(&v.text);
    s.extend(&text[v.range.end..]);
    s
}

/// One stretch for all the versions (an alternatives group has one): the union of theirs, each
/// version filled out to it with the words it leaves as they are. An empty one is the words gone.
pub fn one_span(text: &[char], versions: &[WordVersion]) -> Option<(Range<usize>, Vec<String>)> {
    let start = versions.iter().map(|v| v.range.start).min()?;
    let end = versions.iter().map(|v| v.range.end).max()?;
    let alts = versions
        .iter()
        .map(|v| {
            let mut s: String = text[start..v.range.start].iter().collect();
            s.push_str(&v.text);
            s.extend(&text[v.range.end..end]);
            s
        })
        .collect();
    Some((start..end, alts))
}

/// Unanchored quotes are dropped; marks are in page order and don't overlap, fixes winning.
pub fn parse(pass: Pass, reply: &str, sentences: &[Sentence], text: &[char]) -> Result<Vec<Mark>, AiError> {
    let v = crate::json_object(reply).ok_or_else(|| AiError::BadReply("no JSON".into()))?;
    let items = v["marks"].as_array().cloned().unwrap_or_default();
    let sentence_of = |n: i64| usize::try_from(n).ok().and_then(|n| n.checked_sub(1)).and_then(|i| sentences.get(i)).map(|s| s.range.clone());
    let mut found: Vec<Mark> = Vec::new();
    for it in items {
        let n = it["sentence"].as_i64().or_else(|| it["sentence"].as_str().and_then(|s| s.trim_matches(|c| c == '[' || c == ']').parse().ok())).unwrap_or(0);
        let quote = it["quote"].as_str().unwrap_or("");
        let note = it["note"].as_str().unwrap_or("").trim().to_string();
        let anchored = anchor::near_sentence(sentences, n, quote);
        let mark = match pass {
            Pass::Tone => match anchored {
                Some(r) => Mark { range: anchor::to_words(text, r), note, fix: None, found: Found::Model },
                None => continue,
            },
            Pass::HardToRead => match anchored.or_else(|| sentence_of(n)) {
                // A sentence-level mark whose quote drifted: the sentence it names is still right.
                Some(r) => Mark { range: r, note, fix: None, found: Found::Model },
                None => continue,
            },
            Pass::Mistakes => {
                let fix = it["fix"].as_str().map(str::trim).filter(|f| !f.is_empty());
                let words = anchored.clone().map(|r| anchor::to_words(text, r));
                match (fix, words) {
                    (Some(f), Some(r)) if plausible_fix(&text[r.clone()].iter().collect::<String>(), f) => Mark { range: r, note, fix: Some(f.to_string()), found: Found::Model },
                    (Some(_), _) => continue,
                    (None, _) => match anchored.and_then(|r| sentences.iter().find(|s| s.range.start <= r.start && r.end <= s.range.end)).map(|s| s.range.clone()).or_else(|| sentence_of(n)) {
                        Some(r) => Mark { range: r, note, fix: None, found: Found::Model },
                        None => continue,
                    },
                }
            }
        };
        found.push(mark);
    }
    // Fixes first, so a sentence marked for rebuilding doesn't push out the fixes in it.
    found.sort_by_key(|m| m.fix.is_none());
    let mut out: Vec<Mark> = Vec::new();
    for m in found {
        if !out.iter().any(|o| o.range.start < m.range.end && m.range.start < o.range.end) {
            out.push(m);
        }
    }
    out.sort_by_key(|m| m.range.start);
    Ok(out)
}

/// A sentence the model also found keeps its count.
pub fn join(local: &[Mark], found: Vec<Mark>) -> Vec<Mark> {
    let mut out: Vec<Mark> = local.to_vec();
    for m in found {
        if !out.iter().any(|o| o.range.start < m.range.end && m.range.start < o.range.end) {
            out.push(m);
        }
    }
    out.sort_by_key(|m| m.range.start);
    out
}

/// A typo fix changes a little: word count ±1, length at most about doubled or halved.
fn plausible_fix(quote: &str, fix: &str) -> bool {
    if fix.is_empty() || fix == quote {
        return false;
    }
    let (a, b) = (english::word_count(quote) as isize, english::word_count(fix) as isize);
    let (la, lb) = (quote.chars().count(), fix.chars().count());
    (a - b).abs() <= 1 && lb <= la * 2 + 2 && lb * 2 + 2 >= la
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::sentences;

    fn doc(t: &str) -> (Vec<Sentence>, Vec<char>) {
        (sentences(t, 0..t.chars().count()), t.chars().collect())
    }

    fn quoted(c: &[char], m: &[Mark]) -> Vec<String> {
        m.iter().map(|m| c[m.range.clone()].iter().collect()).collect()
    }

    #[test]
    fn long_is_local() {
        let long = "Word ".to_string() + &"word ".repeat(39) + "end.";
        let t = format!("Short one. {long}");
        let (s, _) = doc(&t);
        let m = long_sentences(&s);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].note, "41 words.");
        assert_eq!(m[0].found, Found::Long);
    }

    #[test]
    fn tone_marks_anchor_or_drop() {
        let t = "Editors have always known this. It is, in a way, basically a one-way street, really.";
        let (s, c) = doc(t);
        let reply = r#"{"marks":[
            {"sentence":2,"quote":"basically","note":"casual"},
            {"sentence":2,"quote":"in a way","note":"vague"},
            {"sentence":1,"quote":"not in the text","note":"x"},
            {"sentence":2,"quote":"basic","note":"overlaps basically once widened"}
        ]}"#;
        let m = parse(Pass::Tone, reply, &s, &c).unwrap();
        assert_eq!(quoted(&c, &m), ["in a way", "basically"]);
    }

    #[test]
    fn a_sentence_mark_with_a_drifted_quote_keeps_its_sentence() {
        let t = "One. Two is hard to follow.";
        let (s, c) = doc(t);
        let m = parse(Pass::HardToRead, r#"{"marks":[{"sentence":2,"quote":"Two is so hard","note":"tangled"}]}"#, &s, &c).unwrap();
        assert_eq!(quoted(&c, &m), ["Two is hard to follow."]);
        assert_eq!(m[0].found, Found::Model);
    }

    #[test]
    fn mistakes_fix_words_and_mark_what_needs_rebuilding() {
        let t = "You pick teh word. She walked in and sits down. Hovering over one, the keys show it.";
        let (s, c) = doc(t);
        let reply = r#"```json
{"marks":[{"sentence":3,"quote":"Hovering over one, the keys show it.","fix":"","note":"dangling: the keys don't hover"},
{"sentence":1,"quote":"teh","fix":"the","note":"teh → the"},
{"sentence":2,"quote":"sits","fix":"sat","note":"slips into the present"},
{"sentence":2,"quote":"walked","fix":"really did not walk at all to anyone","note":"a rewrite, not a fix"}]}
```"#;
        let m = parse(Pass::Mistakes, reply, &s, &c).unwrap();
        assert_eq!(quoted(&c, &m), ["teh", "sits", "Hovering over one, the keys show it."], "the rewrite is refused");
        assert_eq!(m[0].fix.as_deref(), Some("the"));
        assert_eq!(m[1].fix.as_deref(), Some("sat"));
        assert_eq!(m[2].fix, None, "to rebuild");
    }

    #[test]
    fn an_oversized_fix_is_a_rewrite_and_refused() {
        let t = "One. It dont matter.";
        let (s, c) = doc(t);
        let m = parse(Pass::Mistakes, r#"{"marks":[{"sentence":2,"quote":"matter","fix":"really does not matter at all to anyone","note":"rewrite"}]}"#, &s, &c).unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn hard_to_read_joins_the_count_and_the_model() {
        let long = "Word ".to_string() + &"word ".repeat(39) + "end.";
        let t = format!("Short one. {long} The thing that the man who the dog bit saw ran.");
        let (s, c) = doc(&t);
        let local = long_sentences(&s);
        let found = parse(
            Pass::HardToRead,
            r#"{"marks":[{"sentence":3,"quote":"The thing that the man who the dog bit saw ran.","note":"nested"},{"sentence":2,"quote":"Word","note":"long and tangled"}]}"#,
            &s,
            &c,
        )
        .unwrap();
        let m = join(&local, found);
        assert_eq!(m.len(), 2);
        assert!(m[0].found == Found::Long && m[0].note == "41 words.", "the count stays");
        assert_eq!(m[1].found, Found::Model);
    }

    #[test]
    fn what_each_pass_asks_for_versions() {
        assert!(Pass::HardToRead.steer("Nested clauses.", Found::Model).ends_with("(why it was marked: Nested clauses.)"));
        assert!(Pass::Tone.steer("too casual", Found::Model).contains("jar: too casual"));
        assert!(Pass::Mistakes.steer("slips into the present", Found::Model).contains("tense the passage is told in"));
        assert_eq!(Pass::ALL.iter().filter(|p| p.word_level()).count(), 1);
    }

    #[test]
    fn the_mistakes_request_asks_for_fixes_and_tense() {
        let (s, _) = doc("One. Two.");
        let r = request(Pass::Mistakes, "m", Some("T"), &s);
        assert!(r.user.starts_with("Title: T\n\n[1] One.\n[2] Two."));
        assert!(r.system.contains("tense the passage is told in") && r.system.contains("leave \"fix\" empty"));
        assert!(r.schema.as_ref().unwrap().1.to_string().contains("\"fix\""));
        let h = request(Pass::HardToRead, "m", None, &s);
        assert!(h.system.contains("Sentences over 32 words") && !h.schema.unwrap().1.to_string().contains("\"fix\""));
    }

    #[test]
    fn word_versions_are_what_changed_in_the_sentence() {
        // «дом» → «здание» changes «оказался» too.
        let t = "Мы жили в доме. Старый дом нашёлся, и дом оказался пуст до самой крыши.";
        let c: Vec<char> = t.chars().collect();
        let sentence = 16..t.chars().count();
        let mark = 38..41;
        assert_eq!(c[mark.clone()].iter().collect::<String>(), "дом");
        let reply = r#"{"versions":[
            {"sentence":"Старый дом нашёлся, и он оказался пуст до самой крыши."},
            {"sentence":"Другая фраза: дом совсем не о том."},
            {"sentence":"Старый дом нашёлся, и здание оказалось пусто до самой крыши."},
            {"sentence":"  Старый дом нашёлся, и оказался пуст до самой крыши. "},
            {"sentence":"Старый дом нашёлся, и дом оказался пуст до самой крыши."},
            {"sentence":"Старый дом нашёлся, и он оказался пуст до самой крыши."},
            {"sentence":"Старинный дом нашёлся, и дом оказался пуст до самой крыши."},
            {"sentence":"Совсем другая фраза о чём-то своём."},
            {"sentence":"Старое здание нашлось, и оно оказалось пусто до самой крыши."}
        ]}"#;
        let v = parse_word_versions(reply, &c, sentence.clone(), mark.clone(), None).unwrap();
        let shown: Vec<(String, &str)> = v.iter().map(|v| (c[v.range.clone()].iter().collect(), v.text.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("дом".into(), "он"),
                ("дом оказался пуст".into(), "здание оказалось пусто"),
                ("дом ".into(), ""),
                ("Старый дом нашёлся, и дом оказался пуст".into(), "Старое здание нашлось, и оно оказалось пусто")
            ],
            "as written, a repeat, a change away from the mark and another sentence are dropped; the earlier word may change instead"
        );
        let first = 23..26;
        assert_eq!(c[first.clone()].iter().collect::<String>(), "дом");
        let earlier = r#"{"versions":[{"sentence":"Старый особняк нашёлся, и дом оказался пуст до самой крыши."}]}"#;
        assert!(parse_word_versions(earlier, &c, sentence.clone(), mark.clone(), None).unwrap().is_empty(), "not the marked word");
        let v2 = parse_word_versions(earlier, &c, sentence, mark, Some(first)).unwrap();
        assert_eq!(v2[0].text, "особняк", "an echo's earlier word may change instead");
        let (span, alts) = one_span(&c, &v[..3]).unwrap();
        assert_eq!(c[span].iter().collect::<String>(), "дом оказался пуст");
        assert_eq!(alts, ["он оказался пуст", "здание оказалось пусто", "оказался пуст"]);
    }

    #[test]
    fn agreement_may_carry_a_version_across_the_sentence() {
        let t = "Лампа светила тускло, но мама была довольна лампой.";
        let c: Vec<char> = t.chars().collect();
        let all = 0..c.len();
        let (first, mark) = (0..5, 44..50);
        assert_eq!(c[mark.clone()].iter().collect::<String>(), "лампой");
        let reply = r#"{"versions":[{"sentence":"Она светила тускло, но мама была ею довольна."}]}"#;
        let v = parse_word_versions(reply, &c, all, mark, Some(first)).unwrap();
        assert_eq!(c[v[0].range.clone()].iter().collect::<String>(), "Лампа светила тускло, но мама была довольна лампой");
        assert_eq!(v[0].text, "Она светила тускло, но мама была ею довольна");
    }

    #[test]
    fn what_changed_is_in_whole_words_and_keeps_its_spaces() {
        let diff = |a: &str, b: &str| {
            let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
            let (x, y) = changed(&a, &b);
            (a[x].iter().collect::<String>(), b[y].iter().collect::<String>())
        };
        assert_eq!(diff("Der Tisch war schwer, und der Tisch passte.", "Der Tisch war schwer und passte."), (", und der Tisch".into(), " und".into()));
        assert_eq!(diff("a super deep sorrow he had", "a sorrow that ran deep he had"), ("super deep sorrow".into(), "sorrow that ran deep".into()));
        assert_eq!(diff("the house was", "the houses were"), ("house was".into(), "houses were".into()), "never half a word");
        assert_eq!(diff("“Quoted” word.", "\"Quoted\" term."), ("word".into(), "term".into()), "curly and straight quotes alike");
        assert_eq!(diff("same", "same"), (String::new(), String::new()));
    }

    #[test]
    fn the_word_versions_request_shows_the_echo() {
        let a = WordAsk {
            title: None,
            before: "",
            paragraph: "Старый дом стоял у реки, и ⟦дом⟧ был пуст.",
            after: "",
            sentence: "Старый дом стоял у реки, и дом был пуст.",
            current: "дом",
            why: &Pass::Tone.steer("", Found::Echo),
            echo: Some(("дом", "Старый дом стоял у реки, и дом был пуст.")),
            count: 3,
        };
        let r = word_versions_request("m", &a);
        assert!(r.system.contains("Suggest 3 versions of that sentence") && r.system.contains("agreement"));
        assert!(r.user.contains("They repeat \"дом\"") && r.user.contains("The marked words: дом"));
        assert!(r.schema.unwrap().1.to_string().contains("\"versions\""));
    }

    #[test]
    fn bad_replies() {
        let (s, c) = doc("One.");
        assert!(matches!(parse(Pass::Tone, "I can't help", &s, &c), Err(AiError::BadReply(_))));
        assert!(parse(Pass::Tone, "{\"marks\": []}", &s, &c).unwrap().is_empty());
    }
}
