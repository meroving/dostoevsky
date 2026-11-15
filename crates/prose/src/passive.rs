use crate::token::{Class, Token};
use crate::Lang;

pub fn reads_passive(lang: Lang) -> bool {
    matches!(lang, Lang::En | Lang::Ru | Lang::De | Lang::Es)
}

pub fn is_passive(tokens: &[Token], lang: Lang) -> bool {
    match lang {
        Lang::En => english(tokens),
        Lang::Ru => russian(tokens),
        Lang::De => german(tokens),
        Lang::Es => spanish(tokens),
        Lang::Fr | Lang::It | Lang::Pt => false,
    }
}

const EN_IRREGULAR: &[&str] = &[
    "arisen", "awoken", "beaten", "become", "begun", "bent", "bitten", "blown", "born", "borne", "bought", "bound", "bred", "broken", "brought", "built", "burnt", "caught", "chosen", "come", "cut", "dealt", "done",
    "drawn", "driven", "dug", "eaten", "fallen", "fed", "felt", "fled", "flown", "forbidden", "forgiven", "forgotten", "fought", "found", "frozen", "given", "ground", "grown", "heard", "held", "hidden", "hit", "hung", "hurt",
    "kept", "known", "laid", "led", "left", "lent", "lit", "lost", "made", "meant", "met", "mistaken", "paid", "put", "read", "ridden", "risen", "run", "said", "seen", "sent", "set", "shaken", "shot", "shown", "shut", "sold",
    "sought", "spent", "split", "spoken", "spread", "stolen", "struck", "stuck", "sung", "swept", "sworn", "taken", "taught", "thought", "thrown", "told", "torn", "understood", "woken", "won", "worn", "woven", "written",
];

fn english(tokens: &[Token]) -> bool {
    for (i, aux) in tokens.iter().enumerate() {
        if aux.base() != "be" {
            continue;
        }
        for t in tokens.iter().skip(i + 1).take(4) {
            let w = t.word.to_lowercase();
            if t.class == Class::Verb && t.base() != "be" {
                let participle = (w.ends_with("ed") && t.inflected()) || EN_IRREGULAR.contains(&w.as_str());
                if participle {
                    return true;
                }
                break;
            }
            if !(t.class == Class::Adverb || t.class == Class::Particle || t.base() == "be" || w == "n't" || w == "not") {
                break;
            }
        }
    }
    false
}

/// Short passive participles; a bare -т only after *быть*, as it is also a present-tense ending.
fn russian(tokens: &[Token]) -> bool {
    let ends = |w: &str, stems: &[&str], vowel: bool| {
        stems.iter().any(|s| {
            w.ends_with(s) && !vowel || ["а", "о", "ы"].iter().any(|v| w.ends_with(&format!("{s}{v}")))
        })
    };
    for (i, t) in tokens.iter().enumerate() {
        if t.class != Class::Verb || !t.inflected() {
            continue;
        }
        let w = t.word.to_lowercase();
        if ends(&w, &["ан", "ян", "ен", "ён"], false) || ends(&w, &["ыт", "ит", "ут", "ят", "от", "ёрт", "ерт"], true) {
            return true;
        }
        let after_byt = tokens[i.saturating_sub(2)..i].iter().any(|b| b.base() == "быть");
        if after_byt && ["ыт", "ит", "ут", "ят", "от", "ёрт", "ерт"].iter().any(|s| w.ends_with(s)) {
            return true;
        }
    }
    false
}

/// German: *werden* and a Partizip II anywhere after it (it often ends the clause).
fn german(tokens: &[Token]) -> bool {
    let Some(i) = tokens.iter().position(|t| t.base() == "werden") else { return false };
    tokens[i + 1..].iter().any(|t| {
        let w = t.word.to_lowercase();
        let ge = w.chars().take(8).collect::<String>().contains("ge") && (w.ends_with('t') || w.ends_with("en"));
        let inseparable = ["be", "ver", "er", "ent", "zer", "emp", "miss"].iter().any(|p| w.starts_with(p)) && w.ends_with('t');
        t.class == Class::Verb && t.inflected() && t.base() != "werden" && (ge || inseparable)
    })
}

const ES_IRREGULAR: &[&str] = &["abierto", "cubierto", "descubierto", "dicho", "escrito", "frito", "hecho", "impreso", "muerto", "puesto", "resuelto", "roto", "visto", "vuelto"];

fn spanish(tokens: &[Token]) -> bool {
    for (i, aux) in tokens.iter().enumerate() {
        if aux.base() != "ser" {
            continue;
        }
        for t in tokens.iter().skip(i + 1).take(3) {
            let w = t.word.to_lowercase();
            let stem = w.trim_end_matches('s').trim_end_matches(['o', 'a']);
            let participle = ["ad", "id"].iter().any(|e| stem.ends_with(e)) || ES_IRREGULAR.iter().any(|p| p.trim_end_matches('o') == stem);
            if t.class == Class::Verb && participle {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::{t, Class::*};

    fn sentence(words: &[(&str, crate::token::Class, &str)]) -> Vec<Token> {
        let mut at = 0;
        words
            .iter()
            .map(|(w, c, l)| {
                let tok = t(at, w, *c, Some(l));
                at += w.chars().count() + 1;
                tok
            })
            .collect()
    }

    #[test]
    fn english_passive() {
        let erased = sentence(&[("Your", Determiner, "your"), ("sentence", Noun, "sentence"), ("is", Verb, "be"), ("n't", Adverb, "not"), ("erased", Verb, "erase"), ("by", Preposition, "by"), ("a", Determiner, "a"), ("good", Adjective, "good"), ("editor", Noun, "editor")]);
        assert!(is_passive(&erased, Lang::En));
        let written = sentence(&[("It", Pronoun, "it"), ("was", Verb, "be"), ("never", Adverb, "never"), ("written", Verb, "write")]);
        assert!(is_passive(&written, Lang::En));
        let been = sentence(&[("It", Pronoun, "it"), ("has", Verb, "have"), ("been", Verb, "be"), ("sold", Verb, "sell")]);
        assert!(is_passive(&been, Lang::En));
        let tired = sentence(&[("I", Pronoun, "i"), ("was", Verb, "be"), ("tired", Adjective, "tired")]);
        assert!(!is_passive(&tired, Lang::En), "a participle used as an adjective");
        let active = sentence(&[("A", Determiner, "a"), ("good", Adjective, "good"), ("editor", Noun, "editor"), ("erased", Verb, "erase"), ("it", Pronoun, "it")]);
        assert!(!is_passive(&active, Lang::En));
        let going = sentence(&[("She", Pronoun, "she"), ("was", Verb, "be"), ("walking", Verb, "walk")]);
        assert!(!is_passive(&going, Lang::En));
    }

    #[test]
    fn russian_passive() {
        let letter = sentence(&[("Письмо", Noun, "письмо"), ("было", Verb, "быть"), ("написано", Verb, "написать"), ("вчера", Adverb, "вчера")]);
        assert!(is_passive(&letter, Lang::Ru));
        let open = sentence(&[("Окно", Noun, "окно"), ("открыто", Verb, "открыть")]);
        assert!(is_passive(&open, Lang::Ru));
        let taken = sentence(&[("Он", Pronoun, "он"), ("был", Verb, "быть"), ("взят", Verb, "взять")]);
        assert!(is_passive(&taken, Lang::Ru));
        let they_see = sentence(&[("Они", Pronoun, "они"), ("видят", Verb, "видеть"), ("море", Noun, "море")]);
        assert!(!is_passive(&they_see, Lang::Ru), "a present-tense -ят");
        let wrote = sentence(&[("Я", Pronoun, "я"), ("написал", Verb, "написать"), ("письмо", Noun, "письмо")]);
        assert!(!is_passive(&wrote, Lang::Ru));
    }

    #[test]
    fn german_passive() {
        let sold = sentence(&[("Das", Determiner, "das"), ("Haus", Noun, "haus"), ("wurde", Verb, "werden"), ("gestern", Adverb, "gestern"), ("verkauft", Verb, "verkaufen")]);
        assert!(is_passive(&sold, Lang::De));
        let built = sentence(&[("Es", Pronoun, "es"), ("wird", Verb, "werden"), ("gebaut", Verb, "bauen")]);
        assert!(is_passive(&built, Lang::De));
        let future = sentence(&[("Er", Pronoun, "er"), ("wird", Verb, "werden"), ("kommen", Verb, "kommen")]);
        assert!(!is_passive(&future, Lang::De), "werden + infinitive is the future");
    }

    #[test]
    fn spanish_passive() {
        let written = sentence(&[("La", Determiner, "el"), ("carta", Noun, "carta"), ("fue", Verb, "ser"), ("escrita", Verb, "escribir")]);
        assert!(is_passive(&written, Lang::Es));
        let sold = sentence(&[("Las", Determiner, "el"), ("casas", Noun, "casa"), ("fueron", Verb, "ser"), ("vendidas", Verb, "vender")]);
        assert!(is_passive(&sold, Lang::Es));
        let important = sentence(&[("Es", Verb, "ser"), ("importante", Adjective, "importante")]);
        assert!(!is_passive(&important, Lang::Es));
    }

    #[test]
    fn other_languages_never() {
        let est = sentence(&[("Il", Pronoun, "il"), ("est", Verb, "être"), ("allé", Verb, "aller")]);
        assert!(!is_passive(&est, Lang::Fr));
        assert!(!reads_passive(Lang::Fr) && !reads_passive(Lang::It) && !reads_passive(Lang::Pt));
    }
}
