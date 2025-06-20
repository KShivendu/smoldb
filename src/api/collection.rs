use std::{collections::HashMap, sync::Arc};

use actix_web::{Responder, web};
use serde::Serialize;
use serde_json::json;
use tokio::sync::RwLock;

#[derive(Serialize)]
pub struct CollectionConfig {
    params: String,
}

pub type CollectionId = String;

#[derive(Serialize)]
pub struct Collection {
    id: CollectionId,
    collection_config: CollectionConfig,
}

pub type Collections = HashMap<CollectionId, Collection>;

pub struct TableOfContent {
    collections: Arc<RwLock<Collections>>,
}

// Router that decides if query should go through ToC or consensus
pub struct Dispatcher {
    toc: Arc<TableOfContent>,
}

impl Dispatcher {
    pub fn empty() -> Self {
        Dispatcher {
            toc: Arc::new(TableOfContent {
                collections: Arc::new(RwLock::new(HashMap::new())),
            }),
        }
    }

    pub fn dummy() -> Self {
        Dispatcher {
            toc: Arc::new(TableOfContent {
                collections: Arc::new(RwLock::new(HashMap::from_iter([(
                    "c1".to_string(),
                    Collection {
                        id: "c1".to_string(),
                        collection_config: CollectionConfig {
                            params: "dummy_params".to_string(),
                        },
                    },
                )]))),
            }),
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
        return actix_web::HttpResponse::Ok().json(collection);
    }

    actix_web::HttpResponse::Ok().json(json!({
        "error": format!("Collection: {} doesn't exist", collection_name)
    }))
}

#[actix_web::put("/collections/{collection_name}")]
async fn create_collection(collection_name: web::Path<String>) -> impl Responder {
    let collection = format!(
        "Collection '{}' created successfully",
        collection_name.into_inner()
    );
    actix_web::HttpResponse::Created().body(collection)
}
