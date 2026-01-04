use std::cmp::Ordering;

use ahash::HashMap;
use bincode::{Decode, Encode};

use crate::error::{StorageError, StorageResult};

/// In-memory cache for posting lists (optional optimization)
pub struct InMemPostings {
    postings: HashMap<String, Vec<PostingListItem>>,
}

impl InMemPostings {
    pub fn new(tree: &sled::Tree) -> StorageResult<Self> {
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
        Ok(Self { postings })
    }

    pub fn get(&self, term: &str) -> Option<&Vec<PostingListItem>> {
        self.postings.get(term)
    }

    pub fn insert(&mut self, term: String, posting_list: Vec<PostingListItem>) {
        self.postings.insert(term, posting_list);
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Debug)]
pub struct PostingListItem {
    pub doc_id: u64,
    /// Number of times the term appears in the document
    pub term_freq: u64,
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

/// Merge two sorted posting lists, preferring items from `new` when doc_ids collide
pub fn merge_sorted_posting_lists(
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

#[cfg(test)]
mod tests {
    use super::*;

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
