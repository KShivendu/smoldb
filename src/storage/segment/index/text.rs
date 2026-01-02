use std::{cmp::Ordering, collections::HashMap, sync::RwLock};

use bincode::{Decode, Encode};
use rayon::prelude::*;
use serde_json::Value;
use sled::Db;

use crate::{
    error::{StorageError, StorageResult},
    storage::{
        index::{
            filter::FilterOperator,
            payload_index::{decoded_point_ids, FieldIndexTrait},
        },
        segment::PointId,
    },
};

// Full text search index implementation with posting lists and BM25 ranking
pub struct TextIndex {
    db: sled::Tree,
    in_memory_index: Option<InMemoryTextIndex>,
}

impl FieldIndexTrait<&str> for TextIndex {
    fn open(db: &Db, name: &str, use_in_memory: bool) -> StorageResult<Self> {
        let tree = db.open_tree(format!("{name}_text_index"))?;
        let stats_tree = db.open_tree(format!("{name}_text_stats"))?;

        let in_memory_index = if use_in_memory {
            Some(InMemoryTextIndex::new(&tree, &stats_tree)?)
        } else {
            None
        };

        Ok(Self {
            db: tree,
            in_memory_index,
        })
    }

    fn add_point(&self, point_id: u64, value: &Value) -> StorageResult<()> {
        let text = value.as_str().ok_or_else(|| {
            StorageError::BadInput(format!("Value being inserted is not a string: {value}"))
        })?;

        let terms = tokenize_and_count_frequencies(text);

        // In this tree, the term becomes the key, and the point/doc ID is part of a list of values.

        for (term, term_freq) in terms {
            let term_key = term.as_bytes();

            // Fetch existing point IDs for this term
            let mut term_posting_list = if let Some(in_memory_index) = &self.in_memory_index {
                in_memory_index.get_term_posting_list(&term)?
            } else {
                self.db
                    .get(term_key)?
                    .map(|data| PostingListItem::decode_list(&data))
                    .transpose()?
                    .unwrap_or_else(Vec::new)
            };

            // It's a new document, not updating old ones yet
            // FIXME: Should support updating old ones
            term_posting_list.push(PostingListItem::new(point_id, term_freq));
            term_posting_list.sort_by_key(|item| item.doc_id);

            // Store back
            let encoded_term_posting_list = PostingListItem::encode_list(&term_posting_list)?;
            self.db
                .insert(term_key, encoded_term_posting_list)
                .map_err(|e| {
                    StorageError::ServiceError(format!(
                        "Failed to insert into text index tree: {e}"
                    ))
                })?;

            if let Some(in_memory_index) = &self.in_memory_index {
                in_memory_index.override_term_posting_list(&term, term_posting_list)?;
            }
        }

        Ok(())
    }

    fn query(
        &self,
        value: &str,
        _operation: &FilterOperator, // todo: Remove operation for text index?
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        if let Some(in_memory_index) = &self.in_memory_index {
            return Ok(in_memory_index
                .query(value, limit)?
                .into_iter()
                .map(PointId::Id)
                .collect());
        }

        let mut results = Vec::new();
        let query_key = value.as_bytes();

        let Some(encoded_point_ids) = self.db.get(query_key)? else {
            // No results found for this term
            return Ok(Vec::new());
        };

        // Decode existing point IDs for this term
        let point_ids = decoded_point_ids(&encoded_point_ids)?;

        for point_id in point_ids {
            results.push(PointId::Id(point_id));
            if let Some(lim) = limit {
                if results.len() >= lim {
                    break;
                }
            }
        }

        Ok(results)
    }

    fn add_points(&self, point_ids: &[u64], values: &[Value]) -> StorageResult<()> {
        if point_ids.len() != values.len() {
            return Err(StorageError::BadInput(
                "Point IDs and values length mismatch".to_string(),
            ));
        }

        let texts = values
            .iter()
            .map(|v| {
                v.as_str().ok_or_else(|| {
                    StorageError::BadInput(format!("Value being inserted is not a string: {v}"))
                })
            })
            .collect::<Result<Vec<&str>, StorageError>>()?;

        // Build a temporary index in memory in parallel to avoid multiple passes over the data:
        let temp_index: HashMap<String, Vec<PostingListItem>> = point_ids
            .par_iter()
            .zip(texts.par_iter())
            .fold(
                HashMap::<String, Vec<PostingListItem>>::new,
                |mut acc, (&point_id, text)| {
                    let terms = tokenize_and_count_frequencies(text);
                    for (term, term_freq) in terms {
                        acc.entry(term)
                            .or_default()
                            .push(PostingListItem::new(point_id, term_freq));
                    }
                    acc
                },
            )
            .reduce(HashMap::<String, Vec<PostingListItem>>::new, |mut a, b| {
                for (k, mut v) in b {
                    a.entry(k).or_default().append(&mut v);
                }
                a
            });

        // Clone the db tree for parallel access (sled trees are thread-safe)
        let db = self.db.clone();
        let in_memory_index = self.in_memory_index.as_ref();

        // Merge the temporary index into the main index in parallel:
        let results: Result<Vec<_>, StorageError> = temp_index
            .into_par_iter()
            .map(|(term, mut new_posting_list)| {
                let term_key = term.as_bytes();
                let mut posting_list = if let Some(in_memory_index) = in_memory_index {
                    in_memory_index.get_term_posting_list(&term)?
                } else {
                    db.get(term_key)?
                        .map(|data| PostingListItem::decode_list(&data))
                        .transpose()?
                        .unwrap_or_default()
                };
                posting_list.append(&mut new_posting_list);
                posting_list.sort_by_key(|item| item.doc_id);
                let encoded_posting_list = PostingListItem::encode_list(&posting_list)?;
                db.insert(term_key, encoded_posting_list).map_err(|e| {
                    StorageError::ServiceError(format!(
                        "Failed to insert into text index tree: {e}"
                    ))
                })?;
                Ok((term, posting_list))
            })
            .collect();

        // Update in-memory index sequentially (since it uses RwLock)
        if let Some(in_memory_index) = &self.in_memory_index {
            for result in results? {
                let (term, posting_list) = result;
                // Also updates the stats in the in-memory index
                in_memory_index.override_term_posting_list(&term, posting_list)?;
            }
        }

        Ok(())
    }
}

// todo: Remove this layer and use the BM25Index directly
/// This layer only exists to hold the lock
struct InMemoryTextIndex {
    index: RwLock<BM25Index>,
}

impl InMemoryTextIndex {
    pub fn new(tree: &sled::Tree, stats_tree: &sled::Tree) -> StorageResult<Self> {
        let bm25_index = BM25Index::new(tree, stats_tree)?;
        Ok(Self {
            index: RwLock::new(bm25_index),
        })
    }

    pub fn get_term_posting_list(&self, term: &str) -> StorageResult<Vec<PostingListItem>> {
        let index_guard = self.index.read().map_err(|e| {
            StorageError::ServiceError(format!("Failed to read from in-memory index: {e}"))
        })?;
        index_guard.get_term_posting_list(term)
    }

    pub fn override_term_posting_list(
        &self,
        term: &str,
        posting_list: Vec<PostingListItem>,
    ) -> StorageResult<()> {
        let mut index = self.index.write().map_err(|e| {
            StorageError::ServiceError(format!("Failed to write to in-memory index: {e}"))
        })?;
        index.override_term_posting_list(term, posting_list)
    }

    pub fn query(&self, term: &str, limit: Option<usize>) -> StorageResult<Vec<u64>> {
        let index = self.index.read().map_err(|e| {
            StorageError::ServiceError(format!("Failed to read from in-memory index: {e}"))
        })?;
        index.query(term, limit)
    }
}

struct BM25Index {
    // Todo: Use ahashmap here
    postings: HashMap<String, Vec<PostingListItem>>,
    doc_lengths: HashMap<u64, usize>,
    avg_doc_length: f64,
    k1: f64,
    b: f64,
}

impl BM25Index {
    pub fn new(tree: &sled::Tree, _stats_tree: &sled::Tree) -> StorageResult<Self> {
        let mut postings = HashMap::new();
        for item in tree.iter() {
            let (key, value) = item.map_err(|e| {
                StorageError::ServiceError(format!("Failed to read text index item: {e}"))
            })?;
            let term = String::from_utf8(key.to_vec()).map_err(|e| {
                StorageError::CodecError(format!("Failed to decode term {key:?}: {e}"))
            })?;
            let posting_list = PostingListItem::decode_list(&value)?;
            postings.insert(term, posting_list);
        }

        // TODO: Store doc_lengths and other stats in the stats tree?
        let mut doc_lengths: HashMap<u64, usize> = HashMap::new();
        let mut avg_doc_length = 0.0;
        let k1 = 1.2;
        let b = 0.75;

        for posting_list in postings.values() {
            for item in posting_list {
                *doc_lengths.entry(item.doc_id).or_insert(0) += item.term_freq as usize;
                avg_doc_length += item.term_freq as f64;
            }
        }

        let num_docs = doc_lengths.len();
        if num_docs != 0 {
            avg_doc_length /= num_docs as f64;
        }

        Ok(Self {
            postings,
            doc_lengths,
            avg_doc_length,
            k1,
            b,
        })
    }

    pub fn get_term_posting_list(&self, token: &str) -> StorageResult<Vec<PostingListItem>> {
        let token_posting = self.postings.get(token);
        Ok(token_posting.cloned().unwrap_or_default())
    }

    pub fn override_term_posting_list(
        &mut self,
        token: &str,
        posting_list: Vec<PostingListItem>,
    ) -> StorageResult<()> {
        self.postings.insert(token.to_string(), posting_list);

        // ToDo: This is non-incremental and hence inefficient, optimize later
        // Update doc lengths and avg doc length:
        self.doc_lengths.clear();
        self.avg_doc_length = 0.0;
        for posting_list in self.postings.values() {
            for item in posting_list {
                *self.doc_lengths.entry(item.doc_id).or_insert(0) += item.term_freq as usize;
                self.avg_doc_length += item.term_freq as f64;
            }
        }
        let num_docs = self.doc_lengths.len();
        if num_docs != 0 {
            self.avg_doc_length /= num_docs as f64;
        }

        Ok(())
    }

    pub fn query(&self, q: &str, limit: Option<usize>) -> StorageResult<Vec<u64>> {
        let tokens = tokenize(q);

        // Assume OR operator for now. This means we need to
        // can just iterate over all tokens one by one

        // Traverse all terms (posting lists) and add BM25 scores for each document
        let mut docs_with_scores: HashMap<u64, f64> = HashMap::new();
        for token in tokens {
            let token_posting_list = self.get_term_posting_list(&token)?;
            for item in token_posting_list {
                let score = self.calculate_bm25_score(&token, &item);
                *docs_with_scores.entry(item.doc_id).or_insert(0.0) += score;
            }
        }

        // Sort the documents by score
        let mut sorted_docs = docs_with_scores.into_iter().collect::<Vec<_>>();
        sorted_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

        // If limit is provided, return the top N docs, otherwise return all docs
        if let Some(limit) = limit {
            Ok(sorted_docs
                .into_iter()
                .take(limit)
                .map(|(doc_id, _score)| doc_id)
                .collect())
        } else {
            Ok(sorted_docs
                .into_iter()
                .map(|(doc_id, _score)| doc_id)
                .collect())
        }
    }

    /// Calculate BM25 score for a document w.r.t. a term/token
    fn calculate_bm25_score(&self, term: &str, document: &PostingListItem) -> f64 {
        // TODO: Implement BM25 score calculation

        // Calculate IDF component first since it's simpler and independent of document:
        // DF = How many documents contain this token?
        let document_frequency = self
            .postings
            .get(term)
            .map(|posting| posting.len())
            .unwrap_or(0) as f64;
        // IDF = ln((N - n + 0.5) / (n + 0.5) + 1)
        // N = total number of documents
        // n = document containing the token
        // It gives low value for common terms, high value for rare terms
        let idf_component = ((self.doc_lengths.len() as f64 - document_frequency + 0.5)
            / (document_frequency + 0.5)
            + 1.0)
            .ln();

        // Now let's calculate term frequency component within the document
        // TF = (f * (k1 + 1)) / (f + k1 * (1 - b + b * (dl / avgdl)))
        // It gives higher value for higher term frequency, but with diminishing returns and also penalizes longer documents
        let term_frequency = document.term_freq as f64;
        let doc_length = *self.doc_lengths.get(&document.doc_id).unwrap_or(&0) as f64;
        let tf_component = term_frequency * (self.k1 + 1.0)
            / (term_frequency
                + self.k1 * (1.0 - self.b + self.b * (doc_length / self.avg_doc_length)));

        idf_component * tf_component
    }
}

#[derive(Encode, Decode, Clone)]
struct PostingListItem {
    doc_id: u64,
    term_freq: u64,
}

impl PostingListItem {
    pub fn new(doc_id: u64, term_freq: u64) -> Self {
        Self { doc_id, term_freq }
    }

    pub fn encode_list(posting_list: &[Self]) -> StorageResult<Vec<u8>> {
        bincode::encode_to_vec(posting_list, bincode::config::standard())
            .map_err(|e| StorageError::CodecError(format!("Failed to encode posting list: {e}")))
    }

    pub fn decode_list(data: &[u8]) -> StorageResult<Vec<Self>> {
        bincode::decode_from_slice(data, bincode::config::standard())
            .map(|(item, _)| item)
            .map_err(|e| StorageError::CodecError(format!("Failed to decode posting list: {e}")))
    }
}

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
    let mut with_freq = HashMap::new();
    for term in tokenize(text) {
        *with_freq.entry(term).or_insert(0) += 1;
    }
    with_freq
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_tokenizer() {
        assert_eq!(
            tokenize("Hello, world! 123. a1b2c3. 123-456-7890"),
            ["hello", "world", "123", "a1b2c3", "123-456-7890"]
        );
        assert_eq!(
            tokenize_and_count_frequencies("Hello, world! 123. hello a1b2c3. 123-456-7890"),
            HashMap::from([
                ("hello".into(), 2),
                ("world".into(), 1),
                ("123".into(), 1),
                ("a1b2c3".into(), 1),
                ("123-456-7890".into(), 1)
            ])
        );
    }

    #[test]
    fn test_bm25() {
        let tmp_path = tempfile::tempdir().unwrap();
        let db = sled::open(tmp_path.path()).unwrap();
        let index = TextIndex::open(&db, "content", true).unwrap();

        // Store points as (point_id, text) for initial insertions
        let docs = [
            "The quick brown fox jumps over the lazy dog",
            "The quick brown fox",
            "Lazy dog sleeps all day",
            "A fast brown fox leaps over a sleepy dog",
            "the the",
        ];

        for (id, doc) in docs.iter().enumerate() {
            index
                .add_point((id) as u64, &Value::String((*doc).to_string()))
                .unwrap();
        }

        // Query for "quick fox"
        let results = index.query("quick fox", &FilterOperator::Eq, None).unwrap();

        // This order happens because:
        // 1: has both terms, so highest score and is shorter
        // 0: has both terms but is longer
        // 3: has "fox" only
        let expected_ids: Vec<PointId> = vec![PointId::Id(1), PointId::Id(0), PointId::Id(3)];
        assert_eq!(
            results, expected_ids,
            "BM25 query results do not match expected"
        );

        let results = index.query("the", &FilterOperator::Eq, None).unwrap();
        let expected_ids: Vec<PointId> = vec![
            PointId::Id(4), // has "the" twice and shorter
            PointId::Id(0), // has "the" twice
            PointId::Id(1), // has "the" once
        ];
        assert_eq!(
            results, expected_ids,
            "BM25 query results for common term do not match expected"
        );
    }
}
