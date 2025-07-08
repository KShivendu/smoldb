use std::{collections::HashMap, sync::Arc};

use http::Uri;
use tokio::sync::RwLock;
use tonic::transport::{Channel, Error as TonicError};

use crate::{api::grpc::make_default_grpc_channel, types::PeerId};

/// Holds a pool of channels established for a set of URIs.
/// Channel are shared by cloning them.
/// Make the `pool_size` larger to increase throughput.
pub struct TransportChannelPool {
    uri_to_channel: tokio::sync::RwLock<HashMap<Uri, Channel>>,
}

impl TransportChannelPool {
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
}

impl Default for TransportChannelPool {
    fn default() -> Self {
        Self {
            uri_to_channel: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

#[derive(Clone, Default)]
pub struct ChannelService {
    /// Shared with consensus state
    pub id_to_address: Arc<RwLock<HashMap<PeerId, Uri>>>,
    pub channel_pool: Arc<TransportChannelPool>,
}

impl ChannelService {
    pub fn new(id_to_address: Arc<RwLock<HashMap<PeerId, Uri>>>) -> Self {
        Self {
            id_to_address,
            channel_pool: Arc::new(TransportChannelPool::default()),
        }
    }
}
