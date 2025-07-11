use crate::{
    api::points::PointsOperation,
    channel_service::ChannelService,
    storage::{
        collection::{Collection, CollectionConfig, CollectionName, COLLECTION_CONFIG_FILE},
        error::StorageError,
        segment::{Point, PointId},
    },
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

pub const COLLECTIONS_DIR: &str = "collections";

pub struct TableOfContent {
    pub collections: Arc<RwLock<Collections>>,
    pub channel_service: ChannelService,
}

pub type Collections = HashMap<CollectionName, Collection>;

pub enum CollectionMetaOperation {
    CreateCollection {
        collection_name: String,
        params: String,
    },
    DeleteCollection {
        collection_name: String,
    },
}

impl TableOfContent {
    pub fn load(channel_service: ChannelService) -> Self {
        let collections_path = Path::new("storage").join(COLLECTIONS_DIR);
        std::fs::create_dir_all(&collections_path).expect("Failed to create collections directory");

        // Load collections from the directory
        let mut collections = HashMap::new();
        let collection_paths =
            std::fs::read_dir(&collections_path).expect("Failed to read collections directory");
        for dir_entry in collection_paths {
            let path = dir_entry.expect("Can't read directory entry").path();

            if !path.join(COLLECTION_CONFIG_FILE).exists() {
                // Skip directories without a config file
                // This indirectly also checks if the path is a directory
                println!(
                    "Skipping path {} as it does not contain a collection config file",
                    path.display()
                );
                continue;
            }

            let collection_name = path
                .file_name()
                .expect("Can't resolve filename for collection")
                .to_str()
                .expect("Collection name is not valid UTF-8")
                .to_string();

            let collection = Collection::load(collection_name, &path)
                .expect("Failed to load collection from path");

            collections.insert(collection.id.clone(), collection);
        }

        TableOfContent {
            collections: Arc::new(RwLock::new(collections)),
            channel_service,
        }
    }

    /// Creates a new directory at the expected collection path.
    pub async fn mkdir_collection_dir(collection_name: &str) -> Result<PathBuf, StorageError> {
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
            StorageError::ServiceError(format!("Can't create directory for collection: {e}"))
        })?;

        Ok(path)
    }

    pub async fn perform_collection_meta_op(
        &self,
        operation: CollectionMetaOperation,
    ) -> Result<bool, StorageError> {
        match operation {
            CollectionMetaOperation::CreateCollection {
                collection_name,
                params,
            } => {
                println!("Creating collection {collection_name}");
                let path = Self::mkdir_collection_dir(&collection_name).await?;

                let collection =
                    Collection::init(collection_name.clone(), CollectionConfig { params }, &path)
                        .await?;

                {
                    let mut write_collections = self.collections.write().await;
                    if write_collections.contains_key(&collection_name) {
                        return Err(StorageError::BadInput(format!(
                            "Collection with name '{collection_name}' already exists"
                        )));
                    }
                    write_collections.insert(collection_name, collection);
                }
                Ok(true)
            }
            CollectionMetaOperation::DeleteCollection { collection_name } => {
                println!("Deleting collection {collection_name}");
                let mut write_collections = self.collections.write().await;

                // ToDo: Will the order of removing from hashmap vs deleting the directory matter?
                if let Some(collection) = write_collections.remove(&collection_name) {
                    collection.delete()?;
                } else {
                    return Err(StorageError::BadInput(format!(
                        "Collection with name '{collection_name}' does not exist"
                    )));
                }

                Ok(true)
            }
        }
    }

    pub async fn perform_points_op(
        &self,
        collection_name: &str,
        operation: PointsOperation,
    ) -> Result<bool, StorageError> {
        // ToDo: Have independent read locks for each collection. It should improve perf?
        let collections = self.collections.read().await;
        let collection = collections.get(collection_name).ok_or_else(|| {
            StorageError::BadInput(format!("Collection '{collection_name}' does not exist"))
        })?;

        match operation {
            PointsOperation::Upsert(upsert_points) => {
                collection
                    .upsert_points(&upsert_points.points, false)
                    .await
                    .map_err(|e| {
                        StorageError::ServiceError(format!(
                            "Failed to upsert points in collection '{collection_name}': {e}"
                        ))
                    })?;
            }
        }

        Ok(true)
    }

    pub async fn retrieve_points(
        &self,
        collection_name: &str,
        ids: Option<Vec<PointId>>,
    ) -> Result<Vec<Point>, StorageError> {
        let collections = self.collections.read().await;
        let collection = collections.get(collection_name).ok_or_else(|| {
            StorageError::BadInput(format!("Collection '{collection_name}' does not exist"))
        })?;

        collection.get_points(ids, None, false).await.map_err(|e| {
            StorageError::ServiceError(format!(
                "Failed to retrieve points from collection '{collection_name}': {e}"
            ))
        })
    }
}
