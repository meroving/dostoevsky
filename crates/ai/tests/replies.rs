//! Recorded model replies, messy as they come, checked against properties. No network.

use std::collections::HashSet;

use ai::lab::{self, Pass};
use ai::segment::{self, Sentence};
use ai::{ideas, trim};

fn doc() -> (String, Vec<char>, Vec<Sentence>) {
    let text = include_str!("fixtures/keep-the-words.md").to_string();
    let chars: Vec<char> = text.chars().collect();
    let s = segment::sentences(&text, 0..chars.len());
    (text, chars, s)
}

fn reply(name: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(include_str!("fixtures/replies.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

fn slice(chars: &[char], r: &std::ops::Range<usize>) -> String {
    chars[r.clone()].iter().collect()
}

#[test]
fn the_fixture_numbers_like_the_prompt() {
    let (_, _, s) = doc();
    assert_eq!(s[0].text, "Keep the Words You Almost Used");
    assert_eq!(s.len(), 12);
    assert_eq!(s[11].text, "It is, in a way, basically a kind of a one-way street, really.");
}

fn well_formed(marks: &[lab::Mark], chars: &[char]) {
    for w in marks.windows(2) {
        assert!(w[0].range.end <= w[1].range.start, "overlap or out of order: {:?}", w);
    }
    for m in marks {
        assert!(m.range.end <= chars.len() && !m.range.is_empty());
    }
}

#[test]
fn sentence_marks_anchor_through_a_code_fence() {
    let (_, chars, s) = doc();
    let m = lab::parse(Pass::HardToRead, &reply("weakest"), &s, &chars).unwrap();
    well_formed(&m, &chars);
    let got: Vec<String> = m.iter().map(|m| slice(&chars, &m.range)).collect();
    assert_eq!(got, ["Editors have always known this.", "It is, in a way, basically a kind of a one-way street, really."]);
}

#[test]
fn word_marks_anchor_or_drop() {
    let (_, chars, s) = doc();
    let m = lab::parse(Pass::Tone, &reply("hedges"), &s, &chars).unwrap();
    well_formed(&m, &chars);
    let got: Vec<String> = m.iter().map(|m| slice(&chars, &m.range)).collect();
    // "a kind of" was given sentence 11 but is in 12: found next door. "literally" isn't there.
    assert_eq!(got, ["quietly", "in a way", "basically", "a kind of", "really"]);
}

#[test]
fn typo_fixes_are_small_and_anchor_through_curly_quotes() {
    let (_, chars, s) = doc();
    let m = lab::parse(Pass::Mistakes, &reply("typos"), &s, &chars).unwrap();
    well_formed(&m, &chars);
    let got: Vec<(String, String)> = m.iter().map(|m| (slice(&chars, &m.range), m.fix.clone().unwrap())).collect();
    assert_eq!(got, [("teh".into(), "the".into()), ("can’t".into(), "cannot".into())], "the rewrite is refused");
}

#[test]
fn trims_reach_each_level_without_overshooting_by_more_than_one_cut() {
    let (_, chars, s) = doc();
    let cuts = trim::parse(&reply("trim"), &s, &chars).unwrap();
    // The repeated "quietly" is dropped as an overlap.
    let got: Vec<String> = cuts.iter().map(|c| slice(&chars, &c.range)).collect();
    assert_eq!(cuts.len(), 8, "{got:#?}");
    for w in cuts.iter().map(|c| slice(&chars, &c.range)) {
        assert!(w.ends_with(' ') || !w.contains("  "), "a cut takes one space with it: {w:?}");
    }
    let total = segment::word_count(&s);
    for pc in trim::LEVELS.iter().skip(1).copied().chain([5, 17, 35, 45]) {
        let picked = trim::pick(&cuts, total, pc, &HashSet::new());
        let cut = trim::words_cut(&cuts, &picked);
        let target = total * pc / 100;
        let last = picked.last().map(|&i| cuts[i].words).unwrap_or(0);
        let available: usize = cuts.iter().map(|c| c.words).sum();
        assert!(cut >= target.min(available), "−{pc}%: {cut} of {target}");
        assert!(cut < target + last.max(1), "−{pc}%: {cut} overshoots {target} by more than its last cut ({last})");
        let slack = (total * 2 / 100).max(2);
        assert!(cut <= target + slack || cut == available, "−{pc}%: {cut} words, the level is {target} ± {slack}");
    }
}

#[test]
fn ideas_from_a_chatty_reply() {
    let have = ["suggestion".to_string(), "note".to_string()];
    let mut lines = ideas::Lines::default();
    let mut got: Vec<String> = Vec::new();
    let text = reply("ideas");
    let pieces: Vec<String> = text.chars().collect::<Vec<_>>().chunks(5).map(|c| c.iter().collect()).collect();
    for p in pieces {
        for l in lines.feed(&p) {
            let known: Vec<String> = have.iter().chain(got.iter()).cloned().collect();
            if let Some(i) = ideas::clean(&l, ideas::Scope::Word, &known) {
                got.push(i);
            }
        }
    }
    assert_eq!(got, ["margin note", "aside", "gloss", "second thought", "counter-proposal"]);
    assert!(got.iter().all(|i| !i.is_empty() && !i.contains('\n')));
}
