use crate::{
    consensus::{ConsensusOperation, ConsensusState, Msg, Persistent},
    storage::error::ConsensusError,
};
use rand::Rng;
use std::sync::{mpsc::Sender, Arc};

pub struct ConsensusManager {
    pub state: Arc<ConsensusState>,
    pub sender: Sender<Msg>,
}

impl ConsensusManager {
    pub fn new(state: Arc<ConsensusState>, sender: Sender<Msg>) -> Self {
        ConsensusManager { state, sender }
    }

    pub async fn get_cluster_info(&self) -> Persistent {
        self.state.persistent.read().await.clone()
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
