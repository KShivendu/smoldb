use std::cmp::Ordering;
use std::collections::BinaryHeap;

use ahash::HashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::{
    error::{StorageError, StorageResult},
    storage::index::text::posting::PostingList,
};

// Keys for persisting BM25 stats in sled
const STATS_KEY: &[u8] = b"__bm25_stats__";
const DEFAULT_K1: f64 = 1.2;
const DEFAULT_B: f64 = 0.75;

pub struct BM25Scorer {
    pub stats_tree: sled::Tree,
    pub bm25_stats: RwLock<Bm25Stats>,
}

impl BM25Scorer {
    /// Create a new BM25Scorer instance, loading existing stats from sled
    pub fn open(stats_tree: sled::Tree) -> StorageResult<Self> {
        // use serde_cbor to load the stats from the tree
        let mut stats: Bm25Stats = stats_tree
            .get(STATS_KEY)?
            .map(|data| serde_cbor::from_slice(&data))
            .transpose()
            .map_err(|e| StorageError::CodecError(format!("Failed to decode BM25 stats: {e}")))?
            .unwrap_or_default();
        // Recompute cached avg_dl from persisted data
        stats.recompute_avg_dl();
        Ok(Self {
            stats_tree,
            bm25_stats: RwLock::new(stats),
        })
    }

    /// Persist stats to sled
    fn persist(&self, stats: &Bm25Stats) -> StorageResult<()> {
        let encoded = serde_cbor::to_vec(&stats)
            .map_err(|e| StorageError::CodecError(format!("Failed to encode BM25 stats: {e}")))?;
        self.stats_tree.insert(STATS_KEY, encoded)?;
        Ok(())
    }

    /// Add or update a single document's stats
    pub fn add_document(&self, doc_id: u64, doc_length: u64) -> StorageResult<()> {
        let mut stats = self.bm25_stats.write();
        let old_length = stats.doc_length(doc_id);
        let new_length = doc_length as usize;

        if old_length == 0 {
            // New document
            stats.total_doc_length += new_length as f64;
        } else {
            // Update existing document
            stats.total_doc_length += (new_length as f64) - (old_length as f64);
        }
        stats.doc_lengths.insert(doc_id, new_length);
        stats.recompute_avg_dl();

        self.persist(&stats)
    }

    /// Add or update multiple documents' stats in batch
    pub fn add_documents(&self, new_doc_lengths: &HashMap<u64, u64>) -> StorageResult<()> {
        let mut stats = self.bm25_stats.write();
        for (&doc_id, &new_length) in new_doc_lengths {
            let old_length = stats.doc_length(doc_id);
            let new_length = new_length as usize;

            if old_length == 0 {
                stats.total_doc_length += new_length as f64;
            } else {
                stats.total_doc_length += (new_length as f64) - (old_length as f64);
            }
            stats.doc_lengths.insert(doc_id, new_length);
        }
        stats.recompute_avg_dl();

        self.persist(&stats)
    }

    /// Calculate BM25 scores for documents matching the given posting lists.
    /// Uses SoA layout for cache-efficient iteration.
    pub fn score_documents(
        &self,
        token_postings: &[(usize, &PostingList)], // (df, posting_list) pairs
    ) -> StorageResult<HashMap<u64, f64>> {
        // Rank completely from memory because it's faster
        let stats = self.bm25_stats.read();
        let avg_dl = stats.cached_avg_dl;
        let mut docs_with_scores: HashMap<u64, f64> = HashMap::default();

        for (df, posting_list) in token_postings {
            let idf = stats.calculate_idf(*df);

            // Iterate over SoA posting list - doc_ids and term_freqs are contiguous in memory
            for i in 0..posting_list.len() {
                let doc_id = posting_list.doc_ids[i];
                let term_freq = posting_list.term_freqs[i];
                let tf_component = stats.calculate_tf_component(term_freq, doc_id, avg_dl);
                let score = idf * tf_component;
                *docs_with_scores.entry(doc_id).or_insert(0.0) += score;
            }
        }

        Ok(docs_with_scores)
    }

    /// Compute the upper-bound score a term can contribute to any document.
    /// This is used by WAND for early termination.
    fn compute_upper_bound(
        &self,
        df: usize,
        posting_list: &PostingList,
        stats: &Bm25Stats,
        avg_dl: f64,
    ) -> f64 {
        let idf = stats.calculate_idf(df);

        // Find the maximum TF component across all docs in this posting list
        let max_tf_component = posting_list
            .term_freqs
            .iter()
            .zip(posting_list.doc_ids.iter())
            .map(|(&tf, &doc_id)| stats.calculate_tf_component(tf, doc_id, avg_dl))
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))
            .unwrap_or(0.0);

        idf * max_tf_component
    }

    /// WAND (Weak AND) algorithm for efficient top-k scoring.
    /// Skips documents that cannot make it into the top-k results.
    pub fn score_documents_wand(
        &self,
        token_postings: &[(usize, &PostingList)], // (df, posting_list) pairs
        limit: usize,
    ) -> StorageResult<Vec<(u64, f64)>> {
        if token_postings.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let stats = self.bm25_stats.read();
        let avg_dl = stats.cached_avg_dl;

        // Build cursors with upper bounds and IDF precomputed
        let mut cursors: Vec<WandCursor> = token_postings
            .iter()
            .filter(|(_, pl)| !pl.doc_ids.is_empty())
            .map(|(df, posting_list)| {
                let idf = stats.calculate_idf(*df);
                let upper_bound = self.compute_upper_bound(*df, posting_list, &stats, avg_dl);
                WandCursor::new(posting_list, idf, upper_bound)
            })
            .collect();

        if cursors.is_empty() {
            return Ok(Vec::new());
        }

        // Min-heap to track top-k (stores negative scores for min-heap behavior)
        let mut top_k: BinaryHeap<MinScoreDoc> = BinaryHeap::with_capacity(limit + 1);
        let mut threshold = 0.0f64;

        loop {
            // Sort cursors by current doc_id (ascending)
            cursors.sort_unstable_by_key(|c| c.current_doc_id());

            // Remove exhausted cursors
            cursors.retain(|c| !c.is_exhausted());
            if cursors.is_empty() {
                break;
            }

            let min_doc = cursors[0].current_doc_id();

            // Find pivot: first position where cumulative upper bounds >= threshold
            let pivot_idx = self.find_pivot(&cursors, threshold);

            if pivot_idx.is_none() {
                // No pivot found - remaining docs can't beat threshold
                break;
            }
            let pivot_idx = pivot_idx.unwrap();
            let pivot_doc = cursors[pivot_idx].current_doc_id();

            // Check if the minimum doc_id equals pivot doc (all contributing terms aligned)
            if min_doc == pivot_doc {
                // Find all cursors pointing to this doc (they're contiguous after sorting)
                let end_idx = cursors
                    .iter()
                    .position(|c| c.current_doc_id() != pivot_doc)
                    .unwrap_or(cursors.len());

                // Fully score this document using all matching cursors
                let score = self.score_document_at_cursors(&cursors[..end_idx], &stats, avg_dl);

                if score > threshold || top_k.len() < limit {
                    top_k.push(MinScoreDoc {
                        doc_id: pivot_doc,
                        score,
                    });

                    if top_k.len() > limit {
                        top_k.pop(); // Remove smallest
                    }

                    // Update threshold to current k-th best score
                    if top_k.len() == limit {
                        threshold = top_k.peek().map(|d| d.score).unwrap_or(0.0);
                    }
                }

                // Advance all cursors at pivot_doc
                for cursor in cursors[..end_idx].iter_mut() {
                    cursor.advance();
                }
            } else {
                // min_doc < pivot_doc: advance cursors at min_doc to pivot_doc
                // This skips documents that can't reach threshold
                for cursor in cursors.iter_mut() {
                    if cursor.current_doc_id() < pivot_doc {
                        cursor.advance_to(pivot_doc);
                    } else {
                        break; // Cursors are sorted, no more below pivot
                    }
                }
            }
        }

        // Extract results sorted by score descending
        let mut results: Vec<(u64, f64)> = top_k.into_iter().map(|d| (d.doc_id, d.score)).collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
        Ok(results)
    }

    /// Find the pivot index where cumulative upper bounds first exceed threshold
    fn find_pivot(&self, cursors: &[WandCursor], threshold: f64) -> Option<usize> {
        let mut cumulative = 0.0;
        for (i, cursor) in cursors.iter().enumerate() {
            cumulative += cursor.upper_bound;
            if cumulative >= threshold {
                return Some(i);
            }
        }
        None
    }

    /// Score a document using only the cursors that point to it
    fn score_document_at_cursors(
        &self,
        cursors: &[WandCursor],
        stats: &Bm25Stats,
        avg_dl: f64,
    ) -> f64 {
        let mut score = 0.0;
        for cursor in cursors {
            let tf = cursor.current_term_freq();
            let doc_id = cursor.current_doc_id();
            let tf_component = stats.calculate_tf_component(tf, doc_id, avg_dl);
            score += cursor.idf * tf_component;
        }
        score
    }
}

/// Cursor for iterating through a posting list during WAND
struct WandCursor<'a> {
    posting_list: &'a PostingList,
    position: usize,
    idf: f64,
    upper_bound: f64,
}

impl<'a> WandCursor<'a> {
    fn new(posting_list: &'a PostingList, idf: f64, upper_bound: f64) -> Self {
        Self {
            posting_list,
            position: 0,
            idf,
            upper_bound,
        }
    }

    #[inline]
    fn is_exhausted(&self) -> bool {
        self.position >= self.posting_list.doc_ids.len()
    }

    #[inline]
    fn current_doc_id(&self) -> u64 {
        if self.is_exhausted() {
            u64::MAX
        } else {
            self.posting_list.doc_ids[self.position]
        }
    }

    #[inline]
    fn current_term_freq(&self) -> u64 {
        self.posting_list.term_freqs[self.position]
    }

    #[inline]
    fn advance(&mut self) {
        self.position += 1;
    }

    /// Advance cursor to the first doc_id >= target
    fn advance_to(&mut self, target: u64) {
        // Use binary search for efficiency
        if self.is_exhausted() {
            return;
        }

        let remaining = &self.posting_list.doc_ids[self.position..];
        match remaining.binary_search(&target) {
            Ok(offset) => self.position += offset,
            Err(offset) => self.position += offset,
        }
    }
}

/// Wrapper for min-heap ordering (smallest score at top)
#[derive(PartialEq)]
struct MinScoreDoc {
    doc_id: u64,
    score: f64,
}

impl Eq for MinScoreDoc {}

impl PartialOrd for MinScoreDoc {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MinScoreDoc {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse order for min-heap (smallest score pops first)
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify WAND returns same results as exhaustive scoring
    #[test]
    fn test_wand_matches_exhaustive() {
        // Setup: Create BM25 scorer with some documents
        let tmpdir = tempfile::tempdir().unwrap();
        let db = sled::open(tmpdir.path()).unwrap();
        let stats_tree = db.open_tree("test_stats").unwrap();
        let scorer = BM25Scorer::open(stats_tree).unwrap();

        // Add documents with varying lengths
        let doc_lengths: HashMap<u64, u64> = [
            (0, 10),
            (1, 20),
            (2, 5),
            (3, 15),
            (4, 8),
            (5, 25),
            (6, 12),
            (7, 30),
            (8, 7),
            (9, 18),
        ]
        .into_iter()
        .collect();
        scorer.add_documents(&doc_lengths).unwrap();

        // Create posting lists for multiple terms
        let mut pl1 = PostingList::new();
        pl1.push(0, 3); // doc 0, tf=3
        pl1.push(2, 5); // doc 2, tf=5
        pl1.push(4, 2); // doc 4, tf=2
        pl1.push(7, 1); // doc 7, tf=1
        pl1.push(9, 4); // doc 9, tf=4

        let mut pl2 = PostingList::new();
        pl2.push(1, 2); // doc 1, tf=2
        pl2.push(2, 3); // doc 2, tf=3 (overlaps with pl1)
        pl2.push(5, 1); // doc 5, tf=1
        pl2.push(6, 4); // doc 6, tf=4
        pl2.push(9, 2); // doc 9, tf=2 (overlaps with pl1)

        let mut pl3 = PostingList::new();
        pl3.push(0, 1); // overlaps with pl1
        pl3.push(3, 2);
        pl3.push(8, 3);

        let postings: Vec<(usize, &PostingList)> =
            vec![(pl1.len(), &pl1), (pl2.len(), &pl2), (pl3.len(), &pl3)];

        // Get exhaustive results
        let exhaustive = scorer.score_documents(&postings).unwrap();
        let mut exhaustive_sorted: Vec<_> = exhaustive.into_iter().collect();
        exhaustive_sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Get WAND results for top-3
        let wand_top3 = scorer.score_documents_wand(&postings, 3).unwrap();

        // Verify WAND returns correct top-3
        assert_eq!(wand_top3.len(), 3);
        for (i, (doc_id, score)) in wand_top3.iter().enumerate() {
            assert_eq!(
                *doc_id, exhaustive_sorted[i].0,
                "Doc ID mismatch at rank {i}"
            );
            assert!(
                (score - exhaustive_sorted[i].1).abs() < 1e-10,
                "Score mismatch at rank {i}"
            );
        }

        // Verify WAND returns correct top-5
        let wand_top5 = scorer.score_documents_wand(&postings, 5).unwrap();
        assert_eq!(wand_top5.len(), 5);
        for (i, (doc_id, score)) in wand_top5.iter().enumerate() {
            assert_eq!(
                *doc_id, exhaustive_sorted[i].0,
                "Doc ID mismatch at rank {i}"
            );
            assert!(
                (score - exhaustive_sorted[i].1).abs() < 1e-10,
                "Score mismatch at rank {i}"
            );
        }
    }

    #[test]
    fn test_wand_empty_inputs() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = sled::open(tmpdir.path()).unwrap();
        let stats_tree = db.open_tree("test_stats").unwrap();
        let scorer = BM25Scorer::open(stats_tree).unwrap();

        // Empty posting lists
        let postings: Vec<(usize, &PostingList)> = vec![];
        let result = scorer.score_documents_wand(&postings, 10).unwrap();
        assert!(result.is_empty());

        // Zero limit
        let pl = PostingList::new();
        let postings = vec![(0, &pl)];
        let result = scorer.score_documents_wand(&postings, 0).unwrap();
        assert!(result.is_empty());
    }
}

#[derive(Serialize, Deserialize)]
pub struct Bm25Stats {
    pub doc_lengths: HashMap<u64, usize>,
    pub total_doc_length: f64,
    /// Cached average document length to avoid recomputation in hot scoring loop
    #[serde(skip, default = "default_avg_dl")]
    pub cached_avg_dl: f64,
    k1: f64,
    b: f64,
}

fn default_avg_dl() -> f64 {
    1.0
}

impl Bm25Stats {
    /// Recompute and cache the average document length
    #[inline]
    pub fn recompute_avg_dl(&mut self) {
        let num_docs = self.doc_lengths.len();
        self.cached_avg_dl = if num_docs > 0 {
            self.total_doc_length / num_docs as f64
        } else {
            1.0
        };
    }

    /// Calculate IDF component for a term given its document frequency
    #[inline]
    pub fn calculate_idf(&self, df: usize) -> f64 {
        let n = self.num_docs() as f64;
        let df = df as f64;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Calculate TF component for a document using pre-computed avg_dl
    #[inline]
    pub fn calculate_tf_component(&self, tf: u64, doc_id: u64, avg_dl: f64) -> f64 {
        let tf = tf as f64;
        let doc_len = self.doc_length(doc_id) as f64;
        tf * (self.k1 + 1.0) / (tf + self.k1 * (1.0 - self.b + self.b * (doc_len / avg_dl)))
    }

    /// Get the number of documents
    #[inline]
    pub fn num_docs(&self) -> usize {
        self.doc_lengths.len()
    }

    /// Get document length for a specific document
    #[inline]
    pub fn doc_length(&self, doc_id: u64) -> usize {
        self.doc_lengths.get(&doc_id).copied().unwrap_or(0)
    }
}

impl Default for Bm25Stats {
    fn default() -> Self {
        Self {
            doc_lengths: HashMap::default(),
            total_doc_length: 0.0,
            cached_avg_dl: 1.0,
            k1: DEFAULT_K1,
            b: DEFAULT_B,
        }
    }
}
