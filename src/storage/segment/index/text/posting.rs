use std::cmp::Ordering;

use ahash::HashMap;

use crate::error::{StorageError, StorageResult};

/// Posting list with Struct-of-Arrays (SoA) layout for better cache efficiency.
/// Doc IDs are stored separately from term frequencies, allowing faster iteration
/// when only doc IDs are needed (e.g., for intersection).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PostingList {
    /// Document IDs in sorted order
    pub doc_ids: Vec<u64>,
    /// Term frequencies corresponding to each doc_id (same length as doc_ids)
    pub term_freqs: Vec<u64>,
}

impl PostingList {
    /// Create a new empty posting list
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a posting list with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            doc_ids: Vec::with_capacity(capacity),
            term_freqs: Vec::with_capacity(capacity),
        }
    }

    /// Get the number of documents in this posting list (document frequency)
    #[inline]
    pub fn len(&self) -> usize {
        self.doc_ids.len()
    }

    /// Push a new entry to the posting list (must maintain sorted order)
    #[inline]
    pub fn push(&mut self, doc_id: u64, term_freq: u64) {
        self.doc_ids.push(doc_id);
        self.term_freqs.push(term_freq);
    }

    /// Insert at a specific position
    #[inline]
    pub fn insert(&mut self, pos: usize, doc_id: u64, term_freq: u64) {
        self.doc_ids.insert(pos, doc_id);
        self.term_freqs.insert(pos, term_freq);
    }

    /// Update term frequency at a specific position
    #[inline]
    pub fn update_term_freq(&mut self, pos: usize, term_freq: u64) {
        self.term_freqs[pos] = term_freq;
    }

    /// Binary search for a doc_id
    #[inline]
    pub fn binary_search(&self, doc_id: u64) -> Result<usize, usize> {
        self.doc_ids.binary_search(&doc_id)
    }

    /// Encode the posting list using delta encoding for doc_ids and VarInt compression.
    ///
    /// Format:
    /// - [varint] number of entries
    /// - [varint...] delta-encoded doc_ids (first is absolute, rest are deltas)
    /// - [varint...] term frequencies
    pub fn encode(&self) -> StorageResult<Vec<u8>> {
        let mut buf = Vec::new();
        let len = self.doc_ids.len();

        // Write length
        encode_varint(&mut buf, len as u64);

        if len == 0 {
            return Ok(buf);
        }

        // Write delta-encoded doc_ids
        let mut prev_doc_id = 0u64;
        for &doc_id in &self.doc_ids {
            let delta = doc_id - prev_doc_id;
            encode_varint(&mut buf, delta);
            prev_doc_id = doc_id;
        }

        // Write term frequencies (not delta-encoded, usually small values)
        for &tf in &self.term_freqs {
            encode_varint(&mut buf, tf);
        }

        Ok(buf)
    }

    /// Decode a posting list from delta-encoded VarInt format
    pub fn decode(data: &[u8]) -> StorageResult<Self> {
        let mut pos = 0;

        // Read length
        let (len, bytes_read) = decode_varint(&data[pos..]).ok_or_else(|| {
            StorageError::CodecError("Failed to decode posting list length".into())
        })?;
        pos += bytes_read;
        let len = len as usize;

        if len == 0 {
            return Ok(Self::new());
        }

        // Read delta-encoded doc_ids
        let mut doc_ids = Vec::with_capacity(len);
        let mut prev_doc_id = 0u64;
        for _ in 0..len {
            let (delta, bytes_read) = decode_varint(&data[pos..])
                .ok_or_else(|| StorageError::CodecError("Failed to decode doc_id delta".into()))?;
            pos += bytes_read;
            let doc_id = prev_doc_id + delta;
            doc_ids.push(doc_id);
            prev_doc_id = doc_id;
        }

        // Read term frequencies
        let mut term_freqs = Vec::with_capacity(len);
        for _ in 0..len {
            let (tf, bytes_read) = decode_varint(&data[pos..]).ok_or_else(|| {
                StorageError::CodecError("Failed to decode term frequency".into())
            })?;
            pos += bytes_read;
            term_freqs.push(tf);
        }

        Ok(Self {
            doc_ids,
            term_freqs,
        })
    }
}

/// Encode a u64 as a variable-length integer (VarInt/VLQ encoding).
/// Uses 7 bits per byte, with MSB indicating continuation.
#[inline]
fn encode_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8; // 0x7F = b0111_1111. SO it extract 7 bits from the end
        value >>= 7; // shift value since 7 bits have been extracted
        if value != 0 {
            byte |= 0x80; // Set continuation bit. 0x80 = b1000_0000. So it sets the most significant bit to 1.
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Decode a VarInt from a byte slice. Returns (value, bytes_consumed).
#[inline]
fn decode_varint(data: &[u8]) -> Option<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0;

    for (i, &byte) in data.iter().enumerate() {
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return None; // Overflow
        }
    }
    None // Incomplete
}

/// In-memory cache for posting lists
pub struct InMemPostings {
    postings: HashMap<String, PostingList>,
}

impl InMemPostings {
    pub fn new(tree: &sled::Tree) -> StorageResult<Self> {
        let mut postings: HashMap<String, PostingList> = HashMap::default();
        for item in tree.iter() {
            let (key, value) = item.map_err(|e| {
                StorageError::ServiceError(format!("Failed to read text index item: {e}"))
            })?;
            let term = String::from_utf8(key.to_vec()).map_err(|e| {
                StorageError::CodecError(format!("Failed to decode term {key:?}: {e}"))
            })?;
            let posting_list = PostingList::decode(&value)?;
            postings.insert(term, posting_list);
        }
        Ok(Self { postings })
    }

    pub fn get(&self, term: &str) -> Option<&PostingList> {
        self.postings.get(term)
    }

    pub fn insert(&mut self, term: String, posting_list: PostingList) {
        self.postings.insert(term, posting_list);
    }
}

/// Merge two sorted posting lists, preferring items from `new` when doc_ids collide
pub fn merge_sorted_posting_lists(existing: PostingList, new: PostingList) -> PostingList {
    let mut result = PostingList::with_capacity(existing.len() + new.len());
    let mut i = 0;
    let mut j = 0;

    while i < existing.len() && j < new.len() {
        match existing.doc_ids[i].cmp(&new.doc_ids[j]) {
            Ordering::Less => {
                result.push(existing.doc_ids[i], existing.term_freqs[i]);
                i += 1;
            }
            Ordering::Greater => {
                result.push(new.doc_ids[j], new.term_freqs[j]);
                j += 1;
            }
            Ordering::Equal => {
                // New takes precedence (update case)
                result.push(new.doc_ids[j], new.term_freqs[j]);
                i += 1;
                j += 1;
            }
        }
    }

    // Append remaining from existing
    while i < existing.len() {
        result.push(existing.doc_ids[i], existing.term_freqs[i]);
        i += 1;
    }

    // Append remaining from new
    while j < new.len() {
        result.push(new.doc_ids[j], new.term_freqs[j]);
        j += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_encoding() {
        // Test small values (1 byte)
        let mut buf = Vec::new();
        encode_varint(&mut buf, 0);
        assert_eq!(buf, vec![0]);

        buf.clear();
        encode_varint(&mut buf, 127);
        assert_eq!(buf, vec![127]);

        // Test 2-byte value
        buf.clear();
        encode_varint(&mut buf, 128);
        assert_eq!(buf, vec![0x80, 0x01]);

        buf.clear();
        encode_varint(&mut buf, 300);
        assert_eq!(buf, vec![0xAC, 0x02]); // 300 = 0b100101100 -> [0b10101100, 0b00000010]

        // Test round-trip for various values
        for value in [0, 1, 127, 128, 255, 256, 16383, 16384, u64::MAX] {
            buf.clear();
            encode_varint(&mut buf, value);
            let (decoded, _) = decode_varint(&buf).unwrap();
            assert_eq!(decoded, value, "Round-trip failed for {value}");
        }
    }

    #[test]
    fn test_posting_list_encoding() {
        // Empty list
        let list = PostingList::new();
        let encoded = list.encode().unwrap();
        let decoded = PostingList::decode(&encoded).unwrap();
        assert_eq!(decoded, list);

        // Single entry
        let mut list = PostingList::new();
        list.push(5, 3);
        let encoded = list.encode().unwrap();
        let decoded = PostingList::decode(&encoded).unwrap();
        assert_eq!(decoded, list);

        // Multiple entries with sequential doc_ids (good delta compression)
        let mut list = PostingList::new();
        list.push(100, 2);
        list.push(105, 1); // delta = 5
        list.push(108, 4); // delta = 3
        list.push(200, 1); // delta = 92
        let encoded = list.encode().unwrap();
        let decoded = PostingList::decode(&encoded).unwrap();
        assert_eq!(decoded, list);

        // Verify compression benefit: deltas [100, 5, 3, 92] use fewer bytes than [100, 105, 108, 200]
        assert!(encoded.len() < 4 * 8 * 2); // Much less than 8 bytes * 4 entries * 2 arrays
    }

    #[test]
    fn test_merge_sorted_posting_lists() {
        let mut existing = PostingList::new();
        existing.push(1, 2);
        existing.push(3, 1);
        existing.push(5, 3);

        let mut new = PostingList::new();
        new.push(2, 1);
        new.push(3, 5); // Updated
        new.push(4, 2);

        let merged = merge_sorted_posting_lists(existing, new);
        assert_eq!(merged.len(), 5);
        assert_eq!(merged.doc_ids, vec![1, 2, 3, 4, 5]);
        assert_eq!(merged.term_freqs[2], 5); // Should be updated value from new
    }

    #[test]
    fn test_posting_list_binary_search() {
        let mut list = PostingList::new();
        list.push(10, 1);
        list.push(20, 2);
        list.push(30, 3);

        assert_eq!(list.binary_search(10), Ok(0));
        assert_eq!(list.binary_search(20), Ok(1));
        assert_eq!(list.binary_search(30), Ok(2));
        assert_eq!(list.binary_search(15), Err(1)); // Would insert at position 1
        assert_eq!(list.binary_search(5), Err(0)); // Would insert at position 0
        assert_eq!(list.binary_search(35), Err(3)); // Would insert at position 3
    }
}
