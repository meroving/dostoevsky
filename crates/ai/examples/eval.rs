//! Runs the real prompts against a real service and reports what came back.
//! It spends money, so nothing runs it automatically.

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use ai::client::{chat, chat_stream, list_models, ping};
use ai::lab::{self, Pass};
use ai::{ideas, segment, trim, Endpoint, ServiceKind};

const FIRST_IDEA_BUDGET: Duration = Duration::from_millis(1500);

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn main() {
    // DOSTOEVSKY_EVAL_KIND picks the service by its slug (openrouter, openai, gemini, mistral, groq,
    // ollama, lmstudio, custom); without it the address decides: OpenRouter's, or Custom for any other.
    let kind = env("DOSTOEVSKY_EVAL_KIND").map(|k| {
        ServiceKind::ALL.into_iter().find(|s| s.slug() == k).unwrap_or_else(|| {
            let all: Vec<&str> = ServiceKind::ALL.iter().map(|k| k.slug()).collect();
            eprintln!("DOSTOEVSKY_EVAL_KIND: one of {}.", all.join(", "));
            std::process::exit(2);
        })
    });
    let base = env("DOSTOEVSKY_EVAL_BASE")
        .or_else(|| kind.map(|k| k.default_base_url().to_string()).filter(|b| !b.is_empty()))
        .unwrap_or_else(|| ServiceKind::OpenRouter.default_base_url().to_string());
    let kind = kind.unwrap_or(if base.contains("openrouter.ai") { ServiceKind::OpenRouter } else { ServiceKind::Custom });
    let key = env("DOSTOEVSKY_EVAL_KEY").or_else(|| match kind {
        ServiceKind::OpenRouter => env("OPENROUTER_API_KEY"),
        ServiceKind::OpenAi => env("OPENAI_API_KEY"),
        ServiceKind::Gemini => env("GEMINI_API_KEY"),
        ServiceKind::Mistral => env("MISTRAL_API_KEY"),
        ServiceKind::Groq => env("GROQ_API_KEY"),
        _ => None,
    });
    if kind.needs_key() && key.is_none() {
        eprintln!("Set OPENROUTER_API_KEY (or DOSTOEVSKY_EVAL_KIND=openai|gemini|mistral|groq and its *_API_KEY, or DOSTOEVSKY_EVAL_BASE for a local server).");
        std::process::exit(2);
    }
    let ep = Endpoint { kind, base, key };
    println!("{} at {}", kind.label(), ep.base);
    let listed = list_models(&ep);
    // The app's own picks for the service, as Preferences would fill them.
    let (q, d) = match &listed {
        Ok(m) if kind != ServiceKind::OpenRouter => ai::service::default_models(kind, m),
        _ => (Some(ai::service::SONNET.into()), Some(ai::service::OPUS.into())),
    };
    let quick = env("DOSTOEVSKY_EVAL_QUICK").or(q).unwrap_or_else(|| ai::service::SONNET.into());
    let deep = env("DOSTOEVSKY_EVAL_DEEP").or(d).unwrap_or_else(|| quick.clone());
    match listed {
        Ok(m) => {
            let has = |id: &str| if m.iter().any(|x| x.id == id) { "listed" } else { "NOT LISTED" };
            println!("models  {} (quick {}, deep {})", m.len(), has(&quick), has(&deep));
        }
        Err(e) => println!("models  failed: {}", e.message(kind)),
    }
    match ping(&ep, &quick) {
        Ok(d) => println!("ping    {:.2} s", d.as_secs_f32()),
        Err(e) => println!("ping    failed: {}", e.message(kind)),
    }

    let cancel = AtomicBool::new(false);
    let text = include_str!("../tests/fixtures/keep-the-words.md");
    let chars: Vec<char> = text.chars().collect();
    let sentences = segment::sentences(text, 0..chars.len());
    let mut ok = true;

    println!("ideas  ({quick})");
    let word = "suggestion";
    let para = text.lines().find(|l| l.contains(word)).unwrap();
    let at = para.find(word).unwrap();
    let start = para[..at].chars().count();
    let marked = ideas::mark(para, start, start + word.chars().count());
    let existing = vec![word.to_string(), "note".to_string()];
    let ask = ideas::Ask { title: Some("Keep the Words You Almost Used"), before: "", paragraph: &marked, after: "", current: word, scope: ideas::Scope::Word, existing: &existing, steer: None, count: 5 };
    let t = Instant::now();
    let mut first = None;
    let mut lines = ideas::Lines::default();
    let mut got: Vec<String> = Vec::new();
    let r = chat_stream(&ep, &ideas::request(&quick, &ask), &cancel, |p| {
        for l in lines.feed(p) {
            let known: Vec<String> = existing.iter().chain(got.iter()).cloned().collect();
            if let Some(i) = ideas::clean(&l, ideas::Scope::Word, &known) {
                first.get_or_insert(t.elapsed());
                got.push(i);
            }
        }
    });
    if let Some(l) = lines.finish() {
        let known: Vec<String> = existing.iter().chain(got.iter()).cloned().collect();
        got.extend(ideas::clean(&l, ideas::Scope::Word, &known));
    }
    match r {
        Ok(_) => {
            let first = first.unwrap_or(t.elapsed());
            let within = first <= FIRST_IDEA_BUDGET;
            ok &= within;
            println!("  first idea {:.2} s ({}), all {:.2} s", first.as_secs_f32(), if within { "within budget" } else { "OVER BUDGET" }, t.elapsed().as_secs_f32());
            println!("  {} ideas: {}", got.len(), got.join(" · "));
            let fits = got.iter().filter(|i| english::word_count(i) <= 4).count();
            println!("  {fits}/{} fit a word slot", got.len());
        }
        Err(e) => {
            ok = false;
            println!("  failed: {}", e.message(kind));
        }
    }

    for pass in Pass::ALL {
        let t = Instant::now();
        match chat(&ep, &lab::request(pass, &deep, Some("Keep the Words You Almost Used"), &sentences), &cancel) {
            Ok(reply) => {
                let raw = ai::json_object(&reply).and_then(|v| v["marks"].as_array().map(|a| a.len())).unwrap_or(0);
                match lab::parse(pass, &reply, &sentences, &chars) {
                    Ok(m) => {
                        println!("{:<30} {:>5.1} s  {}/{} anchored", pass.menu_label(), t.elapsed().as_secs_f32(), m.len(), raw);
                        for k in m.iter().take(4) {
                            let q: String = chars[k.range.clone()].iter().collect();
                            println!("    “{}” — {}{}", q.chars().take(60).collect::<String>(), k.note, k.fix.as_ref().map(|f| format!(" → {f}")).unwrap_or_default());
                        }
                    }
                    Err(e) => println!("{:<30} unusable reply: {e:?}", pass.menu_label()),
                }
            }
            Err(e) => {
                ok = false;
                println!("{:<30} failed: {}", pass.menu_label(), e.message(kind));
            }
        }
    }

    let t = Instant::now();
    match chat(&ep, &trim::request(&deep, None, &sentences), &cancel).and_then(|r| trim::parse(&r, &sentences, &chars)) {
        Ok(cuts) => {
            let total = segment::word_count(&sentences);
            let avail: usize = cuts.iter().map(|c| c.words).sum();
            println!("trim  {:.1} s  {} cuts, {avail} of {total} words available", t.elapsed().as_secs_f32(), cuts.len());
            for pc in trim::LEVELS.iter().skip(1) {
                let picked = trim::pick(&cuts, total, *pc, &HashSet::new());
                let cut = trim::words_cut(&cuts, &picked);
                println!("  −{pc:>2}%  {cut} words ({:.0}%)", cut as f32 * 100. / total as f32);
            }
        }
        Err(e) => {
            ok = false;
            println!("trim  failed: {}", e.message(kind));
        }
    }

    if !ok && env("DOSTOEVSKY_EVAL_STRICT").is_some() {
        std::process::exit(1);
    }
}
