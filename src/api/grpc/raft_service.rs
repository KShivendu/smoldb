use crate::{
    api::grpc::schema::{
        raft_server::Raft, AddPeerToKnownMessage, AllPeers, Peer, PeerId,
        RaftMessage as RaftMessageBytes, Uri,
    },
    consensus::{self, ConsensusState, Msg},
};
use prost_for_raft::Message as ProtocolBufferMessage; // this trait is required for .decode() to work
use raft::eraftpb::Message as RaftMessageParsed;
use std::sync::{mpsc::Sender, Arc};
use tonic::{Request, Response, Status};

pub struct RaftService {
    state: Arc<ConsensusState>,
    sender: Sender<Msg>,
}

impl RaftService {
    pub fn new(state: Arc<ConsensusState>, sender: Sender<Msg>) -> Self {
        // We can't pass Dispatcher directly because ConsensusManager has Wal that is not Send + Sync
        RaftService { state, sender }
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

        // let msg = DebuggableMessage::from(&message);
        // msg.log("Received Raft message via gRPC");

        self.sender
            .send(consensus::Msg::Raft(Box::new(message)))
            .map_err(|e| {
                Status::internal(format!(
                    "Failed to send Raft message to consensus manager: {e}"
                ))
            })?;

        Ok(Response::new(()))
    }

    async fn who_is(&self, _request: Request<PeerId>) -> Result<Response<Uri>, Status> {
        // Here you would implement the logic to return the URI of a peer by its ID.
        // For now, we return an empty URI.
        let uri = Uri {
            uri: "smoldb:9000".to_string(),
        };
        Ok(Response::new(uri))
    }

    /// Cancel safety??
    async fn add_peer_to_known(
        &self,
        request: Request<AddPeerToKnownMessage>,
    ) -> Result<Response<AllPeers>, Status> {
        let AddPeerToKnownMessage {
            id: peer_id,
            uri: peer_uri,
            port: _,
        } = request.into_inner();

        let uri = peer_uri.map(|u| u.parse::<http::Uri>().unwrap()).unwrap();

        let (this_peer_id, all_peers) = self
            .state
            .add_peer(peer_id, uri)
            .await
            .map_err(|e| Status::internal(format!("Failed to add peer: {e}")))?;

        let response = AllPeers {
            all_peers: all_peers
                .into_iter()
                .map(|(id, uri)| Peer { id, uri })
                .collect(),
            first_peer_id: this_peer_id,
        };

        Ok(Response::new(response))
    }
}
