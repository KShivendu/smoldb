use crate::api::grpc::p2p_grpc_schema::RaftMessage as GrpcRaftMessage;
use crate::{
    api::grpc::{make_default_grpc_channel, p2p_grpc_schema::raft_client::RaftClient},
    consensus::Consensus,
    storage::error::CollectionResult,
    types::PeerId,
};
use prost_for_raft::Message as RaftMessageTrait;
use raft::prelude::Message as RaftMessage;
use std::{collections::HashMap, str::FromStr};
use tonic::{transport::Channel, Request};

pub async fn get_raft_client(peer_id: PeerId) -> CollectionResult<RaftClient<Channel>> {
    let inner_map: HashMap<_, _> = HashMap::from_iter(vec![
        (101, http::Uri::from_str("http://0.0.0.0:5001").unwrap()),
        (102, http::Uri::from_str("http://0.0.0.0:5002").unwrap()),
        (103, http::Uri::from_str("http://0.0.0.0:5003").unwrap()),
    ]);

    let uri = inner_map
        .get(&peer_id)
        .expect("Peer ID not found in the channel map")
        .clone();

    let channel = make_default_grpc_channel(uri.clone()).await?;

    Ok(RaftClient::new(channel))
}

impl Consensus {
    /// Sends Raft messages via p2p gRPC APIs to other peers
    pub async fn send_messages(&mut self, messages: Vec<RaftMessage>) {
        for message in messages {
            let _ = self.send_message(message).await;
        }
    }

    async fn send_message(&mut self, message: RaftMessage) -> CollectionResult<()> {
        println!("Sending message to other peers {message:?}");
        let remotes = vec![101, 102, 103]; // Example peer IDs

        let bytes = <RaftMessage as RaftMessageTrait>::encode_to_vec(&message);
        let req = GrpcRaftMessage { message: bytes };

        for peer_id in remotes {
            if let Ok(mut client) = get_raft_client(peer_id).await {
                // ToDo: Ignoring errors for now. But should be propagated
                let _ = client.send(Request::new(req.clone())).await;
            }
        }

        Ok(())
    }
}
