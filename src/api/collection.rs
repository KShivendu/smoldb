use crate::storage::content_manager::{
    Collection, CollectionConfig, CollectionInfo, CollectionMetaOperation, TableOfContent,
};
use actix_web::{
    Responder,
    web::{self, Json},
};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

// Router that decides if query should go through ToC or consensus
pub struct Dispatcher {
    pub toc: Arc<TableOfContent>,
}

impl Dispatcher {
    pub fn empty() -> Self {
        Dispatcher {
            toc: Arc::new(TableOfContent {
                collections: Arc::new(RwLock::new(HashMap::new())),
            }),
        }
    }

    pub fn from(toc: TableOfContent) -> Self {
        Dispatcher { toc: Arc::new(toc) }
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
                    shards: HashMap::new(),
                    path: "dummy_path".into(),
                },
            )]))),
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
        return actix_web::HttpResponse::Ok().json(CollectionInfo::from(collection));
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
            "Failed to create collection '{}': {}",
            collection_name, e
        ));
    }

    actix_web::HttpResponse::Created().body(format!(
        "Collection '{}' created successfully",
        collection_name
    ))
}
