use english::is_word_char;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Ru,
    De,
    Fr,
    Es,
    It,
    Pt,
}

impl Lang {
    pub const ALL: [Lang; 7] = [Lang::En, Lang::Ru, Lang::De, Lang::Fr, Lang::Es, Lang::It, Lang::Pt];

    /// ISO 639-1.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::Es => "es",
            Lang::It => "it",
            Lang::Pt => "pt",
        }
    }

    /// `word` must be lowercase.
    pub fn is_common(self, word: &str) -> bool {
        self.markers().contains(&word)
    }

    /// Each must be rare in the other languages' running text.
    fn markers(self) -> &'static [&'static str] {
        match self {
            Lang::En => &["the", "and", "of", "to", "is", "that", "it", "was", "for", "you", "he", "with", "on", "as", "his", "but", "they", "not", "this", "have"],
            Lang::Ru => &["и", "в", "не", "на", "я", "что", "он", "с", "как", "это", "по", "но", "она", "к", "его", "из", "у", "же", "так", "было"],
            Lang::De => &["der", "die", "und", "das", "ist", "nicht", "ich", "es", "zu", "ein", "eine", "den", "mit", "sich", "auf", "dem", "auch", "sie", "war", "wie"],
            Lang::Fr => &["le", "les", "et", "des", "est", "une", "du", "que", "qui", "dans", "pas", "pour", "ne", "ce", "il", "je", "sur", "au", "avec", "mais"],
            Lang::Es => &["el", "los", "las", "y", "que", "en", "una", "es", "por", "con", "no", "se", "del", "lo", "para", "su", "al", "como", "pero", "muy"],
            Lang::It => &["il", "di", "che", "e", "è", "un", "una", "per", "non", "del", "della", "con", "sono", "ma", "gli", "nel", "si", "lo", "anche", "come"],
            Lang::Pt => &["o", "os", "de", "que", "e", "do", "da", "em", "um", "uma", "não", "é", "com", "para", "se", "por", "mais", "dos", "como", "mas"],
        }
    }
}

const MIN_HITS: usize = 3;

/// `None` when too few marker words are found. Cyrillic text is always Russian.
pub fn detect(text: &str) -> Option<Lang> {
    let mut hits = [0usize; 7];
    let mut cyrillic = 0usize;
    let mut latin = 0usize;
    for word in text.split(|c: char| !is_word_char(c)).filter(|w| !w.is_empty()) {
        let w = word.to_lowercase();
        for (i, lang) in Lang::ALL.iter().enumerate() {
            if lang.markers().contains(&w.as_str()) {
                hits[i] += 1;
            }
        }
        match w.chars().next() {
            Some(c) if ('\u{400}'..='\u{4ff}').contains(&c) => cyrillic += 1,
            Some(c) if c.is_alphabetic() => latin += 1,
            _ => {}
        }
    }
    if cyrillic > latin {
        return (hits[1] >= MIN_HITS).then_some(Lang::Ru);
    }
    let (best, n) = Lang::ALL.iter().zip(hits).filter(|(l, _)| **l != Lang::Ru).max_by_key(|(_, n)| *n)?;
    (n >= MIN_HITS).then_some(*best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages() {
        assert_eq!(detect("Most of the friction in writing comes from choosing too early. You pick a word, and the sentence moves on."), Some(Lang::En));
        assert_eq!(detect("Я человек больной… Я злой человек. Непривлекательный я человек. Я думаю, что у меня болит печень."), Some(Lang::Ru));
        assert_eq!(detect("Die meisten Schwierigkeiten beim Schreiben kommen daher, dass man sich zu früh entscheidet und es nicht merkt."), Some(Lang::De));
        assert_eq!(detect("La plupart des difficultés de l’écriture viennent de ce que l’on choisit trop tôt, et il ne le sait pas."), Some(Lang::Fr));
        assert_eq!(detect("La mayor parte de la fricción en la escritura viene de elegir demasiado pronto, y no se nota para nada."), Some(Lang::Es));
        assert_eq!(detect("La maggior parte della fatica nella scrittura viene dal scegliere troppo presto, e non si vede per niente."), Some(Lang::It));
        assert_eq!(detect("A maior parte do atrito na escrita vem de escolher cedo demais, e não se percebe isso com o tempo."), Some(Lang::Pt));
    }

    #[test]
    fn too_little_to_say() {
        assert_eq!(detect(""), None);
        assert_eq!(detect("Keep the Words"), None);
        assert_eq!(detect("Записки"), None);
        assert_eq!(detect("12 345 — 678"), None);
    }
}
