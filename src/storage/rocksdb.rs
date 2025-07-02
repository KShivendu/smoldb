use rocksdb::{Options, DB};
use std::path::PathBuf;

pub fn open_db(path: impl Into<PathBuf>) -> DB {
    let path = path.into();

    let db = DB::open_default(&path).unwrap_or_else(|e| {
        panic!("Failed to open RocksDB at {:?}: {}", path, e);
    });
}
