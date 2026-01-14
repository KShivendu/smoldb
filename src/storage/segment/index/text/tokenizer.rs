use std::sync::LazyLock;

use ahash::{HashMap, HashSet};

/// Common English stop words for O(1) lookup, initialized once on first access.
/// These words have very high document frequency and poor selectivity.
static STOP_WORDS_SET: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from_iter([
        // Articles
        "a",
        "an",
        "the",
        // Pronouns
        "i",
        "me",
        "my",
        "myself",
        "we",
        "our",
        "ours",
        "ourselves",
        "you",
        "your",
        "yours",
        "yourself",
        "yourselves",
        "he",
        "him",
        "his",
        "himself",
        "she",
        "her",
        "hers",
        "herself",
        "it",
        "its",
        "itself",
        "they",
        "them",
        "their",
        "theirs",
        "themselves",
        "what",
        "which",
        "who",
        "whom",
        "this",
        "that",
        "these",
        "those",
        // Prepositions
        "in",
        "on",
        "at",
        "by",
        "for",
        "with",
        "about",
        "against",
        "between",
        "into",
        "through",
        "during",
        "before",
        "after",
        "above",
        "below",
        "to",
        "from",
        "up",
        "down",
        "out",
        "off",
        "over",
        "under",
        // Conjunctions
        "and",
        "but",
        "or",
        "nor",
        "so",
        "yet",
        "both",
        "either",
        "neither",
        // Common verbs
        "am",
        "is",
        "are",
        "was",
        "were",
        "be",
        "been",
        "being",
        "have",
        "has",
        "had",
        "having",
        "do",
        "does",
        "did",
        "doing",
        "would",
        "should",
        "could",
        "ought",
        "will",
        "shall",
        "can",
        "may",
        "might",
        "must",
        // Other common words
        "here",
        "there",
        "when",
        "where",
        "why",
        "how",
        "all",
        "each",
        "every",
        "few",
        "more",
        "most",
        "other",
        "some",
        "such",
        "no",
        "not",
        "only",
        "own",
        "same",
        "than",
        "too",
        "very",
        "just",
        "also",
        "now",
        "of",
        "as",
        "if",
        "then",
        "because",
        "while",
        "although",
        "though",
    ])
});

/// Tokenize the text into terms, filtering out stop words
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|s| !s.is_empty() && !STOP_WORDS_SET.contains(s))
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
        // Note: "a" is filtered as a stop word
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

    #[test]
    fn test_stop_words_filtered() {
        // All these are stop words and should return empty
        assert!(tokenize("the a an is are was were").is_empty());

        // Mix of stop words and regular words
        assert_eq!(
            tokenize("the quick brown fox jumps over the lazy dog"),
            ["quick", "brown", "fox", "jumps", "lazy", "dog"]
        );

        // Verify stop words don't appear in frequency map
        let freq = tokenize_and_count_frequencies("the the the cat sat on the mat");
        assert_eq!(freq.get("the"), None); // "the" is a stop word
        assert_eq!(freq.get("cat"), Some(&1));
        assert_eq!(freq.get("sat"), Some(&1));
        assert_eq!(freq.get("mat"), Some(&1));
    }
}
