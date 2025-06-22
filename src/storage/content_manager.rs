use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

pub struct TableOfContent {
    pub collections: Arc<RwLock<Collections>>,
}

#[derive(Serialize)]
pub struct CollectionConfig {
    pub params: String,
}

pub type CollectionId = String;

#[derive(Serialize)]
pub struct Collection {
    pub id: CollectionId,
    pub collection_config: CollectionConfig,
}

pub type Collections = HashMap<CollectionId, Collection>;

pub enum CollectionMetaOperation {
    CreateCollection {
        collection_name: String,
        params: String,
    },
}

impl TableOfContent {
    pub fn from(collections: Collections) -> Self {
        TableOfContent {
            collections: Arc::new(RwLock::new(collections)),
        }
    }

    pub async fn perform_collection_meta_op(&self, operation: CollectionMetaOperation) {
        match operation {
            CollectionMetaOperation::CreateCollection {
                collection_name,
                params,
            } => {
                println!("Creating collection {}", collection_name);
                self.collections.write().await.insert(
                    collection_name.clone(),
                    Collection {
                        id: collection_name,
                        collection_config: CollectionConfig { params },
                    },
                );
            }
        }
    }
}
