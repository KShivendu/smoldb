use std::{cmp::Ordering, collections::BinaryHeap, sync::RwLock};

use ahash::HashMap;
use bincode::{Decode, Encode};
use serde_json::Value;
use sled::Db;

use crate::{
    error::{StorageError, StorageResult},
    storage::{
        index::{filter::FilterOperator, payload_index::FieldIndexTrait},
        segment::PointId,
    },
};

// Full text search index implementation with posting lists and BM25 ranking
pub struct TextIndex {
    db: sled::Tree,
    in_memory_index: Option<RwLock<InMemBM25Index>>,
}

impl FieldIndexTrait<&str> for TextIndex {
    fn open(db: &Db, name: &str, use_in_memory: bool) -> StorageResult<Self> {
        let tree = db.open_tree(format!("{name}_text_index"))?;
        let stats_tree = db.open_tree(format!("{name}_text_stats"))?;

        let in_memory_index = if use_in_memory {
            Some(RwLock::new(InMemBM25Index::new(&tree, &stats_tree)?))
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

        // Collect all updates first, then apply in batch
        let mut updates: Vec<(String, Vec<PostingListItem>)> = Vec::with_capacity(terms.len());

        for (term, term_freq) in &terms {
            let term_key = term.as_bytes();

            // Fetch existing posting list for this term
            let mut term_posting_list = if let Some(in_memory_index) = &self.in_memory_index {
                let index = in_memory_index.read().map_err(|e| {
                    StorageError::ServiceError(format!("Failed to read from in-memory index: {e}"))
                })?;
                index.get_term_posting_list(term)?
            } else {
                self.db
                    .get(term_key)?
                    .map(|data| PostingListItem::decode_list(&data))
                    .transpose()?
                    .unwrap_or_default()
            };

            // Insert maintaining sorted order using binary search
            let new_item = PostingListItem::new(point_id, *term_freq);
            match term_posting_list.binary_search_by_key(&point_id, |item| item.doc_id) {
                Ok(pos) => {
                    // Update existing entry
                    term_posting_list[pos] = new_item;
                }
                Err(pos) => {
                    term_posting_list.insert(pos, new_item);
                }
            }

            // Store to disk
            let encoded = PostingListItem::encode_list(&term_posting_list)?;
            self.db.insert(term_key, encoded).map_err(|e| {
                StorageError::ServiceError(format!("Failed to insert into text index tree: {e}"))
            })?;

            updates.push((term.clone(), term_posting_list));
        }

        // Batch update in-memory index with document stats
        if let Some(in_memory_index) = &self.in_memory_index {
            // Calculate document length (total term count)
            let doc_length: u64 = terms.values().sum();
            let mut index = in_memory_index.write().map_err(|e| {
                StorageError::ServiceError(format!("Failed to write to in-memory index: {e}"))
            })?;
            index.add_document_batch(point_id, doc_length, updates)?;
        }

        Ok(())
    }

    fn query(
        &self,
        value: &str,
        _operation: &FilterOperator,
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        if let Some(in_memory_index) = &self.in_memory_index {
            let index = in_memory_index.read().map_err(|e| {
                StorageError::ServiceError(format!("Failed to read from in-memory index: {e}"))
            })?;
            return Ok(index
                .query(value, limit)?
                .into_iter()
                .map(PointId::Id)
                .collect());
        }

        // For disk-only path, we need to properly decode and score
        // Build a temporary BM25 scorer for this query
        let tokens = tokenize(value);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        // Collect posting lists for all query tokens
        let mut token_postings: Vec<Vec<PostingListItem>> = Vec::with_capacity(tokens.len());
        let mut all_doc_ids: HashMap<u64, ()> = HashMap::default();

        for token in &tokens {
            let term_key = token.as_bytes();
            if let Some(data) = self.db.get(term_key)? {
                let posting_list = PostingListItem::decode_list(&data)?;
                for item in &posting_list {
                    all_doc_ids.insert(item.doc_id, ());
                }
                token_postings.push(posting_list);
            }
        }

        if all_doc_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Calculate doc lengths and stats for BM25
        let mut doc_lengths: HashMap<u64, usize> = HashMap::default();
        let mut total_length: f64 = 0.0;

        // We need to scan all terms to compute doc lengths accurately
        // This is expensive for disk-only but necessary for correct BM25
        for item in self.db.iter() {
            let (_key, value) = item.map_err(|e| {
                StorageError::ServiceError(format!("Failed to read text index item: {e}"))
            })?;
            let posting_list = PostingListItem::decode_list(&value)?;
            for item in &posting_list {
                *doc_lengths.entry(item.doc_id).or_insert(0) += item.term_freq as usize;
                total_length += item.term_freq as f64;
            }
        }

        let num_docs = doc_lengths.len();
        let avg_doc_length = if num_docs > 0 {
            total_length / num_docs as f64
        } else {
            1.0
        };

        // BM25 parameters
        let k1 = 1.2;
        let b = 0.75;

        // Calculate scores
        let mut docs_with_scores: HashMap<u64, f64> = HashMap::default();
        for (_token, posting_list) in tokens.iter().zip(token_postings.iter()) {
            let df = posting_list.len() as f64;
            let idf = ((num_docs as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();

            for item in posting_list {
                let doc_len = *doc_lengths.get(&item.doc_id).unwrap_or(&0) as f64;
                let tf = item.term_freq as f64;
                let tf_component =
                    tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * (doc_len / avg_doc_length)));
                let score = idf * tf_component;
                *docs_with_scores.entry(item.doc_id).or_insert(0.0) += score;
            }
        }

        // Use top-k selection with BinaryHeap for efficiency
        let results = top_k_by_score(docs_with_scores, limit);
        Ok(results.into_iter().map(PointId::Id).collect())
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

        // Build temporary index in memory to batch all operations
        let mut temp_index: HashMap<String, Vec<PostingListItem>> = HashMap::default();
        // Track document lengths for incremental stats update
        let mut new_doc_lengths: HashMap<u64, u64> = HashMap::default();

        for (point_id, text) in point_ids.iter().zip(texts.iter()) {
            let terms = tokenize_and_count_frequencies(text);
            let doc_length: u64 = terms.values().sum();
            new_doc_lengths.insert(*point_id, doc_length);

            for (term, term_freq) in terms {
                temp_index
                    .entry(term)
                    .or_default()
                    .push(PostingListItem::new(*point_id, term_freq));
            }
        }

        // Collect all updates for batch in-memory update
        let mut all_updates: Vec<(String, Vec<PostingListItem>)> =
            Vec::with_capacity(temp_index.len());

        // Merge temporary index into main index
        for (term, mut new_posting_list) in temp_index {
            let term_key = term.as_bytes();
            let mut posting_list = if let Some(in_memory_index) = &self.in_memory_index {
                let index = in_memory_index.read().map_err(|e| {
                    StorageError::ServiceError(format!("Failed to read from in-memory index: {e}"))
                })?;
                index.get_term_posting_list(&term)?
            } else {
                self.db
                    .get(term_key)?
                    .map(|data| PostingListItem::decode_list(&data))
                    .transpose()?
                    .unwrap_or_default()
            };

            // Sort new items by doc_id for efficient merge
            new_posting_list.sort_unstable_by_key(|item| item.doc_id);

            // Merge sorted lists
            posting_list = merge_sorted_posting_lists(posting_list, new_posting_list);

            let encoded = PostingListItem::encode_list(&posting_list)?;
            self.db.insert(term_key, encoded).map_err(|e| {
                StorageError::ServiceError(format!("Failed to insert into text index tree: {e}"))
            })?;

            all_updates.push((term, posting_list));
        }

        // Batch update in-memory index
        if let Some(in_memory_index) = &self.in_memory_index {
            let mut index = in_memory_index.write().map_err(|e| {
                StorageError::ServiceError(format!("Failed to write to in-memory index: {e}"))
            })?;
            index.add_documents_batch(new_doc_lengths, all_updates)?;
        }

        Ok(())
    }
}

/// Merge two sorted posting lists, preferring items from `new` when doc_ids collide
fn merge_sorted_posting_lists(
    existing: Vec<PostingListItem>,
    new: Vec<PostingListItem>,
) -> Vec<PostingListItem> {
    let mut result = Vec::with_capacity(existing.len() + new.len());
    let mut i = 0;
    let mut j = 0;

    while i < existing.len() && j < new.len() {
        match existing[i].doc_id.cmp(&new[j].doc_id) {
            Ordering::Less => {
                result.push(existing[i].clone());
                i += 1;
            }
            Ordering::Greater => {
                result.push(new[j].clone());
                j += 1;
            }
            Ordering::Equal => {
                // New takes precedence (update case)
                result.push(new[j].clone());
                i += 1;
                j += 1;
            }
        }
    }

    // Append remaining
    result.extend_from_slice(&existing[i..]);
    result.extend_from_slice(&new[j..]);
    result
}

/// Efficiently get top-k documents by score using a min-heap
fn top_k_by_score(scores: HashMap<u64, f64>, limit: Option<usize>) -> Vec<u64> {
    // Wrapper for BinaryHeap ordering (min-heap via Reverse)
    #[derive(PartialEq)]
    struct DocScore(u64, f64);

    impl Eq for DocScore {}

    impl PartialOrd for DocScore {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for DocScore {
        fn cmp(&self, other: &Self) -> Ordering {
            // Reverse order for min-heap behavior
            other.1.partial_cmp(&self.1).unwrap_or(Ordering::Equal)
        }
    }

    match limit {
        Some(k) if k < scores.len() => {
            // Use min-heap to keep top-k
            let mut heap: BinaryHeap<DocScore> = BinaryHeap::with_capacity(k + 1);

            for (doc_id, score) in scores {
                heap.push(DocScore(doc_id, score));
                if heap.len() > k {
                    heap.pop(); // Remove smallest
                }
            }

            // Extract in descending order
            let mut result: Vec<_> = heap.into_iter().map(|ds| (ds.0, ds.1)).collect();
            result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            result.into_iter().map(|(id, _)| id).collect()
        }
        _ => {
            // No limit or limit >= docs, sort all
            let mut sorted: Vec<_> = scores.into_iter().collect();
            sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            sorted.into_iter().map(|(id, _)| id).collect()
        }
    }
}

struct InMemBM25Index {
    postings: HashMap<String, Vec<PostingListItem>>,
    doc_lengths: HashMap<u64, usize>,
    total_doc_length: f64, // Sum of all document lengths for incremental avg calculation
    avg_doc_length: f64,
    k1: f64,
    b: f64,
}

impl InMemBM25Index {
    pub fn new(tree: &sled::Tree, _stats_tree: &sled::Tree) -> StorageResult<Self> {
        let mut postings: HashMap<String, Vec<PostingListItem>> = HashMap::default();
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

        // Calculate doc_lengths and stats
        let mut doc_lengths: HashMap<u64, usize> = HashMap::default();
        let mut total_doc_length = 0.0;

        for posting_list in postings.values() {
            for item in posting_list {
                *doc_lengths.entry(item.doc_id).or_insert(0) += item.term_freq as usize;
                total_doc_length += item.term_freq as f64;
            }
        }

        let num_docs = doc_lengths.len();
        let avg_doc_length = if num_docs != 0 {
            total_doc_length / num_docs as f64
        } else {
            0.0
        };

        Ok(Self {
            postings,
            doc_lengths,
            total_doc_length,
            avg_doc_length,
            k1: 1.2,
            b: 0.75,
        })
    }

    pub fn get_term_posting_list(&self, token: &str) -> StorageResult<Vec<PostingListItem>> {
        Ok(self.postings.get(token).cloned().unwrap_or_default())
    }

    /// Add a single document incrementally
    pub fn add_document_batch(
        &mut self,
        doc_id: u64,
        doc_length: u64,
        term_updates: Vec<(String, Vec<PostingListItem>)>,
    ) -> StorageResult<()> {
        // Update posting lists
        for (term, posting_list) in term_updates {
            self.postings.insert(term, posting_list);
        }

        // Incrementally update stats
        let old_length = self.doc_lengths.get(&doc_id).copied().unwrap_or(0);
        let new_length = doc_length as usize;

        if old_length == 0 {
            // New document
            self.total_doc_length += new_length as f64;
            self.doc_lengths.insert(doc_id, new_length);
        } else {
            // Update existing document
            self.total_doc_length += (new_length as f64) - (old_length as f64);
            self.doc_lengths.insert(doc_id, new_length);
        }

        // Recalculate average
        let num_docs = self.doc_lengths.len();
        self.avg_doc_length = if num_docs > 0 {
            self.total_doc_length / num_docs as f64
        } else {
            0.0
        };

        Ok(())
    }

    /// Add multiple documents incrementally
    pub fn add_documents_batch(
        &mut self,
        new_doc_lengths: HashMap<u64, u64>,
        term_updates: Vec<(String, Vec<PostingListItem>)>,
    ) -> StorageResult<()> {
        // Update posting lists
        for (term, posting_list) in term_updates {
            self.postings.insert(term, posting_list);
        }

        // Incrementally update stats for all new documents
        for (doc_id, new_length) in new_doc_lengths {
            let old_length = self.doc_lengths.get(&doc_id).copied().unwrap_or(0);
            let new_length = new_length as usize;

            if old_length == 0 {
                // New document
                self.total_doc_length += new_length as f64;
                self.doc_lengths.insert(doc_id, new_length);
            } else {
                // Update existing document
                self.total_doc_length += (new_length as f64) - (old_length as f64);
                self.doc_lengths.insert(doc_id, new_length);
            }
        }

        // Recalculate average
        let num_docs = self.doc_lengths.len();
        self.avg_doc_length = if num_docs > 0 {
            self.total_doc_length / num_docs as f64
        } else {
            0.0
        };

        Ok(())
    }

    pub fn query(&self, q: &str, limit: Option<usize>) -> StorageResult<Vec<u64>> {
        let tokens = tokenize(q);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        // Traverse all terms (posting lists) and accumulate BM25 scores
        let mut docs_with_scores: HashMap<u64, f64> = HashMap::default();

        for token in &tokens {
            if let Some(posting_list) = self.postings.get(token) {
                // Pre-compute IDF for this term (same for all docs)
                let idf = self.calculate_idf(token);

                for item in posting_list {
                    let tf_component = self.calculate_tf_component(item);
                    let score = idf * tf_component;
                    *docs_with_scores.entry(item.doc_id).or_insert(0.0) += score;
                }
            }
        }

        // Use efficient top-k selection
        Ok(top_k_by_score(docs_with_scores, limit))
    }

    /// Calculate IDF component for a term
    #[inline]
    fn calculate_idf(&self, term: &str) -> f64 {
        let df = self
            .postings
            .get(term)
            .map(|posting| posting.len())
            .unwrap_or(0) as f64;
        let n = self.doc_lengths.len() as f64;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Calculate TF component for a document
    #[inline]
    fn calculate_tf_component(&self, item: &PostingListItem) -> f64 {
        let tf = item.term_freq as f64;
        let doc_len = *self.doc_lengths.get(&item.doc_id).unwrap_or(&0) as f64;
        let avg_dl = if self.avg_doc_length > 0.0 {
            self.avg_doc_length
        } else {
            1.0
        };
        tf * (self.k1 + 1.0) / (tf + self.k1 * (1.0 - self.b + self.b * (doc_len / avg_dl)))
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Debug)]
struct PostingListItem {
    doc_id: u64,
    /// Number of times the term appears in the document
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
    let mut with_freq: HashMap<String, u64> = HashMap::default();
    for term in tokenize(text) {
        *with_freq.entry(term).or_insert(0) += 1;
    }
    with_freq
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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

    #[test]
    fn test_bm25() {
        let docs = [
            "The quick brown fox jumps over the lazy dog",
            "The quick brown fox",
            "Lazy dog sleeps all day",
            "A fast brown fox leaps over a sleepy dog",
            "the the",
        ];

        // Test single insert
        let tmp_path = tempfile::tempdir().unwrap();
        let db = sled::open(tmp_path.path()).unwrap();
        let single_index = TextIndex::open(&db, "content", true).unwrap();

        for (id, doc) in docs.iter().enumerate() {
            single_index
                .add_point(id as u64, &Value::String((*doc).to_string()))
                .unwrap();
        }

        // Test batch insert produces same results
        let tmp_path_batch = tempfile::tempdir().unwrap();
        let db_batch = sled::open(tmp_path_batch.path()).unwrap();
        let batch_index = TextIndex::open(&db_batch, "content", true).unwrap();

        let point_ids: Vec<u64> = (0..docs.len() as u64).collect();
        let values: Vec<Value> = docs.iter().map(|s| Value::String(s.to_string())).collect();
        batch_index.add_points(&point_ids, &values).unwrap();

        // Query for "quick fox" - verify both methods produce same results
        // Expected order:
        // 1: has both terms, so highest score and is shorter
        // 0: has both terms but is longer
        // 3: has "fox" only
        let expected_quick_fox: Vec<PointId> = vec![PointId::Id(1), PointId::Id(0), PointId::Id(3)];

        let single_results = single_index
            .query("quick fox", &FilterOperator::Eq, None)
            .unwrap();
        let batch_results = batch_index
            .query("quick fox", &FilterOperator::Eq, None)
            .unwrap();

        assert_eq!(
            single_results, expected_quick_fox,
            "Single insert query mismatch"
        );
        assert_eq!(
            batch_results, expected_quick_fox,
            "Batch insert query mismatch"
        );

        // Query for common term "the"
        let expected_the: Vec<PointId> = vec![
            PointId::Id(4), // has "the" twice and shorter
            PointId::Id(0), // has "the" twice
            PointId::Id(1), // has "the" once
        ];
        let results = single_index
            .query("the", &FilterOperator::Eq, None)
            .unwrap();
        assert_eq!(results, expected_the, "Common term query mismatch");

        // Test limit (top-k)
        let results = single_index
            .query("quick fox", &FilterOperator::Eq, Some(2))
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], PointId::Id(1));
        assert_eq!(results[1], PointId::Id(0));
    }

    #[test]
    fn test_bm25_only_disk() {
        let tmp_path = tempfile::tempdir().unwrap();
        let db = sled::open(tmp_path.path()).unwrap();
        // Create with in_memory = false
        let index = TextIndex::open(&db, "content", false).unwrap();

        let docs = [
            "The quick brown fox jumps over the lazy dog",
            "The quick brown fox",
        ];

        for (id, doc) in docs.iter().enumerate() {
            index
                .add_point(id as u64, &Value::String((*doc).to_string()))
                .unwrap();
        }

        // Query should still work and return results
        let results = index.query("quick fox", &FilterOperator::Eq, None).unwrap();
        assert!(!results.is_empty(), "Disk-only query should return results");
        // Both docs contain these terms
        assert!(results.contains(&PointId::Id(0)));
        assert!(results.contains(&PointId::Id(1)));
    }

    #[test]
    fn test_merge_sorted_posting_lists() {
        let existing = vec![
            PostingListItem::new(1, 2),
            PostingListItem::new(3, 1),
            PostingListItem::new(5, 3),
        ];
        let new = vec![
            PostingListItem::new(2, 1),
            PostingListItem::new(3, 5), // Updated
            PostingListItem::new(4, 2),
        ];

        let merged = merge_sorted_posting_lists(existing, new);
        assert_eq!(merged.len(), 5);
        assert_eq!(merged[0].doc_id, 1);
        assert_eq!(merged[1].doc_id, 2);
        assert_eq!(merged[2].doc_id, 3);
        assert_eq!(merged[2].term_freq, 5); // Should be updated value
        assert_eq!(merged[3].doc_id, 4);
        assert_eq!(merged[4].doc_id, 5);
    }

    #[test]
    fn test_posting_list_encoding_size() {
        let mut posting_list = vec![];

        let encoded = PostingListItem::encode_list(&posting_list).unwrap();
        assert_eq!(encoded, vec![0]);

        posting_list.push(PostingListItem::new(5, 8));
        let encoded = PostingListItem::encode_list(&posting_list).unwrap();
        assert_eq!(encoded, vec![1, 5, 8]);

        posting_list.push(PostingListItem::new(55, 3));
        posting_list.push(PostingListItem::new(3, 1));
        let encoded = PostingListItem::encode_list(&posting_list).unwrap();
        assert_eq!(encoded, vec![3, 5, 8, 55, 3, 3, 1]); // first byte is the length of the list

        let decoded = PostingListItem::decode_list(&encoded).unwrap();
        assert_eq!(decoded, posting_list);
    }
}
