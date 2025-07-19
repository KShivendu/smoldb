use crate::api::grpc::p2p_grpc_schema::RaftMessage as GrpcRaftMessage;
use crate::{
    api::grpc::{make_default_grpc_channel, p2p_grpc_schema::raft_client::RaftClient},
    consensus::Consensus,
    storage::error::CollectionResult,
};
use http::Uri;
use prost_for_raft::Message as RaftMessageTrait;
use raft::prelude::Message as RaftMessage;
use tonic::{transport::Channel, Request};

pub async fn get_raft_client(uri: Uri) -> CollectionResult<RaftClient<Channel>> {
    let channel = make_default_grpc_channel(uri.clone()).await?;
    Ok(RaftClient::new(channel))
}

impl Consensus {
    /// Sends Raft messages via p2p gRPC APIs to other peers
    pub async fn send_messages(&mut self, messages: Vec<RaftMessage>) {
        for message in messages {
            let bytes = <RaftMessage as RaftMessageTrait>::encode_to_vec(&message);
            let req = GrpcRaftMessage { message: bytes };

            let destination_peer = message.to;

            if let Ok(uri) = self.consensus_state.get_peer_uri(destination_peer).await {
                if let Ok(mut client) = get_raft_client(uri).await {
                    // ToDo: Ignoring errors for now. But should be propagated
                    let _ = client.send(Request::new(req.clone())).await;
                }
            }
        }
    }
}
