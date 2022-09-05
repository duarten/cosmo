use std::thread;

use cosmo::sync::{spsc::bounded, TryRecvError};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn bench_bounded_spsc(c: &mut Criterion) {
    const REPETITIONS: u64 = 10_000_000;
    let mut group = c.benchmark_group("bounded_spsc");
    group.significance_level(0.1).sample_size(10);
    group.throughput(Throughput::Elements(REPETITIONS));
    group.bench_function("send/recv pair", |b| {
        b.iter(|| {
            let (tx, rx) = bounded(1024);
            let c = thread::spawn(move || {
                for i in 0..REPETITIONS {
                    let mut opt: Result<u64, TryRecvError>;
                    while {
                        opt = rx.try_recv();
                        opt.is_err()
                    } {
                        thread::yield_now();
                    }
                    assert_eq!(i, opt.unwrap());
                }
            });
            let p = thread::spawn(move || {
                for i in 0..REPETITIONS {
                    while tx.try_send(i).is_err() {
                        thread::yield_now();
                    }
                }
            });
            c.join().unwrap();
            p.join().unwrap();
        })
    });
    group.finish();
}

criterion_group!(benches, bench_bounded_spsc);
criterion_main!(benches);
