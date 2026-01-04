mod common;
mod query;
mod read;
mod text_search;
mod write;

use criterion::{criterion_group, criterion_main, Criterion};

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = write::write, read::read, query::int_query, text_search::text_query
);
criterion_main!(benches);
