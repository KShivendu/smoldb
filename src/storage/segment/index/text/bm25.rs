use ahash::HashMap;

use crate::{
    error::{StorageError, StorageResult},
    storage::index::text::posting::PostingListItem,
};

// Keys for persisting BM25 stats in sled
const STATS_DOC_LENGTHS_KEY: &[u8] = b"__bm25_doc_lengths__";
const STATS_TOTAL_DOC_LENGTH_KEY: &[u8] = b"__bm25_total_doc_length__";

/// BM25 scoring statistics - shared between disk and in-memory implementations
/// Persists its own stats via sled for durability
pub struct Bm25Stats {
    stats_tree: sled::Tree,
    doc_lengths: HashMap<u64, usize>,
    total_doc_length: f64,
    k1: f64,
    b: f64,
}

impl Bm25Stats {
    /// Create a new Bm25Stats instance, loading existing stats from sled
    pub fn new(stats_tree: sled::Tree) -> StorageResult<Self> {
        let mut stats = Self {
            stats_tree,
            doc_lengths: HashMap::default(),
            total_doc_length: 0.0,
            k1: 1.2,
            b: 0.75,
        };
        stats.load_from_disk()?;
        Ok(stats)
    }

    /// Load stats from sled persistence
    fn load_from_disk(&mut self) -> StorageResult<()> {
        // Load doc_lengths
        if let Some(data) = self.stats_tree.get(STATS_DOC_LENGTHS_KEY)? {
            let doc_lengths: Vec<(u64, usize)> =
                bincode::decode_from_slice(&data, bincode::config::standard())
                    .map(|(v, _)| v)
                    .map_err(|e| {
                        StorageError::CodecError(format!("Failed to decode doc_lengths: {e}"))
                    })?;
            self.doc_lengths = doc_lengths.into_iter().collect();
        }

        // Load total_doc_length
        if let Some(data) = self.stats_tree.get(STATS_TOTAL_DOC_LENGTH_KEY)? {
            self.total_doc_length = bincode::decode_from_slice(&data, bincode::config::standard())
                .map(|(v, _)| v)
                .map_err(|e| {
                    StorageError::CodecError(format!("Failed to decode total_doc_length: {e}"))
                })?;
        }

        Ok(())
    }

    /// Persist stats to sled
    fn persist(&self) -> StorageResult<()> {
        // Persist doc_lengths
        let doc_lengths_vec: Vec<(u64, usize)> =
            self.doc_lengths.iter().map(|(&k, &v)| (k, v)).collect();
        let encoded_doc_lengths =
            bincode::encode_to_vec(&doc_lengths_vec, bincode::config::standard()).map_err(|e| {
                StorageError::CodecError(format!("Failed to encode doc_lengths: {e}"))
            })?;
        self.stats_tree
            .insert(STATS_DOC_LENGTHS_KEY, encoded_doc_lengths)?;

        // Persist total_doc_length
        let encoded_total =
            bincode::encode_to_vec(self.total_doc_length, bincode::config::standard()).map_err(
                |e| StorageError::CodecError(format!("Failed to encode total_doc_length: {e}")),
            )?;
        self.stats_tree
            .insert(STATS_TOTAL_DOC_LENGTH_KEY, encoded_total)?;

        Ok(())
    }

    /// Get the number of documents
    #[inline]
    pub fn num_docs(&self) -> usize {
        self.doc_lengths.len()
    }

    /// Get the average document length
    #[inline]
    pub fn avg_doc_length(&self) -> f64 {
        let num_docs = self.num_docs();
        if num_docs > 0 {
            self.total_doc_length / num_docs as f64
        } else {
            1.0
        }
    }

    /// Get document length for a specific document
    #[inline]
    pub fn doc_length(&self, doc_id: u64) -> usize {
        self.doc_lengths.get(&doc_id).copied().unwrap_or(0)
    }

    /// Add or update a single document's stats
    pub fn add_document(&mut self, doc_id: u64, doc_length: u64) -> StorageResult<()> {
        let old_length = self.doc_lengths.get(&doc_id).copied().unwrap_or(0);
        let new_length = doc_length as usize;

        if old_length == 0 {
            // New document
            self.total_doc_length += new_length as f64;
        } else {
            // Update existing document
            self.total_doc_length += (new_length as f64) - (old_length as f64);
        }
        self.doc_lengths.insert(doc_id, new_length);

        self.persist()
    }

    /// Add or update multiple documents' stats in batch
    pub fn add_documents(&mut self, new_doc_lengths: &HashMap<u64, u64>) -> StorageResult<()> {
        for (&doc_id, &new_length) in new_doc_lengths {
            let old_length = self.doc_lengths.get(&doc_id).copied().unwrap_or(0);
            let new_length = new_length as usize;

            if old_length == 0 {
                self.total_doc_length += new_length as f64;
            } else {
                self.total_doc_length += (new_length as f64) - (old_length as f64);
            }
            self.doc_lengths.insert(doc_id, new_length);
        }

        self.persist()
    }

    /// Calculate IDF component for a term given its document frequency
    #[inline]
    pub fn calculate_idf(&self, df: usize) -> f64 {
        let n = self.num_docs() as f64;
        let df = df as f64;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Calculate TF component for a document
    #[inline]
    pub fn calculate_tf_component(&self, tf: u64, doc_id: u64) -> f64 {
        let tf = tf as f64;
        let doc_len = self.doc_length(doc_id) as f64;
        let avg_dl = self.avg_doc_length();
        tf * (self.k1 + 1.0) / (tf + self.k1 * (1.0 - self.b + self.b * (doc_len / avg_dl)))
    }

    /// Calculate BM25 scores for documents matching the given posting lists
    pub fn score_documents(
        &self,
        token_postings: &[(usize, &[PostingListItem])], // (df, posting_list) pairs
    ) -> HashMap<u64, f64> {
        let mut docs_with_scores: HashMap<u64, f64> = HashMap::default();

        for (df, posting_list) in token_postings {
            let idf = self.calculate_idf(*df);

            for item in *posting_list {
                let tf_component = self.calculate_tf_component(item.term_freq, item.doc_id);
                let score = idf * tf_component;
                *docs_with_scores.entry(item.doc_id).or_insert(0.0) += score;
            }
        }

        docs_with_scores
    }
}
