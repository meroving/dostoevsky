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
            Found::Echo => return format!("a different word, since the text has just used this one ({note})"),
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
                 For each, quote the smallest stretch that contains the mistake exactly as written (a word or a few words), \
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
    fn bad_replies() {
        let (s, c) = doc("One.");
        assert!(matches!(parse(Pass::Tone, "I can't help", &s, &c), Err(AiError::BadReply(_))));
        assert!(parse(Pass::Tone, "{\"marks\": []}", &s, &c).unwrap().is_empty());
    }
}
