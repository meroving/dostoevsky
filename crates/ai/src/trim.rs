use std::collections::HashSet;
use std::ops::Range;

use serde_json::{json, Value};

use crate::anchor;
use crate::client::{AiError, ChatRequest};
use crate::segment::{numbered, word_count, Sentence};

/// Percent of words cut; 0 is as written.
pub const LEVELS: [usize; 5] = [0, 10, 20, 30, 50];

/// Percent; the request asks the model for a little more.
pub const MAX: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    /// Display chars: whole words plus one space.
    pub range: Range<usize>,
    pub words: usize,
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"cuts": {"type": "array", "items": {
            "type": "object",
            "properties": {
                "sentence": {"type": "integer"},
                "quote": {"type": "string"}
            },
            "required": ["sentence", "quote"],
            "additionalProperties": false
        }}},
        "required": ["cuts"],
        "additionalProperties": false
    })
}

pub fn request(model: &str, title: Option<&str>, sentences: &[Sentence]) -> ChatRequest {
    let total = word_count(sentences);
    let want = total * 55 / 100;
    let system = format!(
        "You are an editor tightening a writer's draft by deletion only. The draft comes as numbered sentences, [n] before each. \
         List pieces of text that can be deleted: whole sentences, clauses, phrases or single words. \
         After deleting any of them the text must still read correctly, keep its meaning and the writer's voice; \
         never list a piece whose removal breaks grammar or needs other words changed. \
         Quote each piece exactly as written, inside one sentence; pieces must not overlap. \
         Order them from the easiest to lose to the hardest: first hedges and filler (basically, really, just, very, in a way, \
         a kind of, I think, it seems), then repetition and sentences that only say what another one says or say nothing, \
         then whatever else the text can do without. Keep going until the pieces add up to \
         at least {want} of the {total} words. Never rewrite. \
         Reply with JSON only, shaped like {{\"cuts\": [{{\"sentence\": 4, \"quote\": \"…\"}}]}}."
    );
    let mut user = String::new();
    if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
        user.push_str(&format!("Title: {}\n\n", t.trim()));
    }
    user.push_str(&numbered(sentences));
    ChatRequest { model: model.to_string(), system, user, schema: Some(("cuts".into(), schema())), max_tokens: Some(16000) }
}

/// In rank order; quotes that don't anchor or overlap an earlier cut are dropped.
pub fn parse(reply: &str, sentences: &[Sentence], text: &[char]) -> Result<Vec<Cut>, AiError> {
    let v = crate::json_object(reply).ok_or_else(|| AiError::BadReply("no JSON".into()))?;
    let mut out: Vec<Cut> = Vec::new();
    for it in v["cuts"].as_array().cloned().unwrap_or_default() {
        let n = it["sentence"].as_i64().unwrap_or(0);
        let Some(r) = anchor::near_sentence(sentences, n, it["quote"].as_str().unwrap_or("")) else { continue };
        let core = anchor::to_words(text, r);
        let overlaps = |r: &Range<usize>| out.iter().any(|c| c.range.start < r.end && r.start < c.range.end);
        if overlaps(&core) {
            continue;
        }
        // Take one space along, unless a neighbouring cut already took it.
        let wide = anchor::with_space(text, core.clone());
        let r = if overlaps(&wide) { core } else { wide };
        let piece: String = text[r.clone()].iter().collect();
        let words = english::word_count(&piece);
        if words == 0 {
            continue;
        }
        out.push(Cut { range: r, words });
    }
    Ok(out)
}

/// Indices into `cuts` (rank order) that make `percent` of `total` words within ±2%, skipping
/// `kept`; else rank order plus the smallest cut that reaches the level.
pub fn pick(cuts: &[Cut], total: usize, percent: usize, kept: &HashSet<usize>) -> Vec<usize> {
    let target = total * percent / 100;
    if target == 0 {
        return Vec::new();
    }
    let slack = (total * 2 / 100).max(2);
    within(cuts, target, target + slack, kept).unwrap_or_else(|| greedy(cuts, target, slack, kept))
}

fn within(cuts: &[Cut], lo: usize, hi: usize, kept: &HashSet<usize>) -> Option<Vec<usize>> {
    let items: Vec<usize> = (0..cuts.len()).filter(|i| !kept.contains(i) && cuts[*i].words > 0 && cuts[*i].words <= hi).collect();
    if items.len() > 400 || hi > 50_000 {
        return None;
    }
    // reach[k][t]: some subset of items[k..] adds up to exactly t words.
    let mut reach = vec![vec![false; hi + 1]; items.len() + 1];
    reach[items.len()][0] = true;
    for k in (0..items.len()).rev() {
        let w = cuts[items[k]].words;
        let (head, tail) = reach.split_at_mut(k + 1);
        let (row, next) = (&mut head[k], &tail[0]);
        row.copy_from_slice(next);
        for t in w..=hi {
            if next[t - w] {
                row[t] = true;
            }
        }
    }
    let finishes = |k: usize, sum: usize| (lo.saturating_sub(sum)..=hi.saturating_sub(sum)).any(|t| sum + t >= lo && reach[k][t]);
    if !finishes(0, 0) {
        return None;
    }
    let mut out = Vec::new();
    let mut sum = 0;
    for (k, &i) in items.iter().enumerate() {
        if sum >= lo {
            break;
        }
        let w = cuts[i].words;
        if sum + w <= hi && finishes(k + 1, sum + w) {
            out.push(i);
            sum += w;
        }
    }
    (sum >= lo && sum <= hi).then_some(out)
}

fn greedy(cuts: &[Cut], target: usize, slack: usize, kept: &HashSet<usize>) -> Vec<usize> {
    let mut words = 0;
    let mut out = Vec::new();
    for (i, c) in cuts.iter().enumerate() {
        if words >= target {
            break;
        }
        if kept.contains(&i) || words + c.words > target + slack {
            continue;
        }
        out.push(i);
        words += c.words;
    }
    if words < target {
        let left = |i: &usize| !kept.contains(i) && !out.contains(i);
        let reach = (0..cuts.len()).filter(left).filter(|&i| words + cuts[i].words >= target).min_by_key(|&i| cuts[i].words);
        match reach {
            Some(i) => out.push(i),
            None => {
                for i in (0..cuts.len()).filter(left).collect::<Vec<_>>() {
                    if words >= target {
                        break;
                    }
                    out.push(i);
                    words += cuts[i].words;
                }
            }
        }
    }
    out
}

pub fn words_cut(cuts: &[Cut], picked: &[usize]) -> usize {
    picked.iter().map(|&i| cuts[i].words).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::sentences;

    #[test]
    fn levels_pick_in_rank_order_and_skip_kept() {
        let t = "Most of the friction comes from choosing too early. The other candidates quietly disappear. It is basically a street.";
        let s = sentences(t, 0..t.len());
        let c: Vec<char> = t.chars().collect();
        let reply = r#"{"cuts":[{"sentence":2,"quote":"quietly"},{"sentence":3,"quote":"basically"},{"sentence":1,"quote":"Most of the"},{"sentence":9,"quote":"nope"}]}"#;
        let cuts = parse(reply, &s, &c).unwrap();
        assert_eq!(cuts.len(), 3);
        assert_eq!(c[cuts[0].range.clone()].iter().collect::<String>(), "quietly ");
        let total = word_count(&s);
        assert_eq!(total, 19);
        assert_eq!(pick(&cuts, total, 0, &HashSet::new()), Vec::<usize>::new());
        assert_eq!(pick(&cuts, total, 10, &HashSet::new()), [0]);
        assert_eq!(pick(&cuts, total, 20, &HashSet::new()), [0, 1, 2]);
        let kept: HashSet<usize> = [0].into();
        assert_eq!(pick(&cuts, total, 10, &kept), [1]);
        assert_eq!(words_cut(&cuts, &[1, 2]), 4);
    }

    #[test]
    fn the_request_asks_for_more_than_half() {
        let t = "One two three four. Five six seven eight nine ten.";
        let s = sentences(t, 0..t.len());
        let r = request("m", None, &s);
        assert!(r.system.contains("at least 5 of the 10 words"));
        assert!(r.user.starts_with("[1] One two three four."));
    }
}
