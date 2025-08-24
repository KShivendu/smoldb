use crate::{consensus::manager::ConsensusManager, types::PeerId};
use raft::{
    prelude::{Entry, Snapshot},
    storage::MemStorage,
    GetEntriesContext, RaftState, Storage,
};
use std::sync::Arc;

type RaftResult<T> = Result<T, raft::Error>;

#[derive(Clone)]
/// A thin layer on top of `MemStorage` to provide a consistent interface for Raft storage.
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

    pub fn apply_snapshot(&self, snapshot: Snapshot) -> RaftResult<()> {
        self.mem_storage.wl().apply_snapshot(snapshot)
        // let wal = self.consensus_manager.wal();
        // ToDo: Apply snapshot to disk storage as well

        // Ok(())
    }

    /// Append entries to the Raft log.
    pub fn append_entries(&self, entries: &[Entry]) -> RaftResult<()> {
        self.mem_storage.wl().append(entries)
        // let wal = self.consensus_manager.wal();
        // for entry in entries {
        //     let mut buf = Vec::with_capacity(entry.encoded_len());
        //     prost_for_raft::Message::encode(entry, &mut buf).unwrap(); // todo: don't unwrap
        //     wal.append(buf);
        // }

        // Ok(())
    }

    pub fn set_hardstate(&self, hs: raft::eraftpb::HardState) {
        self.mem_storage.wl().set_hardstate(hs)

        // let wal = self.consensus_manager.wal();
        // wal.set_hardstate(hs);
    }

    pub fn set_hardstate_commit(&self, commit: u64) {
        self.mem_storage.wl().mut_hard_state().set_commit(commit)

        // let wal = self.consensus_manager.wal();
        // wal.set_hardstate_commit(commit);
    }

    pub fn set_conf_state(&self, conf_state: raft::eraftpb::ConfState) {
        self.mem_storage.wl().set_conf_state(conf_state)

        // let wal = self.consensus_manager.wal();
        // wal.set_conf_state(conf_state);
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

        let mem_res = self.mem_storage.entries(low, high, max_size, context);
        let disk_res = self
            .consensus_manager
            .entries(low, high, max_size, context2);

        dbg!(&mem_res, &disk_res);

        mem_res
    }

    fn term(&self, idx: u64) -> RaftResult<u64> {
        let mem_res = self.mem_storage.term(idx);
        let disk_res = self.consensus_manager.term(idx);

        dbg!(&mem_res, &disk_res);

        mem_res
    }

    fn first_index(&self) -> RaftResult<u64> {
        // self.mem_storage.first_index()
        let mem_res = self.mem_storage.first_index();
        let disk_res = self.consensus_manager.first_index();

        dbg!(&mem_res, &disk_res);

        mem_res
    }

    fn last_index(&self) -> RaftResult<u64> {
        let mem_res = self.mem_storage.last_index();
        let disk_res = self.consensus_manager.last_index();

        dbg!(&mem_res, &disk_res);

        mem_res
    }

    fn snapshot(&self, request_index: u64, to: u64) -> RaftResult<Snapshot> {
        let mem_res = self.mem_storage.snapshot(request_index, to);
        let _disk_res = self.consensus_manager.snapshot(request_index, to);

        // disk_res is always going to be Err

        mem_res
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
