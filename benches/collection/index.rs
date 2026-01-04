use crate::common::{
    benchmark_group, create_temp_db, generate_integer_values, generate_text_values, BATCH_SIZE,
};
use criterion::Criterion;
use smoldb::storage::index::{
    integer::IntegerIndex, payload_index::FieldIndexTrait, text::TextIndex,
};

// Todo: payload_index should have dedicated benches binary that's not part of collection benches?

// Integer index benchmarks

pub fn int_indexing(c: &mut Criterion) {
    let mut group = benchmark_group(c, "index/int");

    // For now only upserting BATCH_SIZE points at a time, but must upsert NUM_POINTS points in future
    // once indexing is faster due to batching.
    let num_points = BATCH_SIZE as u64;
    let point_ids: Vec<u64> = (0..num_points).collect();
    let values = generate_integer_values(num_points as usize);

    {
        let (db, _tempdir) = create_temp_db();
        let index = IntegerIndex::open(&db, "price", false).unwrap();

        group.bench_function("batch", |b| {
            b.iter(|| {
                // todo: Add batching
                index.add_points(&point_ids, &values).unwrap();
            });
        });
    }

    {
        let (db, _tempdir) = create_temp_db();
        let index_with_in_mem = IntegerIndex::open(&db, "price", true).unwrap();

        group.bench_function("in_memory/batch", |b| {
            b.iter(|| {
                index_with_in_mem.add_points(&point_ids, &values).unwrap();
            });
        });
    }
}

// Text index benchmarks

pub fn text_indexing(c: &mut Criterion) {
    let mut group = benchmark_group(c, "index/text");

    // For now only upserting BATCH_SIZE points at a time, but must upsert NUM_POINTS points in future
    // once indexing is faster due to batching.
    let num_points = BATCH_SIZE as u64;
    let point_ids: Vec<u64> = (0..num_points).collect();
    let values = generate_text_values(num_points as usize);

    group.bench_function("batch", |b| {
        let (db, _tempdir) = create_temp_db();
        let index = TextIndex::open(&db, "description", false).unwrap();

        b.iter(|| {
            // todo: Add batching
            index.add_points(&point_ids, &values).unwrap();
        });
    });

    group.bench_function("in_memory/batch", |b| {
        let (db, _tempdir) = create_temp_db();
        let index_with_in_mem = TextIndex::open(&db, "description", true).unwrap();

        b.iter(|| {
            // todo: Add batching
            index_with_in_mem.add_points(&point_ids, &values).unwrap();
        });
    });
}
