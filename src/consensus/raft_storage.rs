use crate::{
    consensus::manager::ConsensusManager,
    error::{ConsensusError, ConsensusResult},
    types::PeerId,
};
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
    mem_storage: MemStorage,
    consensus_manager: Arc<ConsensusManager>,
}

impl RaftStorage {
    pub fn new(peer_id: PeerId, consensus_manager: Arc<ConsensusManager>) -> Self {
        let mem_storage = MemStorage::new_with_conf_state((vec![peer_id], vec![]));
        RaftStorage {
            mem_storage,
            consensus_manager,
        }
    }

    pub fn apply_snapshot(&self, _snapshot: Snapshot) -> ConsensusResult<()> {
        // self.mem_storage.wl().apply_snapshot(snapshot)?;
        Err(ConsensusError::RaftError(raft::Error::Store(
            raft::StorageError::SnapshotTemporarilyUnavailable,
        )))
    }

    /// Append entries to the Raft log.
    pub fn append_entries(&self, entries: &[Entry]) -> ConsensusResult<()> {
        if entries.is_empty() {
            return Ok(());
        }

        self.mem_storage.wl().append(entries)?;

        let mut wal = self.consensus_manager.wal();
        let mut buf = Vec::new();

        for entry in entries {
            buf.clear(); // reuse same buffer to avoid allocations
            prost_for_raft::Message::encode(entry, &mut buf).unwrap(); // todo: don't unwrap
            wal.append(&buf.as_slice())?;
        }
        Ok(())
    }

    pub fn set_hardstate(&self, hs: raft::eraftpb::HardState) -> ConsensusResult<()> {
        self.mem_storage.wl().set_hardstate(hs.clone());

        let mut persistent = self.consensus_manager.state.write_persistent();
        persistent.raft_state.hard_state = hs;
        persistent.save()?;
        Ok(())
    }

    pub fn set_hardstate_commit(&self, commit: u64) -> ConsensusResult<()> {
        self.mem_storage.wl().mut_hard_state().set_commit(commit);

        let mut persistent = self.consensus_manager.state.write_persistent();
        persistent.raft_state.hard_state.commit = commit;
        persistent.save()?;
        Ok(())
    }

    pub fn set_conf_state(&self, conf_state: raft::eraftpb::ConfState) -> ConsensusResult<()> {
        self.mem_storage.wl().set_conf_state(conf_state.clone());

        let mut persistent = self.consensus_manager.state.write_persistent();
        persistent.raft_state.conf_state = conf_state;
        persistent.save()?;
        Ok(())
    }
}

impl Storage for RaftStorage {
    fn initial_state(&self) -> RaftResult<RaftState> {
        // self.mem_storage.initial_state()
        self.consensus_manager.initial_state()
    }

    fn entries(
        &self,
        low: u64,
        high: u64,
        max_size: impl Into<Option<u64>>,
        context: GetEntriesContext,
    ) -> RaftResult<Vec<Entry>> {
        let max_size: Option<u64> = max_size.into();

        // UNSAFE: This assumes GetEntriesContext is just a wrapper around the enum
        let context2 = unsafe { core::mem::transmute_copy(&context) };

        let mem_res = self.mem_storage.entries(low, high, max_size, context2);
        let disk_res = self.consensus_manager.entries(low, high, max_size, context);

        dbg!(&mem_res, &disk_res);

        disk_res
    }

    fn term(&self, idx: u64) -> RaftResult<u64> {
        let mem_res = self.mem_storage.term(idx);
        let disk_res = self.consensus_manager.term(idx);

        dbg!(&mem_res, &disk_res);

        disk_res
    }

    fn first_index(&self) -> RaftResult<u64> {
        // self.mem_storage.first_index()
        let mem_res = self.mem_storage.first_index();
        let disk_res = self.consensus_manager.first_index();

        dbg!(&mem_res, &disk_res);

        disk_res
    }

    fn last_index(&self) -> RaftResult<u64> {
        let mem_res = self.mem_storage.last_index();
        let disk_res = self.consensus_manager.last_index();

        dbg!(&mem_res, &disk_res);

        disk_res
    }

    fn snapshot(&self, request_index: u64, to: u64) -> RaftResult<Snapshot> {
        let _mem_res = self.mem_storage.snapshot(request_index, to);
        // NOTE: disk_res is always going to be Err
        self.consensus_manager.snapshot(request_index, to)
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
            .map_err(|_| raft::Error::Store(raft::StorageError::Unavailable))?
            .clone();

        Ok(persistent.into())
    }

    fn entries(
        &self,
        low: u64,
        high: u64,
        _max_size: impl Into<Option<u64>>, // todo: use
        _context: raft::GetEntriesContext, // todo: use
    ) -> raft::Result<Vec<Entry>> {
        let mut entries = Vec::with_capacity((high - low) as usize);

        for i in low..high {
            let entry: Option<Entry> = self
                .wal()
                .entry(i)
                .map(|entry| prost_for_raft::Message::decode(entry.as_ref()).unwrap()); // todo: don't unwrap

            if let Some(entry) = entry {
                entries.push(entry);
            } else {
                break;
            }
        }

        Ok(entries)
    }

    fn first_index(&self) -> raft::Result<u64> {
        Ok(self.wal().first_index())
    }

    fn last_index(&self) -> raft::Result<u64> {
        Ok(self.wal().last_index())
    }

    fn snapshot(&self, _request_index: u64, _to: u64) -> raft::Result<Snapshot> {
        Err(raft::Error::Store(
            raft::StorageError::SnapshotTemporarilyUnavailable,
        ))
    }

    fn term(&self, idx: u64) -> raft::Result<u64> {
        self.wal()
            .entry(idx)
            .map(|entry| {
                let entry: Entry = prost_for_raft::Message::decode(entry.as_ref()).unwrap(); // todo: don't unwrap
                entry.term
            })
            .ok_or(raft::Error::Store(raft::StorageError::Unavailable))
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
            let raft_storage = RaftStorage::new(1, Arc::new(consensus_manager));

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
                    term: 1,
                    index: 1,
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
                .entries(0, 10, None, GetEntriesContext::empty(true))
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
            assert_eq!(first_index, 0);
            assert_eq!(last_index, 1);
        }

        // Recreate to verify persistence
        {
            let state = ConsensusState::new(Uri::from_static("0.0.0.0:5000"), Some(100));
            let (consensus_manager, _receiver) =
                ConsensusManager::init(temp_dir.path(), Arc::new(state));
            let raft_storage = RaftStorage::new(1, Arc::new(consensus_manager));

            let fetched_entries = raft_storage
                .entries(0, 2, None, GetEntriesContext::empty(true))
                .unwrap();
            assert_eq!(fetched_entries.len(), 2);
            assert_eq!(fetched_entries[0].data, b"first entry");
            assert_eq!(fetched_entries[1].data, b"second entry");
            let term = raft_storage.term(1).unwrap();
            assert_eq!(term, 1);
            let first_index = raft_storage.first_index().unwrap();
            let last_index = raft_storage.last_index().unwrap();
            assert_eq!(first_index, 0);
            assert_eq!(last_index, 1);
        }
    }
}
