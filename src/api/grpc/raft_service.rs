use crate::{
    api::grpc::smoldb_p2p_grpc::{
        raft_server::Raft, AddPeerToKnownMessage, AllPeers, PeerId,
        RaftMessage as RaftMessageBytes, Uri,
    },
    consensus,
};
use prost_for_raft::Message as ProtocolBufferMessage; // this trait is required for .decode() to work
use raft::eraftpb::Message as RaftMessageParsed;
use std::sync::mpsc::Sender;
use tonic::{Request, Response, Status};

pub struct RaftService {
    sender: Sender<consensus::Msg>,
}

impl RaftService {
    pub fn new(sender: Sender<consensus::Msg>) -> Self {
        RaftService { sender }
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

    async fn add_peer_to_known(
        &self,
        _request: Request<AddPeerToKnownMessage>,
    ) -> Result<Response<AllPeers>, Status> {
        // Here you would implement the logic to add a peer to the known peers list.
        // For now, we return an empty AllPeers response.
        let all_peers = AllPeers {
            all_peers: vec![],
            first_peer_id: 0,
        };
        Ok(Response::new(all_peers))
    }
}
