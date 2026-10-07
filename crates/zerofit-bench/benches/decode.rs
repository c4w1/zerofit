//! Throughput of zerofit vs fitparser on every fixture.
//!
//! ```sh
//! cargo bench -p zerofit-bench --bench decode
//! ```
//!
//! Two groups per fixture: `bytes/<fixture>` reports MB/s and
//! `messages/<fixture>` reports data messages per second.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use zerofit_bench as w;

fn bench(c: &mut Criterion) {
    for f in w::fixtures() {
        let bytes = &f.bytes;
        for (group, throughput) in [
            ("bytes", Throughput::Bytes(bytes.len() as u64)),
            ("messages", Throughput::Elements(f.messages)),
        ] {
            let mut g = c.benchmark_group(format!("{group}/{}", f.name));
            g.throughput(throughput);
            g.bench_function("zerofit_records", |b| {
                b.iter(|| w::records(black_box(bytes)));
            });
            g.bench_function("zerofit_all_fields", |b| {
                b.iter(|| w::all_fields(black_box(bytes)));
            });
            g.bench_function("zerofit_profile_fields", |b| {
                b.iter(|| w::profile_fields(black_box(bytes)));
            });
            g.bench_function("zerofit_stream", |b| {
                b.iter(|| w::stream(black_box(bytes)));
            });
            g.bench_function("fitparser", |b| {
                b.iter(|| w::fitparser(black_box(bytes)));
            });
            g.finish();
        }
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
