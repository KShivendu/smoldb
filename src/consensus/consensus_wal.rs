// use raft::prelude::*;
// use raft::{prelude::Entry, Storage};
// use slog::Result;
// use std::{path::Path, sync::RwLock};
// use wal::Wal;

// /// A wrapper around Wal to
// pub struct ConsensusWal {
//     wal: RwLock<Wal>,
// }

// impl ConsensusWal {
//     pub fn new(path: &Path) -> Result<Self> {
//         let wal = Wal::open(path)?;
//         Ok(ConsensusWal {
//             wal: RwLock::new(wal),
//         })
//     }

//     pub fn read(&self) -> std::sync::RwLockReadGuard<'_, Wal> {
//         self.wal.read().expect("Failed to lock WAL for reading")
//     }
// }

// impl Storage for ConsensusWal {
//     fn entries(
//         &self,
//         low: u64,
//         high: u64,
//         _max_size: impl Into<Option<u64>>, // todo: use params
//         _context: raft::GetEntriesContext, // todo: use params
//     ) -> raft::Result<Vec<Entry>> {
//         let mut entries = Vec::with_capacity((high - low) as usize);

//         for i in low..high {
//             let entry: Option<Entry> = self
//                 .read()
//                 .entry(i)
//                 .map(|entry| prost_for_raft::Message::decode(entry.as_ref()).unwrap()); // todo: don't unwrap

//             if let Some(entry) = entry {
//                 entries.push(entry);
//             } else {
//                 break;
//             }
//         }

//         Ok(entries)
//     }

//     fn first_index(&self) -> raft::Result<u64> {
//         Ok(self.read().first_index())
//     }

//     fn initial_state(&self) -> raft::Result<RaftState> {
//         //
//     }

//     fn last_index(&self) -> raft::Result<u64> {
//         Ok(self.read().last_index())
//     }

//     fn snapshot(&self, _request_index: u64, _to: u64) -> raft::Result<Snapshot> {
//         Err(raft::Error::Store(
//             raft::StorageError::SnapshotTemporarilyUnavailable,
//         ))
//     }

//     fn term(&self, idx: u64) -> raft::Result<u64> {
//         self.read()
//             .entry(idx)
//             .map(|entry| {
//                 let entry: Entry = prost_for_raft::Message::decode(entry.as_ref()).unwrap(); // todo: don't unwrap
//                 entry.term
//             })
//             .ok_or(raft::Error::Store(raft::StorageError::Unavailable))
//     }
// }

// impl Clone for ConsensusWal {
//     fn clone(&self) -> Self {
//         // ToDo: This is horrible workaround to clone the wal.
//         let wal = Wal::open(self.wal.path()).expect("Failed to clone ConsensusWal");
//         ConsensusWal { wal }
//     }
// }
