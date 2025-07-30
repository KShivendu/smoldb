use crate::consensus::manager::ConsensusManager;
use crate::consensus::utils::add_peer_to_toc_and_consensus_state;
use crate::consensus::{ConsensusOperation, Msg};
use crate::error::{CollectionResult, ConsensusError};
use crate::storage::toc::{CollectionOperation, TableOfContent};
use crate::types::PeerId;
use http::Uri;
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
        if let Some(consensus) = &self.consensus {
            Ok(consensus)
        } else {
            Err(ConsensusError::NotEnabled)
        }
    }

    pub async fn get_peer_id(&self) -> Result<PeerId, ConsensusError> {
        Ok(self.get_consensus()?.state.get_peer_id().await)
    }

    pub async fn submit_collection_op(
        &self,
        operation: CollectionOperation,
    ) -> CollectionResult<()> {
        let Some(consensus) = &self.consensus else {
            // Do locally only if consensus is not enabled
            self.toc.perform_collection_op(operation).await?;
            return Ok(());
        };

        if !consensus.is_ready().await {
            return Err(ConsensusError::NotReady)?;
        }

        consensus
            .propose_consensus_op(ConsensusOperation::CollectionOp(operation.clone()))
            .await?;

        // ToDo: Should return the response of the consensus operation as result: true or acknowledged: true (if consensus)
        // ToDo: Add `wait` param to wait for the consensus operation to be applied across the cluster

        Ok(())
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
        // ToDo: Await Consensus::AddPeer consensus operation to be applied via ready_processor::handle_normal() to add remote replicas and other stuff instead of doing it forcefully here.
        let (this_peer_id, updated_peers) =
            add_peer_to_toc_and_consensus_state(&consensus.state, &self.toc, peer_id, uri, true)
                .await
                .map_err(|e| {
                    ConsensusError::ServiceError(format!(
                        "Failed to add new peer to local states and collections: {e}"
                    ))
                })?;

        Ok((this_peer_id, updated_peers))
    }
}
