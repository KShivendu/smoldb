use crate::{api::grpc::make_default_grpc_channel, types::PeerId};
use http::Uri;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tonic::transport::{Channel, Error as TonicError};

#[derive(Default)]
/// This service is used to manage [`Channel`] connection pools between peers
/// It maintains connection pools and handles re-connection
pub struct ChannelService {
    pub peer_id: PeerId,
    /// Directly shared with `ConsensusState` instead of having a .add_peer() function
    pub id_to_address: Arc<RwLock<HashMap<PeerId, Uri>>>,
    ///
    /// Note: `Channel` can only send one request in flight.
    /// ToDo: We can have a proper channel pool instead of single channel to increase this throughput
    ///
    /// Channels are created only when required
    pub uri_to_channel: tokio::sync::RwLock<HashMap<Uri, Channel>>,
}

impl ChannelService {
    pub fn new(peer_id: PeerId, id_to_address: Arc<RwLock<HashMap<PeerId, Uri>>>) -> Self {
        Self {
            peer_id,
            id_to_address,
            uri_to_channel: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    pub async fn get_or_create_channel(&self, uri: Uri) -> Result<Channel, TonicError> {
        let uri_to_channel_guard = self.uri_to_channel.read().await;

        if uri_to_channel_guard.contains_key(&uri) {
            return Ok(uri_to_channel_guard.get(&uri).unwrap().clone());
        }

        drop(uri_to_channel_guard); // Explicitly drop the read lock before acquiring a write lock
        let mut uri_to_channel_guard = self.uri_to_channel.write().await;
        let channel = make_default_grpc_channel(uri.clone()).await?;
        uri_to_channel_guard.insert(uri.clone(), channel.clone());

        Ok(channel)
    }

    pub async fn get_other_peer_ids(&self) -> Vec<PeerId> {
        let id_to_address = self.id_to_address.read().await;
        id_to_address
            .keys()
            .cloned()
            .filter(|&id| id != self.peer_id)
            .collect()
    }
}
