use crate::{
    consensus::manager::ConsensusManager,
    error::{ConsensusError, ConsensusResult},
    types::PeerId,
};
use log::info;
use raft::{
    prelude::{Entry, Snapshot},
    storage::MemStorage,
    GetEntriesContext, RaftState, Storage,
};
use std::sync::Arc;

type RaftResult<T> = Result<T, raft::Error>;

#[derive(Clone)]
/// A thin layer on top of `MemStorage` & `ConsensusManager` to provide a consistent interface for Raft storage.
pub struct RaftStorage {
    mem_storage: Option<MemStorage>,
    consensus_manager: Arc<ConsensusManager>,
}

impl RaftStorage {
    pub fn new(
        peer_id: PeerId,
        consensus_manager: Arc<ConsensusManager>,
        init_mem_storage: bool,
    ) -> Self {
        let mem_storage =
            init_mem_storage.then(|| MemStorage::new_with_conf_state((vec![peer_id], vec![])));

        RaftStorage {
            mem_storage,
            consensus_manager,
        }
    }

    /// Apply a Raft snapshot to the Raft log and state
    pub fn apply_snapshot(&self, _snapshot: Snapshot) -> ConsensusResult<()> {
        // self.mem_storage.wl().apply_snapshot(snapshot)?;
        Err(ConsensusError::RaftError(raft::Error::Store(
            raft::StorageError::SnapshotTemporarilyUnavailable,
        )))
    }

    /// Append entries to the Raft log.
    ///
    /// Expects that entries are in order and must not have any gaps.
    pub fn append_entries(&self, entries: &[Entry]) -> ConsensusResult<()> {
        if entries.is_empty() {
            return Ok(());
        }

        info!("Appending {} entries to Raft log", entries.len());
        info!("Entries: {:?}", entries);

        if let Some(mem_storage) = &self.mem_storage {
            mem_storage.wl().append(entries)?;
        }

        {
            let mut wal = self.consensus_manager.wal();
            let mut buf = Vec::new();

            if entries[0].index < wal.first_index() {
                // This can happen when you use a snapshot (currently not supported, but adding for safety)
                return Err(ConsensusError::ServiceError(
                    "Cannot append entries that are older than existing entries in WAL".to_string(),
                ));
            }

            // If WAL is not empty, we expect new entry to have index exactly one more than last index in WAL
            // if wal.num_entries() != 0 && entries[0].index > wal.last_index() + 1 {
            //     return Err(ConsensusError::ServiceError(
            //         "Cannot append entries with gaps in index".to_string(),
            //     ));
            // }

            // Remove all entries that will be overwritten
            wal.truncate(entries[0].index)?;

            for entry in entries {
                buf.clear(); // reuse same buffer to avoid allocations
                prost_for_raft::Message::encode(entry, &mut buf).unwrap(); // todo: don't unwrap
                wal.append(&buf.as_slice())?;
            }
            wal.flush_open_segment()?;
        }

        // Try reading from both after appending
        // let low = entries.first().unwrap().index;
        // let high = entries.last().unwrap().index + 1;

        // info!(
        //     "Verifying appended entries between indices [{}, {})",
        //     low, high
        // );
        // let mem_entries =
        //     self.mem_storage
        //         .entries(low, high, None, GetEntriesContext::empty(false))?;
        // let disk_entries =
        //     self.consensus_manager
        //         .entries(low, high, None, GetEntriesContext::empty(false))?;

        // info!("MemStorage entries after append: {:?}", mem_entries);
        // info!("ConsensusManager entries after append: {:?}", disk_entries);

        Ok(())
    }

    /// Set the Raft hard state (term, vote, commit).
    pub fn set_hardstate(&self, hs: raft::eraftpb::HardState) -> ConsensusResult<()> {
        if let Some(mem_storage) = &self.mem_storage {
            mem_storage.wl().set_hardstate(hs.clone());
        }

        let mut persistent = self.consensus_manager.state.write_persistent();
        persistent.raft_state.hard_state = hs;
        persistent.save()?;
        Ok(())
    }

    /// Only set the commit index in the Raft hard state.
    ///
    /// This is helpful because unless election happens, only the commit index changes.
    pub fn set_hardstate_commit(&self, commit: u64) -> ConsensusResult<()> {
        if let Some(mem_storage) = &self.mem_storage {
            mem_storage.wl().mut_hard_state().set_commit(commit);
        }

        let mut persistent = self.consensus_manager.state.write_persistent();
        persistent.raft_state.hard_state.commit = commit;
        persistent.save()?;
        Ok(())
    }

    /// Set the Raft configuration state (nodes, voters, learners)
    pub fn set_conf_state(&self, conf_state: raft::eraftpb::ConfState) -> ConsensusResult<()> {
        if let Some(mem_storage) = &self.mem_storage {
            mem_storage.wl().set_conf_state(conf_state.clone());
        }

        let mut persistent = self.consensus_manager.state.write_persistent();
        persistent.raft_state.conf_state = conf_state;
        persistent.save()?;
        Ok(())
    }
}

impl Storage for RaftStorage {
    /// Initial state of the Raft node (hard state and configuration) when the node is started.
    fn initial_state(&self) -> RaftResult<RaftState> {
        let disk_res = self.consensus_manager.initial_state();

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.initial_state();
            dbg!(&mem_res, &disk_res);
        }

        disk_res
    }

    /// The entries in the Raft log between [low, high)
    ///
    /// This low & high are raft Entry index. low is inclusive, high is exclusive.
    /// max_size limits the total size of the returned entries but we ignore it for now.
    /// context provides additional information to allow async storage engines but we ignore it for now.
    fn entries(
        &self,
        low: u64,
        high: u64,
        max_size: impl Into<Option<u64>>,
        context: GetEntriesContext,
    ) -> RaftResult<Vec<Entry>> {
        let max_size: Option<u64> = max_size.into();
        dbg!(&low, &high, &max_size);

        // UNSAFE: This assumes GetEntriesContext is just a wrapper around the enum
        let context2 = unsafe { core::mem::transmute_copy(&context) };

        let disk_res = self.consensus_manager.entries(low, high, max_size, context);

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.entries(low, high, max_size, context2);
            dbg!(&mem_res, &disk_res);
        }

        disk_res
    }

    fn term(&self, idx: u64) -> RaftResult<u64> {
        let disk_res = self.consensus_manager.term(idx);

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.term(idx);
            dbg!(&mem_res, &disk_res);
        }

        disk_res
    }

    fn first_index(&self) -> RaftResult<u64> {
        let disk_res = self.consensus_manager.first_index();

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.first_index();
            dbg!(&mem_res, &disk_res);
        }

        disk_res
    }

    fn last_index(&self) -> RaftResult<u64> {
        let disk_res = self.consensus_manager.last_index();

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.last_index();
            dbg!(&mem_res, &disk_res);
        }

        disk_res
    }

    fn snapshot(&self, request_index: u64, to: u64) -> RaftResult<Snapshot> {
        // NOTE: disk_res is always going to be Err
        let disk_res = self.consensus_manager.snapshot(request_index, to);

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.snapshot(request_index, to);
            dbg!(&mem_res, &disk_res);
        }

        disk_res
    }
}

// Useful conversions between raft index and wal index because
// WAL index starts from 0
// But raft Entry index starts from 1
impl ConsensusManager {
    fn to_raft_index(&self, wal_index: u64) -> u64 {
        wal_index + 1
    }

    fn from_raft_index(&self, raft_index: u64) -> u64 {
        raft_index.saturating_sub(1)
    }
}

// Raft storage traits methods for consensus manager
impl Storage for ConsensusManager {
    fn initial_state(&self) -> Result<RaftState, raft::Error> {
        // let handle = tokio::runtime::Handle::current();
        // let persistent = handle
        //     .block_on(async move { self.state.persistent.read().await })
        //     .clone();
        // let persistent = self.state.persistent.blocking_read().clone();

        // let persistent = match self.state.persistent.try_read() {
        //     Ok(guard) => guard.clone(),
        //     Err(_) => {
        //         return Err(raft::Error::Store(
        //             raft::StorageError::Unavailable,
        //         ))
        //     }
        // };

        // Tries to create a new runtime to block on the async read. Which doesn't work because we can't have nested runtimes in Tokio
        // let rt = tokio::runtime::Handle::current();
        // let persistent = rt.block_on(async { self.state.persistent.read().await.clone() });

        // let persistent = tokio::runtime::Handle::current()
        //     .spawn_blocking(move || self.state.persistent.blocking_read().clone()).

        let persistent = self
            .state
            .persistent
            .read()
            .map_err(|_| {
                raft::Error::ConfigInvalid(format!("Failed to read persistent state from the lock"))
            })?
            .clone();

        Ok(persistent.into())
    }

    /// Fetches entries from the Raft log between [low, high).
    ///
    /// This low & high are raft Entry index
    fn entries(
        &self,
        low: u64,
        high: u64,
        _max_size: impl Into<Option<u64>>, // todo: use
        _context: raft::GetEntriesContext, // todo: use
    ) -> raft::Result<Vec<Entry>> {
        let mut entries = Vec::with_capacity((high - low) as usize);

        let low_wal_index = self.from_raft_index(low);
        let high_wal_index = self.from_raft_index(high);

        for i in low_wal_index..high_wal_index {
            if let Some(wal_entry) = self.wal().entry(i) {
                let raft_entry: Entry = prost_for_raft::Message::decode(wal_entry.as_ref())
                    .map_err(|e| {
                        // Use a more suitable error here
                        raft::Error::ConfigInvalid(format!(
                            "Failed to decode entry from WAL: {}",
                            e
                        ))
                    })?;

                entries.push(raft_entry);
            } else {
                // Raft expects wal to be continuous without gaps
                break;
            }
        }

        Ok(entries)
    }

    fn first_index(&self) -> raft::Result<u64> {
        let first_entry = self.wal().entry(0);
        if let Some(entry) = first_entry {
            let entry: Entry = prost_for_raft::Message::decode(entry.as_ref()).unwrap(); // todo: don't unwrap
            Ok(entry.index)
        } else {
            // If no entries, return 1 otherwise raft crate's RaftLog init will panic (because it subtracts 1 from first_index)
            Ok(1)
        }
    }

    fn last_index(&self) -> raft::Result<u64> {
        let wal_last_index = self.wal().last_index();
        Ok(self.to_raft_index(wal_last_index))
    }

    fn snapshot(&self, _request_index: u64, _to: u64) -> raft::Result<Snapshot> {
        Err(raft::Error::Store(
            raft::StorageError::SnapshotTemporarilyUnavailable,
        ))
    }

    fn term(&self, raft_index: u64) -> raft::Result<u64> {
        if self.wal().last_index() == 0 {
            // WAL is empty, so we assume term is 1 (default)
            return Ok(1);
        }

        // else if raft_index == 1 {
        //     // If index is 1, return term as 1 without even checking the WAL (first entry always has term 1)
        //     return Ok(1);
        // }

        let wal_index = self.from_raft_index(raft_index);
        info!("Fetching term for Raft index {raft_index} (wal index: {wal_index})");

        let wal_entry = self.wal().entry(wal_index);
        if let Some(entry) = wal_entry {
            let raft_entry: Entry = prost_for_raft::Message::decode(entry.as_ref())
                .expect("Failed to decode the Raft entry from WAL"); // todo: don't unwrap
            Ok(raft_entry.term)
        } else {
            // todo: use better error
            Err(raft::Error::ConfigInvalid(format!(
                "Unable to find term for raft index {}",
                raft_index
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::consensus::ConsensusState;

    use super::*;
    use http::Uri;
    use raft::prelude::EntryType;
    use tempfile::TempDir;

    #[test]
    fn test_raft_storage() {
        let temp_dir = TempDir::new().unwrap();

        // First attempt to create and use RaftStorage
        {
            let state = ConsensusState::new(Uri::from_static("0.0.0.0:5000"), Some(100));
            let (consensus_manager, _receiver) =
                ConsensusManager::init(temp_dir.path(), Arc::new(state));
            let raft_storage = RaftStorage::new(1, Arc::new(consensus_manager), true);

            // Set hard state
            let hard_state = raft::eraftpb::HardState {
                term: 1,
                commit: 0,
                vote: 0,
            };
            raft_storage.set_hardstate(hard_state.clone()).unwrap();

            // Append entries
            let entries = vec![
                Entry {
                    term: 1,  // election term where this entry was created
                    index: 1, // log index of this entry
                    entry_type: EntryType::EntryNormal.into(),
                    data: b"first entry".to_vec(),
                    ..Default::default()
                },
                Entry {
                    term: 1,
                    index: 2,
                    entry_type: EntryType::EntryNormal.into(),
                    data: b"second entry".to_vec(),
                    ..Default::default()
                },
            ];
            raft_storage.append_entries(&entries).unwrap();

            // Verify entries
            let fetched_entries = raft_storage
                .entries(1, 3, None, GetEntriesContext::empty(true))
                .unwrap();
            assert_eq!(fetched_entries.len(), 2);
            assert_eq!(fetched_entries[0].data, b"first entry");
            assert_eq!(fetched_entries[1].data, b"second entry");

            // Verify term
            let term = raft_storage.term(1).unwrap();
            assert_eq!(term, 1);

            // Verify first and last index
            let first_index = raft_storage.first_index().unwrap();
            let last_index = raft_storage.last_index().unwrap();
            assert_eq!(first_index, 1);
            assert_eq!(last_index, 2);
        }

        // Recreate to verify persistence
        {
            let state = ConsensusState::new(Uri::from_static("0.0.0.0:5000"), Some(100));
            let (consensus_manager, _receiver) =
                ConsensusManager::init(temp_dir.path(), Arc::new(state));
            let raft_storage = RaftStorage::new(1, Arc::new(consensus_manager), false);

            let fetched_entries = raft_storage
                .entries(1, 3, None, GetEntriesContext::empty(true))
                .unwrap();
            assert_eq!(fetched_entries.len(), 2);
            assert_eq!(fetched_entries[0].data, b"first entry");
            assert_eq!(fetched_entries[1].data, b"second entry");
            let term = raft_storage.term(1).unwrap();
            assert_eq!(term, 1);
            let first_index = raft_storage.first_index().unwrap();
            let last_index = raft_storage.last_index().unwrap();
            assert_eq!(first_index, 1);
            assert_eq!(last_index, 2);
        }
    }
}
