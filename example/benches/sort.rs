#![no_std]
// TODO: Get rid of this conditional attribute; it's FarCri.rs's
//       implementation detail
#![cfg_attr(target_os = "none", no_main)]

use farcri::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("noop", |b| b.iter(noop));

    let mut array = [0; 256];
    let mut flip = 0;
    for (i, x) in array.iter_mut().enumerate() {
        *x = i;
    }

    let mut group = c.benchmark_group("sort [i32]");
    for &len in &[1, 4, 16, 64, 256] {
        group.throughput(Throughput::Elements(len as _));
        group.bench_function(BenchmarkId::from_parameter(&len), |b| {
            b.iter(|| {
                flip = !flip;
                array[..len].sort_unstable_by_key(|x| *x ^ flip);
            })
        });
    }
    drop(group);
}

#[inline(never)]
fn noop() {}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
