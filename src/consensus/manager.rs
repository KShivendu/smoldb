use crate::{
    api::grpc::p2p_grpc_schema::{AllPeers, Peer},
    consensus::{ConsensusOperation, ConsensusState, Msg, Persistent},
    storage::{error::ConsensusError, toc::TableOfContent},
    types::PeerId,
};
use http::Uri;
use rand::Rng;
use std::sync::{mpsc::Sender, Arc};

pub struct ConsensusManager {
    toc: Arc<TableOfContent>,
    state: Arc<ConsensusState>,
    sender: Sender<Msg>,
}

impl ConsensusManager {
    pub fn new(toc: Arc<TableOfContent>, state: Arc<ConsensusState>, sender: Sender<Msg>) -> Self {
        ConsensusManager { toc, state, sender }
    }

    pub fn get_state(&self) -> &Arc<ConsensusState> {
        &self.state
    }

    pub async fn get_cluster_info(&self) -> Persistent {
        self.state.persistent.read().await.clone()
    }

    pub fn get_sender(&self) -> Result<&Sender<Msg>, ConsensusError> {
        Ok(&self.sender) // .ok_or(ConsensusError::NotEnabled())
    }

    /// Adds a peer to the local consensus state and notifies the Raft consensus loop.
    ///
    /// Not cancel safe
    pub async fn add_peer(&self, peer_id: PeerId, uri: Uri) -> Result<AllPeers, ConsensusError> {
        // First notify the Raft consensus about the new peer

        // ToDo: Why is this random ID generation not allowed by RaftService's trait??
        // If you uncomment it will start throwing error
        // let mut rng = rand::rng();
        // let operation_id = rng.random();

        let operation_id = 100;

        // ToDo: This doesn't notify all existing nodes. Only leader knows about the new peer
        // while the new peer knows about everyone. And consensus seems to be working fine.
        let _res = self.sender.send(Msg::Propose {
            id: operation_id,
            operation: ConsensusOperation::AddPeer {
                peer_id,
                uri: uri.to_string(),
            },
            callback: Box::new(move || {
                println!("Callback executed operation with ID {operation_id}");
            }),
        });
        // ToDo: Await adding the peer to the Raft consensus?

        // ToDo: Program can crash any time so we should recover from this error or make all this atomic

        // Update local consensus state
        let (this_peer_id, updated_peers) = self
            .state
            .add_peer(peer_id, uri.clone())
            .await
            .map_err(|e| {
                ConsensusError::ServiceError(format!("Failed to add peer to local state: {e}"))
            })?;

        // ToDo: We shouldn't modify ToC collections here. But I'm doing it for now
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

        Ok(AllPeers {
            all_peers: updated_peers
                .into_iter()
                .map(|(id, uri)| Peer { id, uri })
                .collect(),
            first_peer_id: this_peer_id,
        })
    }

    pub async fn propose_consensus_op(
        &self,
        operation: ConsensusOperation,
    ) -> Result<(), ConsensusError> {
        let mut rng = rand::rng();
        let id = rng.random();

        let sender = self.get_sender()?;
        sender
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
