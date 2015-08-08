extern crate cosmo;
extern crate test;
extern crate time;

use self::test::Bencher;
use std::sync::{Arc, Barrier};
use std::thread;
use self::time::PreciseTime;
use self::cosmo::collection::ConcurrentQueue;
use self::cosmo::collection::SpscConcurrentQueue;

#[bench]
fn offer_poll(b: &mut Bencher) {
    let q = SpscConcurrentQueue::<u64>::with_capacity(1024);
    b.iter(|| {
        q.offer(10);
        test::black_box(q.poll());
    });
}

#[bench]
fn throughput(b: &mut Bencher) {
    const REPETITIONS: u64 = 10_000_000;
    let q = SpscConcurrentQueue::<u64>::with_capacity(1024);
    b.iter(|| {
        let barrier = Arc::new(Barrier::new(2));
        let pc = barrier.clone();
        let pq = q.clone();
        thread::spawn(move|| {
            pc.wait();
            for i in 0..REPETITIONS {
                while pq.offer(i).is_some() {
                    thread::yield_now();
                }
            }
        });

        barrier.wait();

        let start = PreciseTime::now();

        for _ in 0..REPETITIONS as u64 {
            let mut opt: Option<u64>;
            while {
                opt = q.poll();
                opt.is_none()
            } {
                thread::yield_now();
            }
            test::black_box(opt);
        }

        let end = PreciseTime::now();
        let duration = start.to(end).num_nanoseconds().unwrap() as u64;
        let ops = (REPETITIONS * 1000_000_000) / duration;
        let op = duration as f64 / REPETITIONS as f64; 
        println!("SPSC Queue - ops/sec={} - ns/op={:.3}", ops, op);
    });
}

