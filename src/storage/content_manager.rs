use crate::storage::error::StorageError;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

pub const COLLECTIONS_DIR: &str = "collections";

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
    pub config: CollectionConfig,
    pub path: PathBuf,
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

    /// Creates a new directory at the expected collection path.
    pub async fn mkdir_collection_dir(
        &self,
        collection_name: &str,
    ) -> Result<PathBuf, StorageError> {
        let path = Path::new("storage")
            .join(COLLECTIONS_DIR)
            .join(collection_name);

        if path.exists() {
            return Err(StorageError::BadInput(format!(
                "Collection path already exists: {}",
                path.display()
            )));
        }

        tokio::fs::create_dir_all(&path).await.map_err(|e| {
            StorageError::ServiceError(format!("Can't create directory for collection: {}", e))
        })?;

        Ok(path)
    }

    pub async fn perform_collection_meta_op(&self, operation: CollectionMetaOperation) {
        match operation {
            CollectionMetaOperation::CreateCollection {
                collection_name,
                params,
            } => {
                println!("Creating collection {}", collection_name);
                let path = self
                    .mkdir_collection_dir(&collection_name)
                    .await
                    .expect("Failed to create collection directory");

                let collection = Collection {
                    id: collection_name.clone(),
                    config: CollectionConfig { params },
                    path,
                };

                self.collections
                    .write()
                    .await
                    .insert(collection_name, collection);
            }
        }
    }
}
