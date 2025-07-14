use crate::{
    api::grpc::p2p_grpc_schema::{AllPeers, Peer},
    consensus::{ConsensusOperation, ConsensusState, Msg},
    storage::{error::ConsensusError, toc::TableOfContent},
    types::PeerId,
};
use http::Uri;
use raft::{
    prelude::{ConfChange, ConfChangeType},
    SoftState, StateRole,
};
use rand::Rng;
use std::sync::{mpsc::Sender, Arc};

pub struct ConsensusManager {
    toc: Arc<TableOfContent>,
    state: Arc<ConsensusState>,
    sender: Sender<Msg>,

    // Stores data about leader ID and Raft node role
    role_meta: Option<SoftState>,
}

impl ConsensusManager {
    pub fn new(toc: Arc<TableOfContent>, state: Arc<ConsensusState>, sender: Sender<Msg>) -> Self {
        ConsensusManager {
            toc,
            state,
            sender,
            role_meta: None,
        }
    }

    pub fn get_leader(&self) -> Option<PeerId> {
        self.role_meta.as_ref().map(|meta| meta.leader_id)
    }

    pub fn get_role(&self) -> Option<StateRole> {
        self.role_meta.as_ref().map(|meta| meta.raft_state)
    }

    pub fn get_toc(&self) -> &Arc<TableOfContent> {
        &self.toc
    }

    pub fn get_state(&self) -> &Arc<ConsensusState> {
        &self.state
    }

    pub fn get_sender(&self) -> Result<&Sender<Msg>, ConsensusError> {
        Ok(&self.sender) // .ok_or(ConsensusError::NotEnabled())
    }

    pub async fn add_peer(&self, peer_id: PeerId, uri: Uri) -> Result<AllPeers, ConsensusError> {
        self.state
            .add_peer(peer_id, uri.clone())
            .await
            .map_err(|e| {
                ConsensusError::ServiceError(format!("Failed to add peer to local state: {e}"))
            })?;

        {
            let mut conf_change = ConfChange::default();
            conf_change.set_node_id(peer_id);
            conf_change.set_change_type(ConfChangeType::AddNode);

            let id = 99; // ToDo: Replace with actual logic to generate a unique ID

            let _res = self.sender.send(Msg::Propose {
                id,
                operation: ConsensusOperation::AddPeer {
                    peer_id,
                    uri: uri.to_string(),
                },
                callback: Box::new(move || {
                    println!("Callback executed operation with ID {id}");
                }),
            });
        }

        // ToDo: We shouldn't modify ToC collections here. But I'm doing it for now
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

        let persistent = self.state.persistent.read().await.clone();

        let all_peers = persistent
            .peers
            .into_iter()
            .map(|(id, uri)| Peer { id, uri })
            .collect();

        let this_peer_id = persistent.peer_id;

        Ok(AllPeers {
            all_peers,
            first_peer_id: this_peer_id,
        })
    }

    pub async fn propose_consensus_op(
        &self,
        operation: ConsensusOperation,
    ) -> Result<(), ConsensusError> {
        let mut rng = rand::rng();
        let id = rng.random::<u8>();

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
