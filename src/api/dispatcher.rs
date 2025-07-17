use crate::consensus::manager::ConsensusManager;
use crate::consensus::ConsensusOperation;
use crate::storage::error::{CollectionResult, ConsensusError};
use crate::storage::toc::{CollectionOperation, TableOfContent};
use std::sync::Arc;

/// Router that can decide how an operation/request goes through ToC (local storage) and consensus manager (if enabled)
pub struct Dispatcher {
    pub toc: Arc<TableOfContent>,
    pub consensus: Option<Arc<ConsensusManager>>,
}

impl Dispatcher {
    pub fn from(toc: Arc<TableOfContent>, consensus: Option<Arc<ConsensusManager>>) -> Self {
        Dispatcher { toc, consensus }
    }

    /// Get the consensus manager if it exists, otherwise return [`ConsensusError::NotEnabled`].
    pub fn get_consensus(&self) -> Result<&Arc<ConsensusManager>, ConsensusError> {
        if let Some(consensus_manager) = &self.consensus {
            Ok(consensus_manager)
        } else {
            Err(ConsensusError::NotEnabled)
        }
    }

    pub async fn submit_collection_op(
        &self,
        operation: CollectionOperation,
    ) -> CollectionResult<()> {
        let Some(consensus_manager) = &self.consensus else {
            // Do locally only if consensus is not enabled
            self.toc.perform_collection_op(operation).await?;
            return Ok(());
        };

        // ToDo: Await consensus operations before committing locally
        consensus_manager
            .propose_consensus_op(ConsensusOperation::CollectionOp(operation.clone()))
            .await?;
        self.toc.perform_collection_op(operation).await?;

        Ok(())
    }

    // Send a consensus operation to the consensus manager
    pub async fn send_operation(&self, operation: ConsensusOperation) -> CollectionResult<()> {
        let consensus = self.get_consensus()?;
        Ok(consensus.propose_consensus_op(operation).await?)
    }
}
