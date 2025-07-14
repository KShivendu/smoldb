use crate::api::helpers;
use crate::consensus::manager::ConsensusManager;
use crate::consensus::{ConsensusOperation, Persistent};
use crate::storage::collection::{Collection, CollectionInfo};
use crate::storage::error::{CollectionError, CollectionResult, ConsensusError};
use crate::storage::toc::{CollectionOperation, TableOfContent};
use crate::types::{PeerId, ShardId};
use actix_web::{
    web::{self, Json},
    Responder,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
// Router that decides if query should go through ToC or consensus
pub struct Dispatcher {
    pub toc: Arc<TableOfContent>,
    pub consensus_manager: Option<Arc<ConsensusManager>>,
}

impl Dispatcher {
    pub fn from(
        toc: Arc<TableOfContent>,
        consensus_manager: Option<Arc<ConsensusManager>>,
    ) -> Self {
        Dispatcher {
            toc,
            consensus_manager,
        }
    }

    pub async fn get_cluster_info(&self) -> Option<Persistent> {
        if let Some(consensus_state) = &self.consensus_manager {
            Some(consensus_state.get_state().persistent.read().await.clone())
        } else {
            None
        }
    }

    pub fn get_consensus_manager(&self) -> CollectionResult<&ConsensusManager> {
        Ok(self
            .consensus_manager
            .as_ref()
            .ok_or(ConsensusError::NotEnabled())?)
    }

    pub async fn submit_collection_op(
        &self,
        operation: CollectionOperation,
    ) -> CollectionResult<()> {
        let Some(consensus_manager) = &self.consensus_manager else {
            // Do locally only if consensus is not enabled
            self.toc.perform_collection_meta_op(operation).await?;
            return Ok(());
        };

        // ToDo: Await consensus operations before committing locally
        consensus_manager
            .propose_consensus_op(ConsensusOperation::CollectionOp(operation.clone()))
            .await?;
        self.toc.perform_collection_meta_op(operation).await?;

        Ok(())
    }

    // Send a consensus operation to the consensus manager
    pub async fn send_operation(&self, operation: ConsensusOperation) -> CollectionResult<()> {
        let consensus_manager = self.get_consensus_manager()?;
        Ok(consensus_manager.propose_consensus_op(operation).await?)
    }
}

#[actix_web::get("/collections")]
async fn get_collections(dispatcher: web::Data<Dispatcher>) -> impl Responder {
    helpers::time(async {
        let collections = dispatcher
            .toc
            .collections
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        Ok(collections)
    })
    .await
}

#[actix_web::get("/collections/{collection_name}")]
async fn get_collection(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();

        if let Some(collection) = dispatcher
            .toc
            .collections
            .read()
            .await
            .get(&collection_name)
        {
            return Ok(CollectionInfo::from(collection).await);
        }

        Err(CollectionError::ServiceError(format!(
            "Collection: {collection_name} doesn't exist",
        )))
    })
    .await
}

#[actix_web::delete("/collections/{collection_name}")]
async fn delete_collection(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();

        dispatcher
            .submit_collection_op(CollectionOperation::DeleteCollection {
                collection_name: collection_name.clone(),
            })
            .await?;

        Ok(true)
    })
    .await
}

#[derive(Serialize)]
pub struct CollectionClusterLocalShard {
    pub shard_id: ShardId,
    pub point_count: usize,
    pub state: String,
}

#[derive(Serialize)]
pub struct CollectionClusterRemoteShard {
    pub peer_id: PeerId,
    pub shard_id: ShardId,
    pub state: String,
}

#[derive(Serialize)]
pub struct CollectionClusterInfo {
    pub peer_id: PeerId,
    pub shard_count: usize,
    pub local_shards: Vec<CollectionClusterLocalShard>,
    pub remote_shards: Vec<CollectionClusterRemoteShard>,
}

impl CollectionClusterInfo {
    pub async fn from(collection: &Collection) -> Self {
        let replica_holder = collection.replica_holder.read().await;
        let peer_id = 0;

        let local_shards = replica_holder
            .shards
            .iter()
            .map(|(shard_id, replica_set)| CollectionClusterLocalShard {
                // FixMe: Not all replicas will have a local shard
                shard_id: *shard_id,
                point_count: replica_set.local.count_points(),
                state: "Active".to_string(), // ToDo: Placeholder for actual state
            })
            .collect::<Vec<_>>();

        let mut remote_shards = vec![];
        for (_, replica_set) in replica_holder.shards.iter() {
            for remote_shard in replica_set.remotes.iter() {
                remote_shards.push(CollectionClusterRemoteShard {
                    peer_id: remote_shard.peer_id,
                    shard_id: remote_shard.id,
                    state: "Active".to_string(), // ToDo: Placeholder for actual state
                });
            }
        }

        CollectionClusterInfo {
            peer_id,
            shard_count: replica_holder.shards.len(),
            local_shards,
            remote_shards,
        }
    }
}

#[actix_web::get("/collections/{collection_name}/cluster")]
async fn get_collection_cluster_info(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();

        if let Some(collection) = dispatcher
            .toc
            .collections
            .read()
            .await
            .get(&collection_name)
        {
            return Ok(CollectionClusterInfo::from(collection).await);
        }

        Err(CollectionError::ServiceError(format!(
            "Collection: {collection_name} doesn't exist",
        )))
    })
    .await
}

#[derive(Serialize, Deserialize)]
pub struct CreateCollection {
    pub params: String,
}

#[actix_web::put("/collections/{collection_name}")]
async fn create_collection(
    collection_name: web::Path<String>,
    operation: Json<CreateCollection>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();

        dispatcher
            .submit_collection_op(CollectionOperation::CreateCollection {
                collection_name: collection_name.clone(),
                params: operation.params.clone(),
            })
            .await?;

        Ok(true)
    })
    .await
}
