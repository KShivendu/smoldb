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

    let collections = toc.collections.read().await;
    for collection in collections.values() {
        collection.add_remote_replicas(peer_id).await;
    }

    Ok((peer_id, all_peers))
}
