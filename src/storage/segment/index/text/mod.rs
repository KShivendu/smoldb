mod bm25;
mod posting;
mod tokenizer;

use std::{cmp::Ordering, collections::BinaryHeap, sync::RwLock};

use ahash::HashMap;
use serde_json::Value;
use sled::Db;

use crate::{
    error::{StorageError, StorageResult},
    storage::{
        index::{filter::FilterOperator, payload_index::FieldIndexTrait},
        segment::PointId,
    },
};

use bm25::BM25Wrapper;
use posting::{merge_sorted_posting_lists, InMemPostings, PostingListItem};
use tokenizer::{tokenize, tokenize_and_count_frequencies};

// Full text search index implementation with posting lists and BM25 ranking
pub struct TextIndex {
    db: sled::Tree,
    bm25_scorer: BM25Wrapper,
    in_memory_postings: Option<RwLock<InMemPostings>>,
}

impl FieldIndexTrait<&str> for TextIndex {
    fn open(db: &Db, name: &str, use_in_memory: bool) -> StorageResult<Self> {
        let tree = db.open_tree(format!("{name}_text_index"))?;
        let stats_tree = db.open_tree(format!("{name}_text_stats"))?;

        let bm25_scorer = BM25Wrapper::open(stats_tree)?;

        let in_memory_postings = if use_in_memory {
            Some(RwLock::new(InMemPostings::new(&tree)?))
        } else {
            None
        };

        Ok(Self {
            db: tree,
            bm25_scorer,
            in_memory_postings,
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
            let mut term_posting_list = if let Some(in_memory_postings) = &self.in_memory_postings {
                let postings = in_memory_postings.read().map_err(|e| {
                    StorageError::ServiceError(format!(
                        "Failed to read from in-memory postings: {e}"
                    ))
                })?;
                postings.get(term).cloned().unwrap_or_default()
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

        // Calculate document length (total term count)
        let doc_length: u64 = terms.values().sum();

        // Update BM25 stats (shared between disk and in-memory)
        self.bm25_scorer
            .add_document(point_id, doc_length)
            .map_err(|e| StorageError::ServiceError(format!("Failed to update BM25 stats: {e}")))?;

        // Update in-memory postings cache if enabled
        if let Some(in_memory_postings) = &self.in_memory_postings {
            let mut postings = in_memory_postings.write().map_err(|e| {
                StorageError::ServiceError(format!("Failed to write to in-memory postings: {e}"))
            })?;
            for (term, posting_list) in updates {
                postings.insert(term, posting_list);
            }
        }

        Ok(())
    }

    fn query(
        &self,
        value: &str,
        _operation: &FilterOperator,
        limit: Option<usize>,
    ) -> StorageResult<Vec<PointId>> {
        let tokens = tokenize(value);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        // Collect posting lists for all query tokens
        let mut token_postings: Vec<(usize, Vec<PostingListItem>)> =
            Vec::with_capacity(tokens.len());

        if let Some(in_memory_postings) = &self.in_memory_postings {
            let postings = in_memory_postings.read().map_err(|e| {
                StorageError::ServiceError(format!("Failed to read from in-memory postings: {e}"))
            })?;
            for token in &tokens {
                if let Some(posting_list) = postings.get(token) {
                    token_postings.push((posting_list.len(), posting_list.clone()));
                }
            }
        } else {
            for token in &tokens {
                let term_key = token.as_bytes();
                if let Some(data) = self.db.get(term_key)? {
                    let posting_list = PostingListItem::decode_list(&data)?;
                    let df = posting_list.len();
                    token_postings.push((df, posting_list));
                }
            }
        }

        if token_postings.is_empty() {
            return Ok(Vec::new());
        }

        let postings_refs: Vec<(usize, &[PostingListItem])> = token_postings
            .iter()
            .map(|(df, list)| (*df, list.as_slice()))
            .collect();

        let docs_with_scores = self.bm25_scorer.score_documents(&postings_refs)?;

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
            let mut posting_list = if let Some(in_memory_postings) = &self.in_memory_postings {
                let postings = in_memory_postings.read().map_err(|e| {
                    StorageError::ServiceError(format!(
                        "Failed to read from in-memory postings: {e}"
                    ))
                })?;
                postings.get(&term).cloned().unwrap_or_default()
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

        // Update BM25 stats (shared between disk and in-memory)
        self.bm25_scorer.add_documents(&new_doc_lengths)?;

        // Update in-memory postings cache if enabled
        if let Some(in_memory_postings) = &self.in_memory_postings {
            let mut postings = in_memory_postings.write().map_err(|e| {
                StorageError::ServiceError(format!("Failed to write to in-memory postings: {e}"))
            })?;
            for (term, posting_list) in all_updates {
                postings.insert(term, posting_list);
            }
        }

        Ok(())
    }
}

/// Efficiently get top-k documents by score using a min-heap
fn top_k_by_score(scores: HashMap<u64, f64>, limit: Option<usize>) -> Vec<u64> {
    // todo: Do this during posting list BM25 iteration?

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_text_index() {
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
    fn test_text_index_only_disk() {
        // todo: Use rstest to merge with previous test
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
}
