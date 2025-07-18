use crate::{
    consensus::ConsensusState,
    storage::{error::ConsensusError, toc::TableOfContent},
    types::PeerId,
};
use http::Uri;
use std::sync::Arc;

pub async fn add_peer_to_toc_and_consensus_state(
    consensus_state: &Arc<ConsensusState>,
    toc: &Arc<TableOfContent>,
    peer_id: PeerId,
    uri: Uri,
) -> Result<(PeerId, Vec<(PeerId, String)>), ConsensusError> {
    let (peer_id, all_peers) = consensus_state.add_peer(peer_id, uri).await.map_err(|e| {
        ConsensusError::ServiceError(format!("Failed to add peer to local consensus state: {e}"))
    })?;

    {
        let collections = toc.collections.read().await;
        for (collection_name, collection) in collections.iter() {
            let mut replica_holder = collection.replica_holder.write().await;
            replica_holder
                .add_remote_shards(peer_id, collection_name.clone())
                .await
                .map_err(|e| {
                    ConsensusError::ServiceError(format!(
                        "Failed to add remote shards for collection '{collection_name}': {e}",
                    ))
                })?;
        }
    }

    Ok((peer_id, all_peers))
}
