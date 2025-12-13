mod common;
mod query;
mod read;
mod write;

use criterion::{criterion_group, criterion_main};

criterion_group!(
    benches,
    write::single_write,
    write::concurrent_write,
    read::single_read,
    read::concurrent_read,
    query::single_query,
    query::concurrent_query
);
criterion_main!(benches);
