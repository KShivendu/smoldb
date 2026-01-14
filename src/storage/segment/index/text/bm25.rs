use ahash::HashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::{
    error::{StorageError, StorageResult},
    storage::index::text::posting::PostingListItem,
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

    /// Calculate BM25 scores for documents matching the given posting lists
    pub fn score_documents(
        &self,
        token_postings: &[(usize, &[PostingListItem])], // (df, posting_list) pairs
    ) -> StorageResult<HashMap<u64, f64>> {
        // Rank completely from memory because it's faster
        let stats = self.bm25_stats.read();
        let avg_dl = stats.cached_avg_dl;
        let mut docs_with_scores: HashMap<u64, f64> = HashMap::default();

        for (df, posting_list) in token_postings {
            let idf = stats.calculate_idf(*df);

            for item in *posting_list {
                let tf_component =
                    stats.calculate_tf_component(item.term_freq, item.doc_id, avg_dl);
                let score = idf * tf_component;
                *docs_with_scores.entry(item.doc_id).or_insert(0.0) += score;
            }
        }

        Ok(docs_with_scores)
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
