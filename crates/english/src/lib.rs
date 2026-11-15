//! English-language helpers: word characters and word counts.

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '’' | '-')
}

pub fn word_count(s: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    let mut has_alnum = false;
    for c in s.chars() {
        if is_word_char(c) {
            in_word = true;
            has_alnum |= c.is_alphanumeric();
        } else {
            if in_word && has_alnum {
                count += 1;
            }
            in_word = false;
            has_alnum = false;
        }
    }
    if in_word && has_alnum {
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counting() {
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("Most of the friction"), 4);
        assert_eq!(word_count("can’t stop — won't stop"), 4);
        assert_eq!(word_count("well-known  - fact\n\nnext"), 3);
    }
}
