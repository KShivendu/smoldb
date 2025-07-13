use crate::{
    consensus::{ConsensusOperation, ConsensusState, Msg},
    storage::{error::ConsensusError, toc::TableOfContent},
};
use std::sync::{mpsc::Sender, Arc};

pub struct ConsensusManager {
    toc: Arc<TableOfContent>,
    state: Arc<ConsensusState>,
    sender: Option<Sender<Msg>>,
}

impl ConsensusManager {
    pub fn new(
        toc: Arc<TableOfContent>,
        state: Arc<ConsensusState>,
        sender: Option<Sender<Msg>>,
    ) -> Self {
        ConsensusManager { toc, state, sender }
    }

    pub fn get_toc(&self) -> &Arc<TableOfContent> {
        &self.toc
    }

    pub fn get_state(&self) -> &Arc<ConsensusState> {
        &self.state
    }

    pub fn get_sender(&self) -> Result<&Sender<Msg>, ConsensusError> {
        self.sender.as_ref().ok_or(ConsensusError::NotEnabled())
    }

    pub async fn submit_consensus_op(
        &self,
        operation: ConsensusOperation,
    ) -> Result<(), ConsensusError> {
        let sender = self.get_sender()?;
        sender
            .send(Msg::Propose {
                id: 100, // Example ID, should be replaced with actual logic
                operation,
                callback: Box::new(|| println!("Callback executed for adding peer")),
            })
            .map_err(|e| {
                ConsensusError::ServiceError(format!("Failed to send consensus operation: {e}"))
            })?;

        Ok(())
    }
}
