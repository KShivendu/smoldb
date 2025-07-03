use crate::consensus::{ConsensusState, Persistent};
use crate::storage::content_manager::{
    Collection, CollectionConfig, CollectionInfo, CollectionMetaOperation, ShardHolder,
    TableOfContent,
};
use actix_web::{
    web::{self, Json},
    Responder,
};
use http::Uri;
use serde::Deserialize;
use serde_json::json;
use std::str::FromStr;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

// Router that decides if query should go through ToC or consensus
pub struct Dispatcher {
    pub toc: Arc<TableOfContent>,
    pub consensus_state: Option<Arc<ConsensusState>>,
}

impl Dispatcher {
    pub fn from(toc: TableOfContent, consensus_state: Option<Arc<ConsensusState>>) -> Self {
        Dispatcher {
            toc: Arc::new(toc),
            consensus_state,
        }
    }

    pub async fn get_cluster_info(&self) -> Option<Persistent> {
        if let Some(consensus_state) = &self.consensus_state {
            Some(consensus_state.persistent.read().await.clone())
        } else {
            None
        }
    }

    pub fn dummy() -> Self {
        Dispatcher {
            toc: Arc::new(TableOfContent::from(HashMap::from_iter([(
                "c1".to_string(),
                Collection {
                    id: "c1".to_string(),
                    config: CollectionConfig {
                        params: "dummy_params".to_string(),
                    },
                    shard_holder: Arc::new(RwLock::new(ShardHolder::dummy())),
                    path: "dummy_path".into(),
                },
            )]))),
            consensus_state: Some(Arc::new(ConsensusState::dummy(
                Uri::from_str("http://smoldb-dummy:9900").unwrap(),
            ))),
        }
    }
}

#[actix_web::get("/collections")]
async fn get_collections(dispatcher: web::Data<Dispatcher>) -> impl Responder {
    let collections = dispatcher
        .toc
        .collections
        .read()
        .await
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    actix_web::HttpResponse::Ok().json(collections)
}

#[actix_web::get("/collections/{collection_name}")]
async fn get_collection(
    collection_name: web::Path<String>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    let collection_name = collection_name.into_inner();

    if let Some(collection) = dispatcher
        .toc
        .collections
        .read()
        .await
        .get(&collection_name)
    {
        return actix_web::HttpResponse::Ok().json(CollectionInfo::from(collection).await);
    }

    actix_web::HttpResponse::Ok().json(json!({
        "error": format!("Collection: {} doesn't exist", collection_name)
    }))
}

#[derive(Deserialize)]
struct CreateCollection {
    params: String,
}

#[actix_web::put("/collections/{collection_name}")]
async fn create_collection(
    collection_name: web::Path<String>,
    operation: Json<CreateCollection>,
    dispatcher: web::Data<Dispatcher>,
) -> impl Responder {
    let collection_name = collection_name.into_inner();

    // ToDo: Push this to consensus instead of directly committing locally?
    let result = dispatcher
        .toc
        .perform_collection_meta_op(CollectionMetaOperation::CreateCollection {
            collection_name: collection_name.clone(),
            params: operation.params.clone(),
        })
        .await;

    if let Err(e) = result {
        return actix_web::HttpResponse::BadRequest().body(format!(
            "Failed to create collection '{collection_name}': {e}"
        ));
    }

    actix_web::HttpResponse::Created().body(format!(
        "Collection '{collection_name}' created successfully"
    ))
}
