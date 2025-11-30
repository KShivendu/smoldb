use crate::{
    consensus::{debuggables::DebuggableEntry, manager::ConsensusManager},
    error::{ConsensusError, ConsensusResult},
    types::PeerId,
};
use log::{info, trace};
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

        info!(
            "Appending {} entries to Raft log: {entries:?}",
            entries.len()
        );
        for entry in entries {
            DebuggableEntry::from(entry).log("Appending entry");
        }

        // Try reading from both after appending
        let low: u64 = entries.first().unwrap().index; // read from the start.
        let high: u64 = entries.last().unwrap().index + 1;

        // BEFORE UPDATE:
        let disk_res = self.consensus_manager.last_index()?;
        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.last_index()?;
            debug_assert_eq!(mem_res, disk_res);
            debug_assert_eq!(mem_res, low.saturating_sub(1_u64));
        }

        if let Some(mem_storage) = &self.mem_storage {
            mem_storage.wl().append(entries)?;
        }

        // We need to do equivalent of wl().append() for consensus manager (i.e. persistent wal storage)
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

        info!(
            "Verifying appended entries between indices [{}, {})",
            low, high
        );

        // AFTER UPDATE:

        let disk_entries =
            self.consensus_manager
                .entries(low, high, None, GetEntriesContext::empty(false))?;

        info!("ConsensusManager entries after append: {:?}", disk_entries);

        if let Some(mem_storage) = &self.mem_storage {
            let mem_entries =
                mem_storage.entries(low, high, None, GetEntriesContext::empty(false))?;
            info!("MemStorage entries after append: {:?}", mem_entries);

            debug_assert_eq!(mem_entries, disk_entries);
            debug_assert_eq!(mem_entries, entries);
        }

        let num_entries = self.num_entries()?;
        info!("Total number of entries after append: {}", num_entries);

        let dis_res = self.consensus_manager.first_index()?;
        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.first_index()?;
            debug_assert_eq!(mem_res, dis_res);
            debug_assert_eq!(mem_res, 1); // always remains 1 after first append because we never compact (for now)
        }

        let disk_res = self.consensus_manager.last_index()?;
        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.last_index()?;
            debug_assert_eq!(mem_res, high - 1);
            debug_assert_eq!(mem_res, disk_res); // mem: 1, disk: 0; data got appended to disk or not??
        }

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

    /// For testing only
    fn num_entries(&self) -> RaftResult<usize> {
        trace!("Fetching number of entries in Raft log");
        let disk_res = self.consensus_manager.num_entries()?;

        // Needs custom crate so disabled for now
        // if let Some(mem_storage) = &self.mem_storage {
        //     let mem_res = mem_storage.all_entries().len();
        //     debug_assert_eq!(mem_res, disk_res);
        //     return Ok(mem_res); // prefer in-memory state if available
        // }

        Ok(disk_res)
    }
}

impl Storage for RaftStorage {
    /// Initial state of the Raft node (hard state and configuration) when the node is initialized/restarted.
    fn initial_state(&self) -> RaftResult<RaftState> {
        trace!("Fetching initial Raft state");
        let disk_res = self.consensus_manager.initial_state()?;

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.initial_state()?;
            debug_assert_eq!(mem_res.conf_state, disk_res.conf_state);
            debug_assert_eq!(mem_res.hard_state, disk_res.hard_state);
            return Ok(mem_res); // prefer in-memory state if available
        }

        Ok(disk_res)
    }

    /// The entries in the Raft log between [low, high)
    ///
    /// This low & high are raft Entry index. low is inclusive, high is exclusive.
    /// max_size limits the total size of the returned entries but we ignore it for now.
    /// context provides additional information to allow async storage engines but we ignore it for now.
    ///
    /// low must be >= first_index or you get Compacted Error
    /// low >= 1 because raft Entry index starts from 1
    fn entries(
        &self,
        low: u64,
        high: u64,
        max_size: impl Into<Option<u64>>,
        context: GetEntriesContext,
    ) -> RaftResult<Vec<Entry>> {
        trace!("Fetching entries between indices [{}, {})", low, high);
        let max_size: Option<u64> = max_size.into();
        dbg!(&low, &high, &max_size); // called when actual raft storage is used and operations are applied

        // UNSAFE: This assumes GetEntriesContext is just a wrapper around the enum
        let context2 = unsafe { core::mem::transmute_copy(&context) };

        let disk_res = self.consensus_manager.entries(low, high, max_size, context);

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.entries(low, high, max_size, context2);
            debug_assert_eq!(mem_res, disk_res);
            return mem_res; // prefer in-memory state if available
        }

        disk_res
    }

    /// Term for the entry at the given index in the Raft log.
    fn term(&self, idx: u64) -> RaftResult<u64> {
        info!("Fetching term for index {idx}");
        // print_caller_stack();
        let disk_res = self.consensus_manager.term(idx);

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res: Result<u64, raft::Error> = mem_storage.term(idx);
            debug_assert_eq!(mem_res, disk_res);
            return mem_res; // prefer in-memory state if available
        }

        disk_res
    }

    /// Index of the first entry in the Raft log.
    fn first_index(&self) -> RaftResult<u64> {
        // trace!("Fetching first index");
        let disk_res = self.consensus_manager.first_index();

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.first_index();
            debug_assert_eq!(mem_res, disk_res);
            return mem_res; // prefer in-memory state if available
        }

        disk_res
    }

    /// Index of the last entry in the Raft log.
    fn last_index(&self) -> RaftResult<u64> {
        trace!("Fetching last index");
        let disk_res = self.consensus_manager.last_index()?;

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.last_index()?;
            debug_assert_eq!(mem_res, disk_res);
            return Ok(mem_res); // prefer in-memory state if available
        }

        Ok(disk_res)
    }

    fn snapshot(&self, request_index: u64, to: u64) -> RaftResult<Snapshot> {
        // NOTE: disk_res is always going to be Err
        let disk_res = self.consensus_manager.snapshot(request_index, to);

        if let Some(mem_storage) = &self.mem_storage {
            let mem_res = mem_storage.snapshot(request_index, to);
            debug_assert_eq!(mem_res, disk_res);
            return mem_res; // prefer in-memory state if available
        }

        disk_res
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
                raft::Error::ConfigInvalid(
                    "Failed to read persistent state from the lock".to_string(),
                )
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

        let low_wal_index = low.saturating_sub(1);
        let high_wal_index = high.saturating_sub(1);

        for i in low_wal_index..high_wal_index {
            if let Some(wal_entry) = self.wal().entry(i) {
                let raft_entry: Entry = prost_for_raft::Message::decode(wal_entry.as_ref())
                    .map_err(|e| {
                        // Use a more suitable error here
                        raft::Error::ConfigInvalid(format!("Failed to decode entry from WAL: {e}",))
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
        // Assume there's no offset in WAL. So we can just read the entry with idx=0.
        let first_entry = self.wal().entry(0);
        if let Some(entry) = first_entry {
            let entry: Entry = prost_for_raft::Message::decode(entry.as_ref()).unwrap(); // todo: don't unwrap
            Ok(entry.index)
        } else {
            // If no entries, return 1 otherwise raft crate's RaftLog init will panic (because it subtracts 1 from first_index)
            Ok(1)
        }
    }

    /// Raft log last index
    fn last_index(&self) -> raft::Result<u64> {
        let wal_last_index = self.wal().last_index();
        // self.wal.lock().unwrap().num_entries();
        let last_entry = self.wal().entry(wal_last_index);
        if let Some(entry) = last_entry {
            let entry: Entry = prost_for_raft::Message::decode(entry.as_ref()).unwrap(); // todo: don't unwrap
            Ok(entry.index)
        } else {
            // If no entries, return 0
            Ok(0)
        }
    }

    fn snapshot(&self, _request_index: u64, _to: u64) -> raft::Result<Snapshot> {
        Err(raft::Error::Store(
            raft::StorageError::SnapshotTemporarilyUnavailable,
        ))
    }

    fn term(&self, raft_index: u64) -> raft::Result<u64> {
        if raft_index == 0 {
            // Raft index starts from 1, so term for index 0 is always 0
            return Ok(0);
        }

        // if self.wal().num_entries() == 0 {
        //     warn!("WAL is empty, returning term as 0 for any index");
        //     // WAL is empty, so we assume term is 1 (default)
        //     return Ok(0);
        // }

        let wal_index = raft_index.saturating_sub(1);

        // else if raft_index == 1 {
        //     // If index is 1, return term as 1 without even checking the WAL (first entry always has term 1)
        //     return Ok(1);
        // }

        // let wal_index = Self::from_raft_index(raft_index);
        // info!("Fetching term for Raft index {raft_index} (wal index: {wal_index})");

        let wal_entry = self.wal().entry(wal_index);
        if let Some(entry) = wal_entry {
            let raft_entry: Entry = prost_for_raft::Message::decode(entry.as_ref())
                .expect("Failed to decode the Raft entry from WAL"); // todo: don't unwrap
            Ok(raft_entry.term)
        } else {
            // todo: use better error
            Err(raft::Error::ConfigInvalid(format!(
                "Unable to find term for raft index {raft_index}",
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
            let state =
                ConsensusState::new(temp_dir.path(), Uri::from_static("0.0.0.0:5000"), Some(100));
            let (consensus_manager, _receiver) =
                ConsensusManager::init(temp_dir.path(), Arc::new(state));
            let raft_storage = RaftStorage::new(100, Arc::new(consensus_manager), true);

            // Before appending, verify initial state
            let initial_state = raft_storage.initial_state().unwrap();
            assert_eq!(
                initial_state.hard_state,
                raft::eraftpb::HardState {
                    commit: 0,
                    term: 0,
                    vote: 0,
                }
            );
            assert_eq!(
                initial_state.conf_state,
                raft::eraftpb::ConfState {
                    voters: vec![100],
                    ..Default::default()
                }
            );

            // Check the entries are empty initially
            let first_index = raft_storage.first_index().unwrap();
            let last_index = raft_storage.last_index().unwrap();
            let last_index_term = raft_storage.term(last_index).unwrap();
            let num_entries = raft_storage.num_entries().unwrap();
            assert_eq!(first_index, 1);
            assert_eq!(last_index, 0);
            assert_eq!(last_index_term, 0);
            assert_eq!(num_entries, 0);

            // Set raft storage state
            // raft_storage
            //     .set_hardstate(initial_state.hard_state)
            //     .unwrap();
            // raft_storage
            //     .set_conf_state(initial_state.conf_state)
            //     .unwrap();

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
                Entry {
                    term: 1, // new election term
                    index: 3,
                    entry_type: EntryType::EntryNormal.into(),
                    data: b"third entry".to_vec(),
                    ..Default::default()
                },
            ];
            raft_storage.append_entries(&entries).unwrap();

            // Verify entries
            let fetched_entries = raft_storage
                .entries(1, 4, None, GetEntriesContext::empty(true))
                .unwrap();
            assert_eq!(fetched_entries.len(), 3);
            assert_eq!(fetched_entries[0].data, b"first entry");
            assert_eq!(fetched_entries[1].data, b"second entry");
            assert_eq!(fetched_entries[2].data, b"third entry");

            // Verify term
            let term = raft_storage.term(1).unwrap();
            assert_eq!(term, 1);

            // Verify first and last index
            let first_index = raft_storage.first_index().unwrap();
            let last_index = raft_storage.last_index().unwrap();
            let last_index_term = raft_storage.term(last_index).unwrap();
            let num_entries = raft_storage.num_entries().unwrap();
            assert_eq!(first_index, 1);
            assert_eq!(last_index, 3);
            assert_eq!(last_index_term, 1);
            assert_eq!(num_entries, 3);
        }

        // Reload consensus from disk to verify persistence
        {
            let state =
                ConsensusState::new(temp_dir.path(), Uri::from_static("0.0.0.0:5000"), Some(100));
            let (consensus_manager, _receiver) =
                ConsensusManager::init(temp_dir.path(), Arc::new(state));
            let raft_storage = RaftStorage::new(100, Arc::new(consensus_manager), false);

            let fetched_entries = raft_storage
                .entries(1, 4, None, GetEntriesContext::empty(true))
                .unwrap();
            assert_eq!(fetched_entries.len(), 3);
            assert_eq!(fetched_entries[0].data, b"first entry");
            assert_eq!(fetched_entries[1].data, b"second entry");
            assert_eq!(fetched_entries[2].data, b"third entry");
            let term = raft_storage.term(1).unwrap();
            assert_eq!(term, 1);
            let first_index = raft_storage.first_index().unwrap();
            let last_index = raft_storage.last_index().unwrap();
            let last_index_term = raft_storage.term(last_index).unwrap();
            assert_eq!(first_index, 1);
            assert_eq!(last_index, 3);
            assert_eq!(last_index_term, 1);
        }
    }
}
