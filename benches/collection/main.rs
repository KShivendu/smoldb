mod common;
mod index;
mod query;
mod read;
mod segment_write;
mod text_search;
mod write;

use criterion::{criterion_group, criterion_main, Criterion};
use std::{fs::File, sync::Once};
use tracing_subscriber::fmt;

static INIT: Once = Once::new();

pub fn setup_tracing() {
    INIT.call_once(|| {
        // Create or truncate the log file
        let file = File::create("bench_trace.log").unwrap();

        let result = tracing_subscriber::fmt()
            // .with_writer(|| std::io::stderr()) // Use stderr instead of stdout
            .with_writer(file)
            .with_target(false)
            .with_ansi(false) // Remove color codes
            .with_thread_ids(false)
            .with_level(false)
            // .with_env_filter(EnvFilter::from_default_env())
            .with_max_level(tracing::Level::ERROR) // Set to ERROR level for benches on CI
            .with_timer(fmt::time::uptime())
            // .pretty()
            .try_init();

        match result {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to initialize tracing: {e}");
                std::process::exit(1);
            }
        }
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = write::write, read::read, query::int_query, text_search::text_query, index::int_indexing, index::text_indexing, segment_write::segment_benches
);

criterion_main!(benches);
