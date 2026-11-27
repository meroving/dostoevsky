//! Reading prose without a UI: the text's language, reading ease, passive sentences,
//! echoes, sentence bounds, and a word tagger for systems with none.

mod ease;
mod echo;
mod lang;
mod passive;
mod sentence;
mod tag;
mod token;

pub use ease::{band, ease, stats, syllables, Band, Stats, MIN_WORDS};
pub use echo::{echoes, plain_tokens, Echo, ECHO_WINDOW};
pub use lang::{detect, Lang};
pub use passive::{is_passive, reads_passive};
pub use sentence::{sentences, Sentence};
pub use tag::tag;
pub use token::{Class, Token};

/// `None` when the language isn't one the app knows.
pub fn reading(text: &str) -> Option<(Lang, Stats)> {
    let lang = detect(text)?;
    Some((lang, stats(text, lang)))
}
