use crate::{
    consensus::{
        debuggables::DebuggableEntry, state::Persistent, ConsensusOperation, ConsensusState, Msg,
    },
    error::ConsensusError,
};
use raft::prelude::Entry;
use rand::Rng;
use std::{
    path::Path,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
};
use wal::Wal;

pub struct ConsensusManager {
    pub wal: Mutex<Wal>, // This is important to ensure ConsensusManager remains thread safe (i.e. Send + Sync)
    pub state: Arc<ConsensusState>,
    pub sender: Sender<Msg>,
}

impl ConsensusManager {
    pub fn init(path: &Path, state: Arc<ConsensusState>) -> (Self, Receiver<Msg>) {
        std::fs::create_dir_all(path).expect("Failed to create consensus storage directory");
        let wal = Mutex::new(Wal::open(path).expect("Failed to open consensus WAL"));
        let (sender, receiver) = channel::<Msg>();

        (ConsensusManager { wal, state, sender }, receiver)
    }

    pub fn wal(&self) -> std::sync::MutexGuard<'_, Wal> {
        self.wal.lock().expect("Failed to lock WAL")
    }

    /// For testing only
    pub fn num_entries(&self) -> raft::Result<usize> {
        let num_entries = self.wal().num_entries();
        Ok(num_entries as usize)
    }

    pub fn peek_consensus_wal(&self, n: usize) -> Vec<DebuggableEntry> {
        let wal = self.wal.lock().expect("Failed to lock WAL");
        let first_entry = wal.first_index();
        let mut results = vec![];
        for i in 0..n {
            if let Some(wal_entry) = wal.entry(first_entry + i as u64) {
                let raft_entry: Entry =
                    prost_for_raft::Message::decode(wal_entry.as_ref()).unwrap(); // todo: don't unwrap
                results.push(DebuggableEntry::from(&raft_entry));
            } else {
                break;
            }
        }
        results
    }

    pub async fn is_ready(&self) -> bool {
        let p = self.state.read_persistent();
        // Intentionally didn't make it a Option because raft crate sets individual fields at a time
        // and doing that will complicate the logic with no benefit.
        // Downside of using a badly designed library 😢
        !p.raft_info.role.is_empty()
    }

    pub async fn get_cluster_info(&self) -> Persistent {
        self.state.read_persistent().clone()
    }

    // pub async fn peek_consensus_wal(&self, n: usize) -> Vec<u8> {
    //     let wal = self.wal.lock().expect("Failed to lock WAL");
    //     let first_entry = wal.first_index();
    //     let results = vec![];
    //     for i in 0..n {
    //         if let Some(entry) = wal.entry(first_entry + i as u64) {
    //             results.extend_from_slice(&entry);
    //         } else {
    //             break;
    //         }
    //     }
    // }

    pub fn send(&self, msg: Msg) -> Result<(), ConsensusError> {
        self.sender.send(msg).map_err(|e| {
            ConsensusError::ServiceError(format!("Failed to send message over channel: {e}"))
        })
    }

    pub async fn propose_consensus_op(
        &self,
        operation: ConsensusOperation,
    ) -> Result<(), ConsensusError> {
        let mut rng = rand::rng();
        let id = rng.random();

        self.sender
            .send(Msg::Propose {
                id,
                operation,
                callback: Box::new(move || {
                    println!("Callback executed operation with ID {id}");
                }),
            })
            .map_err(|e| {
                ConsensusError::ServiceError(format!("Failed to propose consensus operation: {e}"))
            })?;

        Ok(())
    }
}
