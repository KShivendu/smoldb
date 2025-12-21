use criterion::Criterion;
use smoldb::storage::segment::Segment;
use std::collections::BTreeMap;
use tracing::info_span;

use crate::common::{benchmark_group, create_tempdir, generate_points};

// Takes 9.0491 µs on my machine
pub fn segment_benches(c: &mut Criterion) {
    let mut group = benchmark_group(c, "segment");

    {
        let tempdir = create_tempdir();
        let segment = Segment::create(tempdir.path(), BTreeMap::new()).unwrap();
        let points = generate_points(1);

        group.bench_function("write/single", |b| {
            b.iter(|| {
                let _span = info_span!("segment single write").entered();
                segment.insert_points(&points).unwrap();
            });
        });
    }
}
