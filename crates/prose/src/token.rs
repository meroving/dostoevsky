use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Noun,
    Verb,
    Adjective,
    Adverb,
    Pronoun,
    Determiner,
    Particle,
    Preposition,
    Number,
    Conjunction,
    Interjection,
    Other,
}

impl Class {
    pub fn from_tag(tag: &str) -> Class {
        match tag {
            "Noun" => Class::Noun,
            "Verb" => Class::Verb,
            "Adjective" => Class::Adjective,
            "Adverb" => Class::Adverb,
            "Pronoun" => Class::Pronoun,
            "Determiner" => Class::Determiner,
            "Particle" => Class::Particle,
            "Preposition" => Class::Preposition,
            "Number" => Class::Number,
            "Conjunction" => Class::Conjunction,
            "Interjection" => Class::Interjection,
            _ => Class::Other,
        }
    }

    pub fn is_function_word(self) -> bool {
        matches!(self, Class::Pronoun | Class::Determiner | Class::Particle | Class::Preposition | Class::Conjunction | Class::Interjection | Class::Number)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// Display chars in the text that was tagged.
    pub range: Range<usize>,
    pub word: String,
    pub class: Class,
    /// Lowercase.
    pub lemma: Option<String>,
}

impl Token {
    pub fn base(&self) -> String {
        self.lemma.clone().unwrap_or_else(|| self.word.to_lowercase())
    }

    pub fn inflected(&self) -> bool {
        self.lemma.as_deref().is_some_and(|l| l != self.word.to_lowercase())
    }
}

#[cfg(test)]
pub(crate) fn t(start: usize, word: &str, class: Class, lemma: Option<&str>) -> Token {
    Token { range: start..start + word.chars().count(), word: word.into(), class, lemma: lemma.map(str::to_string) }
}
