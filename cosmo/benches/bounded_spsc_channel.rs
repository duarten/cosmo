use std::{pin::Pin, thread};

use cosmo::sync::{channel::spsc::bounded, RecvError};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use futures_task::{noop_waker, Context, Poll};
use futures_util::Future;

fn bench_bounded_spsc_channel(c: &mut Criterion) {
    const REPETITIONS: u64 = 10_000_000;
    let mut group = c.benchmark_group("bounded_spsc_channel");
    group.significance_level(0.1).sample_size(10);
    group.throughput(Throughput::Elements(REPETITIONS));
    group.bench_function("send/recv pair", |b| {
        b.iter(|| {
            let (tx, rx) = bounded::<u64>(1024);
            let c = thread::spawn(move || {
                let waker = noop_waker();
                let mut cx = Context::from_waker(&waker);
                for i in 0..REPETITIONS {
                    let mut res: Poll<Result<u64, RecvError>>;
                    while {
                        res = unsafe { Pin::new_unchecked(&mut rx.recv()) }.poll(&mut cx);
                        res.is_pending()
                    } {
                        thread::yield_now();
                    }
                    assert!(matches!(res, Poll::Ready(Ok(val)) if val == i));
                }
            });
            let p = thread::spawn(move || {
                let waker = noop_waker();
                let mut cx = Context::from_waker(&waker);
                for i in 0..REPETITIONS {
                    while unsafe { Pin::new_unchecked(&mut tx.send(i)) }
                        .poll(&mut cx)
                        .is_pending()
                    {
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

criterion_group!(benches, bench_bounded_spsc_channel);
criterion_main!(benches);
