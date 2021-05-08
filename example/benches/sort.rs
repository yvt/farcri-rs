#![no_std]
// TODO: Get rid of this conditional attribute; it's FarCri.rs's
//       implementation detail
#![cfg_attr(target_os = "none", no_main)]

use farcri::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("sort [i32; 100]", |b| {
        let mut array = [0; 100];
        for (i, x) in array.iter_mut().enumerate() {
            *x = i;
        }

        let mut flip = 0;

        b.iter(|| {
            flip = !flip;
            array.sort_unstable_by_key(|x| *x ^ flip);
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
