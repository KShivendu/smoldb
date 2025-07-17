use http::Uri;

use crate::consensus::manager::ConsensusManager;
use crate::consensus::{ConsensusOperation, Msg};
use crate::storage::error::{CollectionResult, ConsensusError};
use crate::storage::toc::{CollectionOperation, TableOfContent};
use crate::types::PeerId;
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

    /// Adds a peer to ToC and Consensus.
    /// local consensus state, updates remote shards, and notifies the Raft consensus loop.
    ///
    /// Not cancel safe
    pub async fn add_peer(
        &self,
        peer_id: PeerId,
        uri: Uri,
    ) -> Result<(PeerId, Vec<(PeerId, String)>), ConsensusError> {
        // ToDo: Await adding the peer to the Raft consensus?
        // ToDo: Program can crash any time so we should recover from this error or make all this atomic
        // ToDo: Why is this random ID generation not allowed by RaftService's trait??
        // If you uncomment it will start throwing error
        // let mut rng = rand::rng();
        // let operation_id = rng.random();

        // ToDo: This doesn't notify all existing nodes. Only leader knows about the new peer
        // while the new peer knows about everyone. And consensus seems to be working fine.

        // Propose the operation in to the consensus loop
        let operation_id = 100;
        let consensus = self.get_consensus()?;
        let _res = consensus.sender.send(Msg::Propose {
            id: operation_id,
            operation: ConsensusOperation::AddPeer {
                peer_id,
                uri: uri.to_string(),
            },
            callback: Box::new(move || {
                println!("Callback executed operation with ID {operation_id}");
            }),
        });

        // Update local consensus state
        let (this_peer_id, updated_peers) = consensus
            .state
            .add_peer(peer_id, uri.clone())
            .await
            .map_err(|e| {
                ConsensusError::ServiceError(format!("Failed to add peer to local state: {e}"))
            })?;

        {
            let collections_guard = self.toc.collections.write().await;
            for (collection_name, collection) in collections_guard.iter() {
                let mut replica_holder_guard = collection.replica_holder.write().await;

                replica_holder_guard
                    .add_remote_shards(peer_id, collection_name.clone())
                    .await
                    .map_err(|e| {
                        ConsensusError::ServiceError(format!(
                            "Failed to add remote shards for collection '{collection_name}': {e}",
                        ))
                    })?;
            }
        }

        Ok((this_peer_id, updated_peers))
    }
}
