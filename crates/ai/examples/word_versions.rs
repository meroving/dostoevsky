//! Word choice alternatives, the one-word ask (before) against the whole-sentence ask (now), on
//! echoes and off-tone words whose replacements change the words around them. A judge model
//! grades every version blind. It spends money, so nothing runs it automatically:
//!
//!   OPENROUTER_API_KEY=… cargo run -p ai --example word_versions
//!
//! DOSTOEVSKY_EVAL_CASES=2,13 runs only those cases (1-based); DOSTOEVSKY_EVAL_SHOW=1 prints the
//! replies and skips the judge.
//!
//! DOSTOEVSKY_EVAL_QUICK picks the model asked (Haiku by default), DOSTOEVSKY_EVAL_JUDGE the judge
//! (Sonnet), DOSTOEVSKY_EVAL_BUDGET the most it may spend in dollars (0.40), checked on the key.

use std::sync::atomic::AtomicBool;

use ai::client::chat;
use ai::lab::{self, Found, Pass, WordAsk, WordVersion};
use ai::{ideas, ChatRequest, Endpoint, ServiceKind};
use serde_json::Value;

struct Case {
    /// The paragraph.
    text: &'static str,
    /// The marked words and which of their occurrences (0-based).
    word: &'static str,
    nth: usize,
    /// An echo of this earlier word, or an off-tone word with this note.
    echo_of: Option<&'static str>,
    note: &'static str,
}

const CASES: &[Case] = &[
    Case { text: "Мы долго искали дом у моря. Наконец старый дом нашёлся на окраине, и дом оказался совсем пустым.", word: "дом", nth: 2, echo_of: Some("дом"), note: "" },
    Case { text: "Анна открыла книгу и сразу поняла, что книга была написана для детей.", word: "книга", nth: 0, echo_of: Some("книгу"), note: "" },
    Case { text: "В углу стоял огромный шкаф. Этот шкаф достался нам от бабушки, и по ночам шкаф тихо скрипел.", word: "шкаф", nth: 2, echo_of: Some("шкаф"), note: "" },
    Case { text: "Дорога шла через лес. Узкая дорога петляла между соснами, и вскоре дорога исчезла в тумане.", word: "дорога", nth: 1, echo_of: Some("дорога"), note: "" },
    Case { text: "Мама принесла новую лампу. Лампа светила тускло, но мама всё равно была довольна лампой.", word: "лампой", nth: 0, echo_of: Some("Лампа"), note: "" },
    Case { text: "Письмо пришло утром. Он долго держал письмо в руках, боясь открыть письмо.", word: "письмо", nth: 1, echo_of: Some("письмо"), note: "" },
    Case { text: "Учитель задал сложный вопрос. Никто в классе не знал ответа на этот вопрос, и учитель терпеливо повторил вопрос.", word: "вопрос", nth: 2, echo_of: Some("вопрос"), note: "" },
    Case { text: "She bought a small house near the river. The house was old, and the house needed a new roof before winter.", word: "house", nth: 2, echo_of: Some("house"), note: "" },
    Case { text: "The meeting was long. Everyone agreed the meeting had achieved nothing, although the meeting had run for three hours.", word: "meeting", nth: 2, echo_of: Some("meeting"), note: "" },
    Case { text: "You pick a word, the sentence moves on, and the other candidates quietly disappear. A week later you can’t remember those words, only that one of them was better.", word: "words", nth: 0, echo_of: Some("word"), note: "" },
    Case { text: "Wir kauften auf dem Markt einen alten Tisch. Der Tisch war schwer, und der Tisch passte kaum durch die Tür.", word: "Tisch", nth: 2, echo_of: Some("Tisch"), note: "" },
    Case { text: "Князь медленно поднялся по лестнице, и в его глазах читалась реально глубокая печаль.", word: "реально", nth: 0, echo_of: None, note: "too casual for this voice" },
    Case { text: "Старуха поставила на стол свой гаджет и долго смотрела на него, не говоря ни слова.", word: "гаджет", nth: 0, echo_of: None, note: "a modern word in a nineteenth-century voice" },
    Case { text: "The old duke rose slowly from his chair; his eyes betrayed a super deep sorrow he had never spoken of.", word: "super deep", nth: 0, echo_of: None, note: "too casual for this voice" },
];

const COUNT: usize = 3;

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// Dollars this key has spent, as OpenRouter counts them.
fn spent(ep: &Endpoint) -> Option<f64> {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    let mut r = agent.get("https://openrouter.ai/api/v1/key").header("Authorization", &format!("Bearer {}", ep.key.as_deref()?)).call().ok()?;
    let v: Value = serde_json::from_str(&r.body_mut().read_to_string().ok()?).ok()?;
    v["data"]["usage"].as_f64()
}

fn char_range(text: &str, word: &str, nth: usize) -> std::ops::Range<usize> {
    let (b, _) = text.match_indices(word).nth(nth).unwrap_or_else(|| panic!("no {nth}th {word} in {text}"));
    let s = text[..b].chars().count();
    s..s + word.chars().count()
}

fn splice(chars: &[char], r: &std::ops::Range<usize>, with: &str) -> String {
    let mut s: String = chars[..r.start].iter().collect();
    s.push_str(with);
    s.extend(&chars[r.end..]);
    s
}

/// The one-word ask as it was: `ideas::request` at word scope, lines cleaned as the app did.
fn before(ep: &Endpoint, model: &str, c: &Case, chars: &[char], mark: &std::ops::Range<usize>) -> Vec<String> {
    let note = match c.echo_of {
        Some(first) => format!("“{}” again, a few words after “{first}”.", c.word),
        None => c.note.to_string(),
    };
    let steer = match c.echo_of {
        Some(_) => format!("a different word, since the text has just used this one ({note})"),
        None => Pass::Tone.steer(&note, Found::Model),
    };
    let marked = ideas::mark(c.text, mark.start, mark.end);
    let existing = vec![c.word.to_string()];
    let ask = ideas::Ask { title: None, before: "", paragraph: &marked, after: "", current: c.word, scope: ideas::Scope::Word, existing: &existing, steer: Some(&steer), count: COUNT };
    let Ok(reply) = chat(ep, &ideas::request(model, &ask), &AtomicBool::new(false)) else { return Vec::new() };
    let (b, a): (String, String) = (chars[mark.start.saturating_sub(120)..mark.start].iter().collect(), chars[mark.end..].iter().collect());
    let mut have = existing.clone();
    for line in reply.lines() {
        if let Some(l) = ideas::unrepeat(line, &b, &a).and_then(|l| ideas::clean(&l, ideas::Scope::Word, &have)) {
            have.push(l);
        }
    }
    have[1..].iter().take(COUNT).map(|w| splice(chars, mark, w)).collect()
}

/// The whole-sentence ask, as the app sends it now (before the app's own echo check).
fn now(ep: &Endpoint, model: &str, c: &Case, chars: &[char], mark: &std::ops::Range<usize>) -> (Vec<String>, usize) {
    let sentences = ai::segment::sentences(c.text, 0..chars.len());
    let sentence = sentences.iter().find(|s| s.range.start <= mark.start && mark.start < s.range.end).map(|s| s.range.clone()).unwrap();
    let first_sentence = c.echo_of.and_then(|f| sentences.iter().find(|s| s.text.contains(f)).map(|s| s.text.clone()));
    let found = if c.echo_of.is_some() { Found::Echo } else { Found::Model };
    let why = Pass::Tone.steer(c.note, found);
    let marked = ideas::mark(c.text, mark.start, mark.end);
    let sentence_text: String = chars[sentence.clone()].iter().collect();
    let ask = WordAsk {
        title: None,
        before: "",
        paragraph: &marked,
        after: "",
        sentence: &sentence_text,
        current: c.word,
        why: &why,
        echo: c.echo_of.zip(first_sentence.as_deref()),
        count: COUNT,
    };
    let reply = match chat(ep, &lab::word_versions_request(model, &ask), &AtomicBool::new(false)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("   now: {e:?}");
            return (Vec::new(), 0);
        }
    };
    if env("DOSTOEVSKY_EVAL_SHOW").is_some() {
        eprintln!("   reply: {reply}");
    }
    // The echo's earlier word, when it's in the marked sentence: the nearest one before the mark.
    let first = c.echo_of.and_then(|f| {
        let before: String = chars[sentence.start..mark.start].iter().collect();
        let b = before.rfind(f)?;
        let s = sentence.start + before[..b].chars().count();
        Some(s..s + f.chars().count())
    });
    let versions: Vec<WordVersion> = lab::parse_word_versions(&reply, chars, sentence, mark.clone(), first).unwrap_or_default().into_iter().take(COUNT).collect();
    let wide = versions.iter().filter(|v| v.range != *mark).count();
    let Some((span, alts)) = lab::one_span(chars, &versions) else { return (Vec::new(), 0) };
    (alts.iter().map(|a| splice(chars, &span, a)).collect(), wide)
}

#[derive(Default)]
struct Tally {
    n: usize,
    grammatical: usize,
    solved: usize,
    natural: f64,
    meaning: usize,
    all_good: usize,
}

fn judge(ep: &Endpoint, model: &str, c: &Case, versions: &[String]) -> Vec<Value> {
    let problem = match c.echo_of {
        Some(first) => format!("the word “{}” repeats “{first}” used just before; a version should leave the passage without that repetition (and bring in no new one)", c.word),
        None => format!("the words “{}” jar: {}", c.word, c.note),
    };
    let mut user = format!("Original passage:\n{}\n\nThe problem: {problem}.\n\nVersions of the passage:\n", c.text);
    for (i, v) in versions.iter().enumerate() {
        user.push_str(&format!("[{}] {v}\n", i + 1));
    }
    let system = "You are a strict native-speaker editor grading versions of a passage. For each version answer: \
        grammatical (every word agrees in gender, number and case, articles and verb forms are right, nothing is missing or doubled), \
        solved (the problem is gone and no new repetition was brought in), natural (1-5: how well it reads to a native reader, 5 = as a good writer would put it), \
        meaning (the passage still says the same thing). Reply with JSON only: {\"grades\": [{\"id\": 1, \"grammatical\": true, \"solved\": true, \"natural\": 4, \"meaning\": true, \"why\": \"…\"}]}."
        .to_string();
    let req = ChatRequest { model: model.to_string(), system, user, schema: None, max_tokens: Some(1500) };
    match chat(ep, &req, &AtomicBool::new(false)) {
        Ok(r) => ai::json_object(&r).and_then(|v| v["grades"].as_array().cloned()).unwrap_or_default(),
        Err(e) => {
            eprintln!("judge failed: {e:?}");
            Vec::new()
        }
    }
}

fn main() {
    let key = env("OPENROUTER_API_KEY").unwrap_or_else(|| {
        eprintln!("Set OPENROUTER_API_KEY.");
        std::process::exit(2);
    });
    let ep = Endpoint { kind: ServiceKind::OpenRouter, base: ServiceKind::OpenRouter.default_base_url().to_string(), key: Some(key) };
    let quick = env("DOSTOEVSKY_EVAL_QUICK").unwrap_or_else(|| ai::service::HAIKU.into());
    let judge_model = env("DOSTOEVSKY_EVAL_JUDGE").unwrap_or_else(|| ai::service::SONNET.into());
    let budget: f64 = env("DOSTOEVSKY_EVAL_BUDGET").and_then(|b| b.parse().ok()).unwrap_or(0.40);
    let start = spent(&ep).unwrap_or_else(|| {
        eprintln!("Can't read the key's usage; not running without a budget check.");
        std::process::exit(2);
    });
    println!("asking {quick}, judged by {judge_model}; budget ${budget:.2}\n");
    let (mut old_t, mut new_t) = (Tally::default(), Tally::default());
    let mut widened = 0;
    // DOSTOEVSKY_EVAL_CASES=2,13 runs just those (1-based).
    let only: Option<Vec<usize>> = env("DOSTOEVSKY_EVAL_CASES").map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect());
    for (i, c) in CASES.iter().enumerate() {
        if only.as_ref().is_some_and(|o| !o.contains(&(i + 1))) {
            continue;
        }
        let so_far = spent(&ep).unwrap_or(f64::MAX) - start;
        if so_far > budget {
            println!("stopping: ${so_far:.3} spent");
            break;
        }
        let chars: Vec<char> = c.text.chars().collect();
        let mark = char_range(c.text, c.word, c.nth);
        let old = before(&ep, &quick, c, &chars, &mark);
        let (new, wide) = now(&ep, &quick, c, &chars, &mark);
        widened += wide;
        // Blind: the judge sees them interleaved, without saying which is which.
        let mut all: Vec<(bool, String)> = Vec::new();
        for k in 0..old.len().max(new.len()) {
            if let Some(v) = new.get(k) {
                all.push((true, v.clone()));
            }
            if let Some(v) = old.get(k) {
                all.push((false, v.clone()));
            }
        }
        if i % 2 == 1 {
            all.reverse();
        }
        let texts: Vec<String> = all.iter().map(|(_, t)| t.clone()).collect();
        // DOSTOEVSKY_EVAL_SHOW prints the raw replies and skips the judge.
        let grades = if env("DOSTOEVSKY_EVAL_SHOW").is_some() { Vec::new() } else { judge(&ep, &judge_model, c, &texts) };
        println!("── {} «{}»", i + 1, c.word);
        println!("   {}", c.text);
        for (j, (is_new, t)) in all.iter().enumerate() {
            let id = |g: &Value| g["id"].as_u64().or_else(|| g["id"].as_str().and_then(|s| s.trim_matches(|c| c == '[' || c == ']').parse().ok()));
            let g = grades.iter().find(|g| id(g) == Some(j as u64 + 1)).or_else(|| grades.get(j)).cloned().unwrap_or(Value::Null);
            let (gr, so, na, me) = (g["grammatical"].as_bool() == Some(true), g["solved"].as_bool() == Some(true), g["natural"].as_f64().unwrap_or(0.), g["meaning"].as_bool() == Some(true));
            let tally = if *is_new { &mut new_t } else { &mut old_t };
            tally.n += 1;
            tally.grammatical += gr as usize;
            tally.solved += so as usize;
            tally.natural += na;
            tally.meaning += me as usize;
            tally.all_good += (gr && so && me && na >= 4.) as usize;
            let flag = |b: bool, s: &str| if b { String::new() } else { format!(" ✗{s}") };
            println!(
                "   {} {}  [n{}{}{}{}] {}",
                if *is_new { "NOW" } else { "OLD" },
                t,
                na,
                flag(gr, "grammar"),
                flag(so, "unsolved"),
                flag(me, "meaning"),
                g["why"].as_str().unwrap_or("")
            );
        }
        println!();
    }
    let row = |name: &str, t: &Tally| {
        let pc = |x: usize| if t.n == 0 { 0. } else { 100. * x as f64 / t.n as f64 };
        println!(
            "{name}: {} versions · grammatical {:.0}% · solved {:.0}% · meaning kept {:.0}% · natural {:.2} · all good (and natural ≥ 4) {:.0}%",
            t.n,
            pc(t.grammatical),
            pc(t.solved),
            pc(t.meaning),
            if t.n == 0 { 0. } else { t.natural / t.n as f64 },
            pc(t.all_good)
        );
    };
    row("OLD", &old_t);
    row("NOW", &new_t);
    println!("NOW versions that took in words beside the mark: {widened}");
    println!("spent ${:.3}", spent(&ep).unwrap_or(start) - start);
}
