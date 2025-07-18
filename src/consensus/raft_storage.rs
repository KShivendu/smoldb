use crate::types::PeerId;
use raft::{
    prelude::{Entry, Snapshot},
    storage::MemStorage,
    GetEntriesContext, RaftState, Storage,
};

type RaftResult<T> = Result<T, raft::Error>;

#[derive(Clone)]
/// A thin layer on top of `MemStorage` to provide a consistent interface for Raft storage.
pub struct RaftStorage {
    mem_storage: MemStorage,
}

impl RaftStorage {
    pub fn new(peer_id: PeerId) -> Self {
        let mem_storage = MemStorage::new_with_conf_state((vec![peer_id], vec![]));
        RaftStorage { mem_storage }
    }

    pub fn apply_snapshot(&self, snapshot: Snapshot) -> RaftResult<()> {
        self.mem_storage.wl().apply_snapshot(snapshot)
    }

    /// Append entries to the Raft log.
    pub fn append_entries(&self, entries: &[Entry]) -> RaftResult<()> {
        self.mem_storage.wl().append(entries)
    }

    pub fn set_hardstate(&self, hs: raft::eraftpb::HardState) {
        self.mem_storage.wl().set_hardstate(hs)
    }

    pub fn set_hardstate_commit(&self, commit: u64) {
        self.mem_storage.wl().mut_hard_state().set_commit(commit)
    }

    pub fn set_conf_state(&self, conf_state: raft::eraftpb::ConfState) {
        self.mem_storage.wl().set_conf_state(conf_state)
    }
}

impl Storage for RaftStorage {
    fn initial_state(&self) -> RaftResult<RaftState> {
        self.mem_storage.initial_state()
    }

    fn entries(
        &self,
        low: u64,
        high: u64,
        max_size: impl Into<Option<u64>>,
        context: GetEntriesContext,
    ) -> RaftResult<Vec<Entry>> {
        let max_size: Option<u64> = max_size.into();
        self.mem_storage.entries(low, high, max_size, context)
    }

    fn term(&self, idx: u64) -> RaftResult<u64> {
        self.mem_storage.term(idx)
    }

    fn first_index(&self) -> RaftResult<u64> {
        self.mem_storage.first_index()
    }

    fn last_index(&self) -> RaftResult<u64> {
        self.mem_storage.last_index()
    }

    fn snapshot(&self, request_index: u64, to: u64) -> RaftResult<Snapshot> {
        self.mem_storage.snapshot(request_index, to)
    }
}
