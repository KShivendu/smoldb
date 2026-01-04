use ahash::HashMap;

// TODO: Add stop words, stemming, etc based on language
/// Tokenize the text into terms
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn tokenize_and_count_frequencies(text: &str) -> HashMap<String, u64> {
    let mut with_freq: HashMap<String, u64> = HashMap::default();
    for term in tokenize(text) {
        *with_freq.entry(term).or_insert(0) += 1;
    }
    with_freq
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenizer() {
        assert_eq!(
            tokenize("Hello, world! 123. a1b2c3 hello 123-456-7890"),
            ["hello", "world", "123", "a1b2c3", "hello", "123-456-7890"]
        );

        let freq = tokenize_and_count_frequencies("Hello, world! 123. hello a1b2c3. 123-456-7890");
        assert_eq!(freq.get("hello"), Some(&2));
        assert_eq!(freq.get("world"), Some(&1));
        assert_eq!(freq.get("123"), Some(&1));
        assert_eq!(freq.get("a1b2c3"), Some(&1));
        assert_eq!(freq.get("123-456-7890"), Some(&1));
    }
}
