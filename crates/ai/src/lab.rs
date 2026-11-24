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
/// beside it, a noun its article. So the model says which stretch of the sentence each version
/// replaces, as few words as it needs.
pub fn word_versions_request(model: &str, a: &WordAsk) -> ChatRequest {
    let n = a.count;
    let system = format!(
        "You help a writer replace marked words in their draft. The marked words are ⟦…⟧ in the text; the sentence they are in follows it. \
         Suggest {n} replacements. Each is a stretch of that sentence copied exactly as written (\"quote\", taking in all of the marked words) \
         and the text that takes its place (\"text\"). Usually the quote is just the marked words. When a replacement only reads right if words beside it \
         change too (gender, number or case agreement, an article, a preposition, a verb form, the word order), the quote takes in those words \
         and the text gives them changed: as few words as that needs, never beyond the sentence. \
         With the text in place of the quote the sentence must read correctly and naturally, so check every word that stays against the new ones. \
         Keep the writer's meaning, voice, tense and register, and take the surrounding paragraphs into account: you are not a thesaurus. \
         Suggest different directions, not small variations of one idea, and never the words as they stand. \
         One text may be empty, when the sentence reads better with the quoted words simply gone. \
         Write in the language of the text, whatever it is. \
         Reply with JSON only, shaped like {{\"versions\": [{{\"quote\": \"…\", \"text\": \"…\"}}]}}."
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
            "They repeat \"{first}\", used just before (in: {}). Every replacement must leave the text without that repetition: \
             another word, a pronoun, the word gone, or the sentence turned around it; if both are in this sentence, the quote may take in the earlier one instead.\n",
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
            "properties": {
                "quote": {"type": "string", "description": "Copied exactly from the sentence, taking in the marked words."},
                "text": {"type": "string", "description": "What takes the quote's place."}
            },
            "required": ["quote", "text"],
            "additionalProperties": false
        }}},
        "required": ["versions"],
        "additionalProperties": false
    });
    ChatRequest { model: model.to_string(), system, user, schema: Some(("versions".into(), schema)), max_tokens: Some((150 * n.max(3) + 200) as u32) }
}

/// Versions whose quote takes in `mark` inside `sentence` (display chars in `text`), widened to
/// whole words; a version gone takes a space along. Repeats of the words as they stand, of one
/// another, and of the words either side of the quote are dropped.
pub fn parse_word_versions(reply: &str, text: &[char], sentence: Range<usize>, mark: Range<usize>) -> Result<Vec<WordVersion>, AiError> {
    let v = crate::json_object(reply).ok_or_else(|| AiError::BadReply("no JSON".into()))?;
    let hay: String = text.get(sentence.clone()).unwrap_or_default().iter().collect();
    let inner = mark.start.saturating_sub(sentence.start)..mark.end.saturating_sub(sentence.start);
    let mut out: Vec<WordVersion> = Vec::new();
    for it in v["versions"].as_array().cloned().unwrap_or_default() {
        let Some(r) = anchor::find_covering(&hay, it["quote"].as_str().unwrap_or(""), inner.clone()) else { continue };
        let core = anchor::to_words(text, sentence.start + r.start..sentence.start + r.end);
        let said = it["text"].as_str().unwrap_or("").trim();
        let (range, version) = if said.is_empty() {
            (anchor::with_space(text, core), String::new())
        } else {
            let before: String = text[sentence.start.min(core.start)..core.start].iter().collect();
            let after: String = text[core.end..sentence.end.max(core.end)].iter().collect();
            match crate::ideas::unrepeat(said, &before, &after) {
                Some(t) => (core, t),
                None => continue,
            }
        };
        let was: String = text[range.clone()].iter().collect();
        if version.trim() == was.trim() || out.iter().any(|o| spliced(text, o) == spliced(text, &WordVersion { range: range.clone(), text: version.clone() })) {
            continue;
        }
        out.push(WordVersion { range, text: version });
    }
    Ok(out)
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
    fn word_versions_take_in_the_words_they_change() {
        // «дом» → «здание» changes «старый» and «стоял» too.
        let t = "Мы жили в доме. Старый дом стоял у реки, и дом был пуст.";
        let c: Vec<char> = t.chars().collect();
        let sentence = 16..t.chars().count();
        let mark = 43..46;
        assert_eq!(c[mark.clone()].iter().collect::<String>(), "дом");
        let reply = r#"{"versions":[
            {"quote":"дом был пуст","text":"он был пуст"},
            {"quote":"дом","text":"и здание"},
            {"quote":"дом","text":"особняк"},
            {"quote":"Старый дом","text":"Старое здание"},
            {"quote":"дом","text":"дом"},
            {"quote":"и дом был","text":"и он был"},
            {"quote":"не в тексте","text":"x"},
            {"quote":"дом","text":""}
        ]}"#;
        let v = parse_word_versions(reply, &c, sentence, mark).unwrap();
        let texts: Vec<&str> = v.iter().map(|v| v.text.as_str()).collect();
        assert_eq!(texts, ["он был пуст", "здание", "особняк", ""], "the first «дом» isn't the marked one; «и» came along and goes; as written; repeats; nowhere");
        assert_eq!(c[v[0].range.clone()].iter().collect::<String>(), "дом был пуст");
        assert_eq!(c[v[3].range.clone()].iter().collect::<String>(), "дом ", "gone with its space");
        let (span, alts) = one_span(&c, &v).unwrap();
        assert_eq!(c[span].iter().collect::<String>(), "дом был пуст");
        assert_eq!(alts, ["он был пуст", "здание был пуст", "особняк был пуст", "был пуст"]);
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
        assert!(r.system.contains("Suggest 3 replacements") && r.system.contains("agreement"));
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
