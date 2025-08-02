use crate::api::dispatcher::Dispatcher;
use crate::api::helpers;
use crate::error::CollectionError;
use crate::storage::collection::{Collection, CollectionConfig, CollectionInfo};
use crate::storage::replicas::ShardState;
use crate::storage::toc::CollectionOperation;
use crate::types::{PeerId, ShardId};
use actix_web::{
    web::{self, Json},
    Responder,
};
use serde::Serialize;

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
    pub state: ShardState,
}

#[derive(Serialize)]
pub struct CollectionClusterRemoteShard {
    pub peer_id: PeerId,
    pub shard_id: ShardId,
    pub state: ShardState,
}

#[derive(Serialize)]
pub struct CollectionClusterInfo {
    pub peer_id: PeerId,
    pub shard_count: usize,
    pub local_shards: Vec<CollectionClusterLocalShard>,
    pub remote_shards: Vec<CollectionClusterRemoteShard>,
}

impl CollectionClusterInfo {
    pub async fn from(peer_id: PeerId, collection: &Collection) -> Self {
        let replica_holder = collection.replica_holder.read().await;

        let local_shards = replica_holder
            .shards
            .iter()
            .map(|(shard_id, replica_set)| CollectionClusterLocalShard {
                // FixMe: Not all replicas will have a local shard
                shard_id: *shard_id,
                point_count: replica_set.local.count_points(),
                state: replica_set.local.shard_state.clone(),
            })
            .collect::<Vec<_>>();

        let mut remote_shards = vec![];
        for replica_set in replica_holder.shards.values() {
            for remote_shard in replica_set.remotes.values() {
                remote_shards.push(CollectionClusterRemoteShard {
                    peer_id: remote_shard.peer_id,
                    shard_id: remote_shard.id,
                    state: remote_shard.state.clone(),
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
            let peer_id = dispatcher.get_peer_id().await?;
            return Ok(CollectionClusterInfo::from(peer_id, collection).await);
        }

        Err(CollectionError::ServiceError(format!(
            "Collection: {collection_name} doesn't exist",
        )))
    })
    .await
}

#[actix_web::put("/collections/{collection_name}")]
async fn create_collection(
    collection_name: web::Path<String>,
    config: Json<CollectionConfig>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    helpers::time(async {
        let collection_name = collection_name.into_inner();

        dispatcher
            .submit_collection_op(CollectionOperation::CreateCollection {
                collection_name: collection_name.clone(),
                config: config.into_inner(),
            })
            .await?;

        Ok(true)
    })
    .await
}
