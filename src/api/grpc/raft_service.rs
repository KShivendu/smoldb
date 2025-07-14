use crate::{
    api::grpc::p2p_grpc_schema::{
        raft_server::Raft, AddPeerToKnownMessage, AllPeers, PeerId,
        RaftMessage as RaftMessageBytes, Uri,
    },
    consensus::{self, manager::ConsensusManager},
    storage::toc::TableOfContent,
};
use prost_for_raft::Message as ProtocolBufferMessage; // this trait is required for .decode() to work
use raft::eraftpb::Message as RaftMessageParsed;
use std::sync::{mpsc::Sender, Arc};
use tonic::{Request, Response, Status};

pub struct RaftService {
    sender: Sender<consensus::Msg>,
    #[allow(dead_code)] // ToDo: Not used. remove?
    toc: Arc<TableOfContent>,
    consensus_manager: Arc<ConsensusManager>,
    // consensus_state: Option<Arc<ConsensusState>>,
}

impl RaftService {
    pub fn new(
        sender: Sender<consensus::Msg>,
        toc: Arc<TableOfContent>,
        consensus_manager: Arc<ConsensusManager>,
    ) -> Self {
        RaftService {
            sender,
            toc,
            consensus_manager,
        }
    }
}

#[tonic::async_trait]
impl Raft for RaftService {
    async fn send(&self, mut request: Request<RaftMessageBytes>) -> Result<Response<()>, Status> {
        // Here you would handle the Raft message, e.g., by forwarding it to the Raft consensus algorithm.
        // For now, we just return an empty response.

        let message_bytes = &request.get_mut().message[..];
        let message = <RaftMessageParsed>::decode(message_bytes)
            .map_err(|e| Status::internal(format!("Failed to decode Raft message: {e}")))?;

        self.sender
            .send(consensus::Msg::Raft(Box::new(message)))
            .map_err(|e| {
                Status::internal(format!("Failed to send Raft message over channel: {e}"))
            })?;

        Ok(Response::new(()))
    }

    async fn who_is(&self, _request: Request<PeerId>) -> Result<Response<Uri>, Status> {
        // Here you would implement the logic to return the URI of a peer by its ID.
        // For now, we return an empty URI.
        let uri = Uri {
            uri: "smoldb:9900".to_string(),
        };
        Ok(Response::new(uri))
    }

    /// Cancel safety??
    async fn add_peer_to_known(
        &self,
        request: Request<AddPeerToKnownMessage>,
    ) -> Result<Response<AllPeers>, Status> {
        // Here you would implement the logic to add a peer to the known peers list.
        // For now, we return an empty AllPeers response.
        let AddPeerToKnownMessage {
            id: peer_id,
            uri: peer_uri,
            port: _,
        } = request.into_inner();

        let uri = peer_uri.map(|u| u.parse::<http::Uri>().unwrap()).unwrap();

        // let consensus_state = self
        //     .consensus_manager
        //     .enabled_or_error()
        //     .map_err(|e| Status::internal(format!("Consensus is not enabled: {e}")));

        let all_peers = self
            .consensus_manager
            .add_peer(peer_id, uri)
            .await
            .map_err(|e| Status::internal(format!("Failed to add peer: {e}")))?;

        Ok(Response::new(all_peers))
    }
}
