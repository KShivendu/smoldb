use crate::{
    consensus::{ConsensusOperation, ConsensusState, Msg, Persistent},
    error::ConsensusError,
};
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

// unsafe impl Sync for ConsensusManager {}
// unsafe impl Send for ConsensusManager {}

impl ConsensusManager {
    pub fn init(path: &Path, state: Arc<ConsensusState>) -> (Self, Receiver<Msg>) {
        std::fs::create_dir_all(path).expect("Failed to create consensus storage directory");
        let wal = Mutex::new(Wal::open(path).expect("Failed to open consensus WAL"));
        // let wal = ConsensusWal::new(path).expect("Failed to open consensus WAL");
        let (sender, receiver) = channel::<Msg>();

        (ConsensusManager { wal, state, sender }, receiver)
    }

    pub fn wal(&self) -> std::sync::MutexGuard<'_, Wal> {
        self.wal.lock().expect("Failed to lock WAL")
    }

    pub async fn is_ready(&self) -> bool {
        let p = self.state.persistent.read().await;
        // Intentionally didn't make it a Option because raft crate sets individual fields at a time
        // and doing that will complicate the logic with no benefit.
        // Downside of using a badly designed library 😢
        !p.raft_info.role.is_empty()
    }

    pub async fn get_cluster_info(&self) -> Persistent {
        self.state.persistent.read().await.clone()
    }

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
