use crate::{
    api::grpc::smoldb_p2p_grpc::{
        raft_server::Raft, AddPeerToKnownMessage, AllPeers, Peer, PeerId,
        RaftMessage as RaftMessageBytes, Uri,
    },
    consensus::{self, ConsensusState},
};
use prost_for_raft::Message as ProtocolBufferMessage; // this trait is required for .decode() to work
use raft::eraftpb::Message as RaftMessageParsed;
use std::sync::{mpsc::Sender, Arc};
use tonic::{Request, Response, Status};

pub struct RaftService {
    sender: Sender<consensus::Msg>,
    consensus_state: Option<Arc<ConsensusState>>,
}

impl RaftService {
    pub fn new(
        sender: Sender<consensus::Msg>,
        consensus_state: Option<Arc<ConsensusState>>,
    ) -> Self {
        RaftService {
            sender,
            consensus_state,
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

    async fn add_peer_to_known(
        &self,
        request: Request<AddPeerToKnownMessage>,
    ) -> Result<Response<AllPeers>, Status> {
        // Here you would implement the logic to add a peer to the known peers list.
        // For now, we return an empty AllPeers response.
        let request = request.into_inner();

        let consensus_state = self
            .consensus_state
            .as_ref()
            .ok_or_else(|| Status::internal("Consensus state is not available in RaftService"))?;

        let uri = request
            .uri
            .map(|u| u.parse::<http::Uri>().unwrap())
            .unwrap();

        consensus_state
            .add_peer(request.id, uri)
            .await
            .map_err(|e| Status::internal(format!("Failed to add peer: {e}")))?;

        let persistent = consensus_state.persistent.read().await.clone();

        let all_peers = persistent
            .peers
            .into_iter()
            .map(|(id, uri)| Peer { id, uri })
            .collect();

        let this_peer_id = persistent.peer_id;

        let all_peers = AllPeers {
            all_peers,
            first_peer_id: this_peer_id,
        };

        Ok(Response::new(all_peers))
    }
}
